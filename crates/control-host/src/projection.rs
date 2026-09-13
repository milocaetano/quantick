//! Dynamic, on-demand snapshot projection registry.
//!
//! The registry owns no application state and is not called by the frame
//! loop. The local gateway owns one per running instance, asks it to build
//! owned DTOs on the UI thread, then moves each capture away from that thread
//! before JSON serialization.
//!
//! It is generic over the host state `H` its projectors read, so the
//! registry — scope validation, revision tracking, the capture budget — lives
//! here and the application supplies only `H` and the projectors. Time comes
//! from the [`HostClock`] handed to [`ProjectionRegistry::new`].

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use quantick_control::{
    error::ControlError,
    id::{InstanceId, ModuleId, PermissionId, SnapshotScopeId},
    limits::{CONTROL_MAX_SNAPSHOT_SCOPES, CONTROL_UI_BUDGET_US},
    registry::ModuleDescriptor,
    schema::{generated_schema, validate_schema},
    wire::{ModuleRevision, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::HostClock;

/// Metadata shared by every scope projected in one coherent capture.
#[derive(Clone, Copy, Debug)]
pub struct CaptureContext {
    pub captured_at_unix_ms: i64,
}

trait ProjectionPayload: Send {
    fn into_json(self: Box<Self>) -> Result<Value, serde_json::Error>;
}

struct TypedProjectionPayload<T>(T);

impl<T> ProjectionPayload for TypedProjectionPayload<T>
where
    T: Serialize + Send + 'static,
{
    fn into_json(self: Box<Self>) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self.0)
    }
}

trait RevisionKey: Send {
    fn as_any(&self) -> &dyn Any;
    fn equals(&self, other: &dyn RevisionKey) -> bool;
}

struct TypedRevisionKey<T>(T);

impl<T> RevisionKey for TypedRevisionKey<T>
where
    T: Eq + Send + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn RevisionKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.0 == other.0)
    }
}

type Projector<H> = Box<dyn Fn(&H, CaptureContext) -> Box<dyn ProjectionPayload> + Send>;
type RevisionProjector<H> = Box<dyn Fn(&H) -> Box<dyn RevisionKey> + Send>;

struct RegisteredModule<H> {
    descriptor: ModuleDescriptor,
    revision: RevisionProjector<H>,
}

/// One registered semantic snapshot scope.
pub struct ProjectionDescriptor<H> {
    pub scope_id: SnapshotScopeId,
    pub module_id: ModuleId,
    pub schema_version: u32,
    pub title: String,
    pub description: String,
    pub required_permissions: BTreeSet<PermissionId>,
    pub schema: Value,
    projector: Projector<H>,
}

struct ObservedRevision {
    key: Box<dyn RevisionKey>,
    revision: u64,
}

/// Registry-local performance observations. Serialization is deliberately not
/// included because it happens after the capture leaves the UI thread.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionPerformance {
    pub captures: u64,
    pub last_capture_us: u64,
    pub worst_capture_us: u64,
    pub budget_violations: u64,
}

/// The extensible projection port hosted by the application.
pub struct ProjectionRegistry<H> {
    modules: BTreeMap<ModuleId, RegisteredModule<H>>,
    scopes: BTreeMap<SnapshotScopeId, ProjectionDescriptor<H>>,
    observed_revisions: BTreeMap<ModuleId, ObservedRevision>,
    next_capture_revision: u64,
    performance: ProjectionPerformance,
    clock: Arc<dyn HostClock>,
}

