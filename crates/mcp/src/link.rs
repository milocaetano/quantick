//! The port to a running instance, and its real implementation.
//!
//! [`ControlLink`] is what the tool layer talks to: list the live instances,
//! invoke one capability on one of them. [`LocalLink`] implements it over
//! [`quantick_control_local`] — discovery in the private descriptor directory,
//! then the blocking loopback client — and applies the instance-routing rules
//! of contract §8: one live instance is selected when none is named, none is
//! `control.instance_gone`, several without a choice is
//! `control.instance_ambiguous`, and `--instance` pins every call. Nothing
//! here can start the application.
//!
//! [`crate::fake::FakeLink`] is the second implementation, for tests.

use std::{collections::BTreeMap, path::PathBuf};

use quantick_control::{
    error::{ControlError, codes},
    id::{ErrorCode, InstanceId},
    wire::ResponseEnvelope,
};
use quantick_control_local::{
    client::{ConnectOptions, LiveInstances, LocalClient, discover, discover_in},
    discovery::{DiscoveryError, discover_descriptors, discover_descriptors_in},
};
use serde::Serialize;
use serde_json::Value;

/// One live instance as `quantick_describe` lists it without an ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InstanceSummary {
    pub instance_id: InstanceId,
    pub application_version: String,
    pub application_commit: String,
    pub process_id: u32,
    pub published_at_unix_ms: i64,
}

/// The result of one discovery pass: live instances in the contract's
/// deterministic order, plus the descriptors that were found but could not be
/// used and what to do when there is nothing to use.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Instances {
    pub instances: Vec<InstanceSummary>,
    pub issues: Vec<String>,
    pub next_steps: Vec<String>,
}

/// What the tools need from a running instance. Implemented over the local
/// transport by [`LocalLink`] and by [`crate::fake::FakeLink`] for tests.
pub trait ControlLink {
    /// Discover the live instances now. Creates and starts nothing.
    fn instances(&mut self) -> Result<Instances, ControlError>;

    /// Invoke one capability on one instance. `None` selects the single live
    /// instance, or fails with the contract's routing errors.
    fn invoke(
        &mut self,
        instance: Option<&InstanceId>,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<ResponseEnvelope, ControlError>;
}

/// The real link: discovery plus one cached authenticated connection per
/// instance, reopened after a transport failure.
pub struct LocalLink {
    options: ConnectOptions,
    directory: Option<PathBuf>,
    pinned: Option<InstanceId>,
    clients: BTreeMap<InstanceId, LocalClient>,
}

impl LocalLink {
    /// `directory` overrides the platform's private runtime directory (for
    /// tests and tools that point at their own); `pinned` is `--instance`.
    pub fn new(
        options: ConnectOptions,
        directory: Option<PathBuf>,
        pinned: Option<InstanceId>,
    ) -> Self {
        Self {
            options,
            directory,
            pinned,
            clients: BTreeMap::new(),
        }
    }

    fn discover(&self) -> Result<LiveInstances, ControlError> {
        let live = match &self.directory {
            Some(directory) => discover_in(directory, &self.options),
            None => discover(&self.options),
        };
        live.map_err(discovery_failed)
    }

    /// The instance a call targets, honouring the pin. An explicit ID that
    /// contradicts the pin is refused rather than silently overridden.
    fn wanted(&self, instance: Option<&InstanceId>) -> Result<Option<InstanceId>, ControlError> {
        match (&self.pinned, instance) {
            (Some(pinned), Some(asked)) if pinned != asked => Err(ControlError::invalid_request(
                "instance_id differs from the instance this adapter was pinned to with --instance",
            )),
            (Some(pinned), _) => Ok(Some(pinned.clone())),
            (None, Some(asked)) => Ok(Some(asked.clone())),
            (None, None) => Ok(None),
        }
    }

    fn connection(&mut self, instance: Option<&InstanceId>) -> Result<InstanceId, ControlError> {
        let wanted = self.wanted(instance)?;
        if let Some(id) = &wanted
            && self.clients.contains_key(id)
        {
            return Ok(id.clone());
        }
        // Without a named instance the contract's "exactly one live instance"
        // rule is about liveness now: a cached connection could hide a second
        // window opened since. Re-reading the descriptors is cheap and tells
        // whether the world still holds exactly the instance the cache holds;
        // only when it does not is the full discovery — connect and handshake
        // against every candidate — worth its cost.
        if wanted.is_none()
            && let Some(only) = self.sole_advertised_instance()
            && self.clients.contains_key(&only)
        {
            return Ok(only);
        }
        let live = self.discover()?;
        let client = live.select(wanted.as_ref())?;
        let id = client.descriptor().instance_id.clone();
        self.clients.insert(id.clone(), client);
        Ok(id)
    }