impl<H: 'static> ProjectionRegistry<H> {
    /// An empty registry that stamps and times its captures by `clock`.
    pub fn new(clock: Arc<dyn HostClock>) -> Self {
        Self {
            modules: BTreeMap::new(),
            scopes: BTreeMap::new(),
            observed_revisions: BTreeMap::new(),
            next_capture_revision: 1,
            performance: ProjectionPerformance::default(),
            clock,
        }
    }

    /// Dock one owner module and its semantic revision projection.
    pub fn register_module<K>(
        &mut self,
        descriptor: ModuleDescriptor,
        revision: fn(&H) -> K,
    ) -> Result<(), ProjectionRegistryError>
    where
        K: Eq + Send + 'static,
    {
        if descriptor.title.trim().is_empty() || descriptor.description.trim().is_empty() {
            return Err(ProjectionRegistryError::InvalidDescriptor(
                "module title and description must not be empty".to_owned(),
            ));
        }
        let id = descriptor.id.clone();
        if self.modules.contains_key(&id) {
            return Err(ProjectionRegistryError::DuplicateModule(id));
        }
        self.modules.insert(
            id,
            RegisteredModule {
                descriptor,
                revision: Box::new(move |app| Box::new(TypedRevisionKey(revision(app)))),
            },
        );
        Ok(())
    }

    /// Dock one typed scope. Its JSON Schema is generated from the same DTO
    /// the projector builds, so mapping and contract cannot drift apart.
    #[allow(clippy::too_many_arguments)]
    pub fn register_scope<T>(
        &mut self,
        scope_id: SnapshotScopeId,
        module_id: ModuleId,
        schema_version: u32,
        title: impl Into<String>,
        description: impl Into<String>,
        required_permission_ids: &[&str],
        project: fn(&H, CaptureContext) -> T,
    ) -> Result<(), ProjectionRegistryError>
    where
        T: JsonSchema + Serialize + Send + 'static,
    {
        if schema_version == 0 {
            return Err(ProjectionRegistryError::InvalidDescriptor(
                "scope schema version must be positive".to_owned(),
            ));
        }
        if !self.modules.contains_key(&module_id) {
            return Err(ProjectionRegistryError::UnknownModule(module_id));
        }
        let prefix = format!("{module_id}.");
        if !scope_id.as_str().starts_with(&prefix) {
            return Err(ProjectionRegistryError::InvalidDescriptor(format!(
                "scope `{scope_id}` is not owned by module `{module_id}`"
            )));
        }
        if self.scopes.contains_key(&scope_id) {
            return Err(ProjectionRegistryError::DuplicateScope(scope_id));
        }
        let title = title.into();
        let description = description.into();
        if title.trim().is_empty() || description.trim().is_empty() {
            return Err(ProjectionRegistryError::InvalidDescriptor(
                "scope title and description must not be empty".to_owned(),
            ));
        }
        let required_permissions = required_permission_ids
            .iter()
            .map(|id| {
                PermissionId::new(*id).map_err(|error| {
                    ProjectionRegistryError::InvalidDescriptor(format!(
                        "scope `{scope_id}` has invalid permission `{id}`: {error}"
                    ))
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if required_permissions.is_empty() {
            return Err(ProjectionRegistryError::InvalidDescriptor(format!(
                "scope `{scope_id}` must declare at least one permission"
            )));
        }
        let schema = generated_schema::<T>();
        validate_schema(&schema).map_err(|error| {
            ProjectionRegistryError::InvalidDescriptor(format!(
                "scope `{scope_id}` generated an invalid schema: {error}"
            ))
        })?;
        self.scopes.insert(
            scope_id.clone(),
            ProjectionDescriptor {
                scope_id,
                module_id,
                schema_version,
                title,
                description,
                required_permissions,
                schema,
                projector: Box::new(move |app, context| {
                    Box::new(TypedProjectionPayload(project(app, context)))
                }),
            },
        );
        Ok(())
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &ProjectionDescriptor<H>> {
        self.scopes.values()
    }

    pub fn module_descriptors(&self) -> impl Iterator<Item = &ModuleDescriptor> {
        self.modules.values().map(|module| &module.descriptor)
    }

    pub fn performance(&self) -> ProjectionPerformance {
        self.performance
    }

    /// Capture exactly the requested scopes in one bounded, immutable pass.
    /// No serialization occurs here.
    pub fn capture(
        &mut self,
        app: &H,
        instance_id: &InstanceId,
        requested_scopes: &[SnapshotScopeId],
    ) -> Result<SnapshotCapture, ControlError> {
        if requested_scopes.is_empty() || requested_scopes.len() > CONTROL_MAX_SNAPSHOT_SCOPES {
            return Err(ControlError::invalid_request(
                "snapshot capture scope count is outside its reviewed limit",
            ));
        }
        let mut unique_scopes = BTreeSet::new();
        let mut modules = BTreeSet::new();
        for scope_id in requested_scopes {
            if !unique_scopes.insert(scope_id.clone()) {
                return Err(ControlError::invalid_request(format!(
                    "snapshot scope `{scope_id}` was requested more than once"
                )));
            }
            let descriptor = self.scopes.get(scope_id).ok_or_else(|| {
                ControlError::invalid_request(format!(
                    "snapshot scope `{scope_id}` is not registered"
                ))
            })?;
            modules.insert(descriptor.module_id.clone());
        }
        let omitted_scopes = self
            .scopes
            .keys()
            .filter(|scope_id| !unique_scopes.contains(*scope_id))
            .cloned()
            .collect();

        let capture_revision = WireU64::new(self.next_capture_revision);
        self.next_capture_revision = self.next_capture_revision.saturating_add(1);
        let context = CaptureContext {
            captured_at_unix_ms: self.clock.unix_ms(),
        };
        let started = self.clock.monotonic();

        let mut module_revisions = Vec::with_capacity(modules.len());
        for module_id in modules {
            let module = self
                .modules
                .get(&module_id)
                .expect("registered scopes always retain their owner module");
            let key = (module.revision)(app);
            let revision = match self.observed_revisions.get_mut(&module_id) {
                Some(observed) if observed.key.equals(key.as_ref()) => observed.revision,
                Some(observed) => {
                    observed.key = key;
                    observed.revision = observed.revision.saturating_add(1);
                    observed.revision
                }
                None => {
                    self.observed_revisions
                        .insert(module_id.clone(), ObservedRevision { key, revision: 1 });
                    1
                }
            };
            module_revisions.push(ModuleRevision {
                module_id,
                revision: WireU64::new(revision),
            });
        }

        let mut scopes = BTreeMap::new();
        for scope_id in unique_scopes {
            let descriptor = self
                .scopes
                .get(&scope_id)
                .expect("requested scopes were validated above");
            scopes.insert(
                scope_id,
                OwnedScope {
                    module_id: descriptor.module_id.clone(),
                    schema_version: descriptor.schema_version,
                    payload: (descriptor.projector)(app, context),
                },
            );
        }

        let elapsed = self.clock.monotonic().saturating_sub(started);
        let elapsed_us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.performance.captures = self.performance.captures.saturating_add(1);
        self.performance.last_capture_us = elapsed_us;
        self.performance.worst_capture_us = self.performance.worst_capture_us.max(elapsed_us);
        if elapsed_us > CONTROL_UI_BUDGET_US {
            self.performance.budget_violations =
                self.performance.budget_violations.saturating_add(1);
        }

        Ok(SnapshotCapture {
            instance_id: instance_id.clone(),
            capture_revision,
            captured_at_unix_ms: context.captured_at_unix_ms,
            module_revisions,
            capture_elapsed_us: elapsed_us,
            omitted_scopes,
            scopes,
        })
    }
}

struct OwnedScope {
    module_id: ModuleId,
    schema_version: u32,
    payload: Box<dyn ProjectionPayload>,
}

/// Owned result of the UI-thread projection pass. Moving this value to a
/// worker and calling [`Self::into_serialized`] is the serialization boundary.
pub struct SnapshotCapture {
    instance_id: InstanceId,
    capture_revision: WireU64,
    captured_at_unix_ms: i64,
    module_revisions: Vec<ModuleRevision>,
    capture_elapsed_us: u64,
    omitted_scopes: Vec<SnapshotScopeId>,
    scopes: BTreeMap<SnapshotScopeId, OwnedScope>,
}

impl SnapshotCapture {
    pub fn into_serialized(self) -> Result<SerializedSnapshotCapture, serde_json::Error> {
        let mut scopes = BTreeMap::new();
        for (scope_id, scope) in self.scopes {
            scopes.insert(
                scope_id,
                SerializedScope {
                    module_id: scope.module_id,
                    schema_version: scope.schema_version,
                    value: scope.payload.into_json()?,
                },
            );
        }
        Ok(SerializedSnapshotCapture {
            instance_id: self.instance_id,
            capture_revision: self.capture_revision,
            captured_at_unix_ms: self.captured_at_unix_ms,
            module_revisions: self.module_revisions,
            capture_elapsed_us: WireU64::new(self.capture_elapsed_us),
            capture_budget_us: WireU64::new(CONTROL_UI_BUDGET_US),
            capture_within_budget: self.capture_elapsed_us <= CONTROL_UI_BUDGET_US,
            omitted_scopes: self.omitted_scopes,
            scopes,
        })
    }
}

/// Transport-ready form produced after the owned capture leaves the UI
/// thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SerializedSnapshotCapture {
    pub instance_id: InstanceId,
    pub capture_revision: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub captured_at_unix_ms: i64,
    #[schemars(length(max = CONTROL_MAX_SNAPSHOT_SCOPES))]
    pub module_revisions: Vec<ModuleRevision>,
    #[schemars(extend("x-unit" = "microseconds"))]
    pub capture_elapsed_us: WireU64,
    #[schemars(extend("x-unit" = "microseconds"))]
    pub capture_budget_us: WireU64,
    pub capture_within_budget: bool,
    pub omitted_scopes: Vec<SnapshotScopeId>,
    #[schemars(length(max = CONTROL_MAX_SNAPSHOT_SCOPES))]
    pub scopes: BTreeMap<SnapshotScopeId, SerializedScope>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SerializedScope {
    pub module_id: ModuleId,
    #[schemars(range(min = 1))]
    pub schema_version: u32,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionRegistryError {
    DuplicateModule(ModuleId),
    DuplicateScope(SnapshotScopeId),
    UnknownModule(ModuleId),
    InvalidDescriptor(String),
}

impl fmt::Display for ProjectionRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModule(id) => write!(formatter, "duplicate projection module `{id}`"),
            Self::DuplicateScope(id) => write!(formatter, "duplicate snapshot scope `{id}`"),
            Self::UnknownModule(id) => write!(formatter, "unknown projection module `{id}`"),
            Self::InvalidDescriptor(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProjectionRegistryError {}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    /// A clock the test tells: a fixed wall time, and a monotonic reading
    /// that advances by `step_us` every time it is read.
    struct FixedClock {
        step_us: u64,
        reads: AtomicU64,
    }

    impl HostClock for FixedClock {
        fn unix_ms(&self) -> i64 {
            NOW_MS
        }

        fn monotonic(&self) -> Duration {
            let reads = self.reads.fetch_add(1, Ordering::SeqCst);
            Duration::from_micros(reads * self.step_us)
        }
    }

    struct Host {
        price: u32,
    }

    #[derive(Serialize, JsonSchema)]
    struct PriceDto {
        price: u32,
    }

    fn registry(step_us: u64) -> ProjectionRegistry<Host> {
        let mut registry = ProjectionRegistry::new(Arc::new(FixedClock {
            step_us,
            reads: AtomicU64::new(0),
        }));
        registry
            .register_module(
                ModuleDescriptor {
                    id: ModuleId::new("quote").unwrap(),
                    title: "Quote".to_owned(),
                    description: "The one number this host shows.".to_owned(),
                },
                |host: &Host| host.price,
            )
            .unwrap();
        registry
            .register_scope(
                SnapshotScopeId::new("quote.price").unwrap(),
                ModuleId::new("quote").unwrap(),
                1,
                "Price",
                "The price the host holds.",
                &["observe"],
                |host: &Host, _| PriceDto { price: host.price },
            )
            .unwrap();
        registry
    }

    fn scope() -> Vec<SnapshotScopeId> {
        vec![SnapshotScopeId::new("quote.price").unwrap()]
    }

    fn instance() -> InstanceId {
        InstanceId::from_bytes([0x42; 16])
    }

    #[test]
    fn a_capture_is_stamped_and_timed_by_the_injected_clock() {
        let mut registry = registry(250);
        let capture = registry
            .capture(&Host { price: 7 }, &instance(), &scope())
            .unwrap()
            .into_serialized()
            .unwrap();

        assert_eq!(capture.captured_at_unix_ms, NOW_MS);
        assert_eq!(capture.capture_elapsed_us, WireU64::new(250));
        assert!(capture.capture_within_budget);
        assert_eq!(registry.performance().last_capture_us, 250);
        assert_eq!(registry.performance().budget_violations, 0);
    }

    #[test]
    fn a_capture_over_budget_is_counted_by_the_injected_clock() {
        let mut registry = registry(CONTROL_UI_BUDGET_US + 1);
        let capture = registry
            .capture(&Host { price: 7 }, &instance(), &scope())
            .unwrap()
            .into_serialized()
            .unwrap();

        assert!(!capture.capture_within_budget);
        assert_eq!(registry.performance().budget_violations, 1);
    }

    #[test]
    fn a_module_revision_moves_only_when_the_host_state_does() {
        let mut registry = registry(1);
        let revision = |registry: &mut ProjectionRegistry<Host>, price| {
            registry
                .capture(&Host { price }, &instance(), &scope())
                .unwrap()
                .into_serialized()
                .unwrap()
                .module_revisions[0]
                .revision
        };

        assert_eq!(revision(&mut registry, 7), WireU64::new(1));
        assert_eq!(revision(&mut registry, 7), WireU64::new(1));
        assert_eq!(revision(&mut registry, 8), WireU64::new(2));
    }
}