    /// The instance ID of the one advertised descriptor, when exactly one is
    /// advertised and readable; `None` otherwise (including when discovery
    /// itself fails, which the full path then reports properly).
    fn sole_advertised_instance(&self) -> Option<InstanceId> {
        let report = match &self.directory {
            Some(directory) => discover_descriptors_in(directory),
            None => discover_descriptors(),
        }
        .ok()?;
        match report.candidates.as_slice() {
            [only] => Some(only.descriptor.instance_id.clone()),
            _ => None,
        }
    }
}

impl ControlLink for LocalLink {
    fn instances(&mut self) -> Result<Instances, ControlError> {
        let live = self.discover()?;
        let issues = live
            .issues
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect();
        let next_steps = live.next_steps.clone();
        // Instances that are gone leave the cache with their sockets; a live
        // one keeps the connection it already has.
        let live_ids = live.instance_ids();
        self.clients.retain(|id, _| live_ids.contains(id));
        // The pin applies to the listing as to every call (contract §8): a
        // pinned adapter lists its instance, and fails when it is gone.
        let clients = match &self.pinned {
            Some(pinned) => vec![live.select(Some(pinned))?],
            None => live.clients,
        };
        let mut instances = Vec::with_capacity(clients.len());
        for client in clients {
            let descriptor = client.descriptor();
            let id = descriptor.instance_id.clone();
            instances.push(InstanceSummary {
                instance_id: id.clone(),
                application_version: descriptor.application_version.clone(),
                application_commit: descriptor.application_commit.clone(),
                process_id: descriptor.process_id,
                published_at_unix_ms: descriptor.published_at_unix_ms,
            });
            // Keep the authenticated connection for the calls that follow.
            self.clients.entry(id).or_insert(client);
        }
        Ok(Instances {
            instances,
            issues,
            next_steps,
        })
    }

    fn invoke(
        &mut self,
        instance: Option<&InstanceId>,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<ResponseEnvelope, ControlError> {
        let id = self.connection(instance)?;
        let client = self
            .clients
            .get_mut(&id)
            .expect("a connection was just selected or cached");
        // A parked wait answers only when the journal moves or its timeout
        // elapses, which is longer than an ordinary request's patience; the
        // client waits for the wait's own timeout plus the request timeout,
        // so the gateway's structured reply is received rather than raced.
        let patience = (capability_id == crate::tools::EVENTS_WAIT_CAPABILITY)
            .then(|| payload.get("timeout_ms").and_then(Value::as_u64))
            .flatten();
        let outcome: Result<ResponseEnvelope, ControlError> = (|| {
            let request_id = client.send_versioned(capability_id, capability_version, payload)?;
            loop {
                let response = match patience {
                    Some(timeout_ms) => client.read_with_extra_patience(timeout_ms)?,
                    None => client.read()?,
                };
                if response.request_id == request_id {
                    return Ok(response);
                }
            }
        })();
        if let Err(error) = &outcome
            && error.code.as_str() == codes::INSTANCE_GONE
        {
            // The socket is no longer trustworthy; the next call rediscovers.
            self.clients.remove(&id);
        }
        outcome
    }
}

/// A discovery failure is reported as the instance being unreachable: the
/// effect for the caller is the same, and the message says why.
fn discovery_failed(error: DiscoveryError) -> ControlError {
    let mut control = ControlError::new(
        ErrorCode::new(codes::INSTANCE_GONE).expect("static error code is valid"),
        format!("instance discovery failed: {error}"),
        true,
    );
    control.context.next_steps = vec![
        "Check that Quantick is open with local agent access enabled, and that the private runtime directory is readable by this user."
            .to_owned(),
    ];
    control
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn link(pinned: Option<InstanceId>) -> LocalLink {
        LocalLink::new(
            ConnectOptions::observer("test", "0", BTreeSet::new()),
            Some(crate::scratch::ScratchDir::new("link").path().to_path_buf()),
            pinned,
        )
    }

    #[test]
    fn a_pin_wins_and_a_contradicting_id_is_refused() {
        let pinned = InstanceId::from_bytes([1; 16]);
        let link = link(Some(pinned.clone()));
        assert_eq!(link.wanted(None).unwrap(), Some(pinned.clone()));
        assert_eq!(link.wanted(Some(&pinned)).unwrap(), Some(pinned));
        let other = InstanceId::from_bytes([2; 16]);
        assert_eq!(
            link.wanted(Some(&other)).unwrap_err().code.as_str(),
            codes::INVALID_REQUEST
        );
    }

    #[test]
    fn an_empty_directory_lists_nothing_with_a_next_step_and_starts_nothing() {
        let mut link = link(None);
        let directory = link.directory.clone().unwrap();
        let _ = std::fs::remove_dir_all(&directory);
        let instances = link.instances().unwrap();
        assert!(instances.instances.is_empty());
        assert!(!instances.next_steps.is_empty());
        assert!(
            !directory.exists(),
            "discovery must not create the directory"
        );
        let error = link
            .invoke(None, "control.describe", 1, serde_json::json!({}))
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert!(!error.context.next_steps.is_empty());
    }
}
