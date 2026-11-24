use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use quantick_control::id::{InstanceId, ModuleId, SnapshotScopeId};
use quantick_control::limits::{CONTROL_MAX_SNAPSHOT_SCOPES, CONTROL_UI_BUDGET_US};
use quantick_control::registry::ModuleDescriptor;
use quantick_control::wire::WireU64;
use quantick_control_host::clock::HostClock;
use quantick_control_host::projection::{
    CaptureContext, ProjectionRegistry, SerializedSnapshotCapture,
};
use schemars::JsonSchema;
use serde::{Serialize, Serializer};
use serde_json::json;

#[derive(Default)]
struct Clock {
    time: AtomicU64,
    step: AtomicU64,
    wall_reads: AtomicU64,
}

impl HostClock for Clock {
    fn unix_ms(&self) -> i64 {
        self.wall_reads.fetch_add(1, Ordering::Relaxed);
        1234
    }
    fn monotonic(&self) -> Duration {
        Duration::from_micros(
            self.time
                .fetch_add(self.step.load(Ordering::Relaxed), Ordering::Relaxed),
        )
    }
}

#[derive(Default)]
struct Host {
    quote: u32,
    depth: u32,
    quote_reads: Cell<u32>,
    depth_reads: Cell<u32>,
}

#[derive(Serialize, JsonSchema)]
struct Reading {
    value: u32,
    captured_at: i64,
}

fn quote(host: &Host, context: CaptureContext) -> Reading {
    host.quote_reads.set(host.quote_reads.get() + 1);
    Reading {
        value: host.quote,
        captured_at: context.captured_at_unix_ms,
    }
}

fn depth(host: &Host, context: CaptureContext) -> Reading {
    host.depth_reads.set(host.depth_reads.get() + 1);
    Reading {
        value: host.depth,
        captured_at: context.captured_at_unix_ms,
    }
}

fn module(id: &str) -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(id).unwrap(),
        title: id.to_owned(),
        description: "Captured market state".to_owned(),
    }
}

fn scopes(names: &[&str]) -> Vec<SnapshotScopeId> {
    names
        .iter()
        .map(|name| SnapshotScopeId::new(*name).unwrap())
        .collect()
}

fn registry(clock: Arc<Clock>) -> ProjectionRegistry<Host> {
    let mut registry = ProjectionRegistry::new(clock);
    registry
        .register_module(module("quote"), |host: &Host| host.quote)
        .unwrap();
    registry
        .register_module(module("depth"), |host: &Host| host.depth)
        .unwrap();
    for (name, owner, project) in [
        (
            "quote.last",
            "quote",
            quote as fn(&Host, CaptureContext) -> Reading,
        ),
        ("quote.summary", "quote", quote),
        ("depth.book", "depth", depth),
    ] {
        registry
            .register_scope(
                SnapshotScopeId::new(name).unwrap(),
                ModuleId::new(owner).unwrap(),
                2,
                "Reading",
                "A coherent reading of the owner",
                &["observe"],
                project,
            )
            .unwrap();
    }
    registry
}

fn capture(
    registry: &mut ProjectionRegistry<Host>,
    host: &Host,
    names: &[&str],
) -> SerializedSnapshotCapture {
    registry
        .capture(host, &InstanceId::from_bytes([1; 16]), &scopes(names))
        .unwrap()
        .into_serialized()
        .unwrap()
}

#[test]
fn an_invalid_scope_after_a_valid_one_runs_no_projectors_or_clock_reads() {
    let clock = Arc::new(Clock::default());
    let mut registry = registry(Arc::clone(&clock));
    let host = Host::default();
    let error = registry
        .capture(
            &host,
            &InstanceId::from_bytes([1; 16]),
            &scopes(&["quote.last", "quote.missing"]),
        )
        .err()
        .unwrap();
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::INVALID_REQUEST
    );
    assert_eq!(host.quote_reads.get(), 0);
    assert_eq!(clock.wall_reads.load(Ordering::Relaxed), 0);
    assert_eq!(registry.performance().captures, 0);
    let first = capture(&mut registry, &host, &["quote.last"]);
    assert_eq!(first.capture_revision.get(), 1);
    assert_eq!(first.module_revisions[0].revision.get(), 1);
}

#[test]
fn duplicate_scopes_are_rejected_before_a_partial_capture_can_escape() {
    let clock = Arc::new(Clock::default());
    let mut registry = registry(Arc::clone(&clock));
    let host = Host::default();
    let error = registry
        .capture(
            &host,
            &InstanceId::from_bytes([1; 16]),
            &scopes(&["quote.last", "depth.book", "quote.last"]),
        )
        .err()
        .unwrap();
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::INVALID_REQUEST
    );
    assert_eq!(host.quote_reads.get(), 0);
    assert_eq!(host.depth_reads.get(), 0);
    assert_eq!(clock.wall_reads.load(Ordering::Relaxed), 0);
    assert_eq!(registry.performance().captures, 0);
}

#[test]
fn empty_and_over_limit_requests_do_not_consume_capture_revisions() {
    let mut registry = registry(Arc::new(Clock::default()));
    let host = Host::default();
    let instance = InstanceId::from_bytes([1; 16]);
    assert!(registry.capture(&host, &instance, &[]).is_err());
    let too_many =
        vec![SnapshotScopeId::new("quote.last").unwrap(); CONTROL_MAX_SNAPSHOT_SCOPES + 1];
    assert!(registry.capture(&host, &instance, &too_many).is_err());
    assert_eq!(registry.performance().captures, 0);
    assert_eq!(
        capture(&mut registry, &host, &["quote.last"])
            .capture_revision
            .get(),
        1
    );
}

#[test]
fn two_scopes_of_one_module_share_one_revision_and_one_capture_time() {
    let clock = Arc::new(Clock::default());
    let mut registry = registry(Arc::clone(&clock));
    let host = Host {
        quote: 42,
        ..Host::default()
    };
    let result = capture(&mut registry, &host, &["quote.summary", "quote.last"]);
    assert_eq!(result.module_revisions.len(), 1);
    assert_eq!(result.module_revisions[0].module_id.as_str(), "quote");
    assert_eq!(result.module_revisions[0].revision.get(), 1);
    assert_eq!(clock.wall_reads.load(Ordering::Relaxed), 1);
    for value in result.scopes.values() {
        assert_eq!(value.schema_version, 2);
        assert_eq!(value.value, json!({"value": 42, "captured_at": 1234}));
    }
    assert_eq!(host.quote_reads.get(), 2);
    assert_eq!(host.depth_reads.get(), 0);
    assert_eq!(result.omitted_scopes, scopes(&["depth.book"]));
}

#[test]
fn scope_order_does_not_change_output_order_or_omitted_scope_reporting() {
    let mut registry = registry(Arc::new(Clock::default()));
    let host = Host::default();
    let result = capture(&mut registry, &host, &["quote.summary", "depth.book"]);
    assert_eq!(
        result
            .scopes
            .keys()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["depth.book", "quote.summary"]
    );
    assert_eq!(
        result
            .module_revisions
            .iter()
            .map(|row| row.module_id.as_str())
            .collect::<Vec<_>>(),
        ["depth", "quote"]
    );
    assert_eq!(result.omitted_scopes, scopes(&["quote.last"]));
    let single = capture(&mut registry, &host, &["quote.summary"]);
    assert_eq!(single.omitted_scopes, scopes(&["depth.book", "quote.last"]));
}

#[test]
fn unrequested_modules_are_not_observed_until_they_are_captured() {
    let mut registry = registry(Arc::new(Clock::default()));
    let mut host = Host::default();
    capture(&mut registry, &host, &["quote.last"]);
    host.depth = 5;
    capture(&mut registry, &host, &["quote.last"]);
    host.depth = 10;
    let first_depth = capture(&mut registry, &host, &["depth.book"]);
    assert_eq!(first_depth.module_revisions[0].revision.get(), 1);
    assert_eq!(host.depth_reads.get(), 1);
    host.depth = 11;
    let both = capture(&mut registry, &host, &["quote.last", "depth.book"]);
    assert_eq!(both.module_revisions[0].module_id.as_str(), "depth");
    assert_eq!(both.module_revisions[0].revision.get(), 2);
    assert_eq!(both.module_revisions[1].module_id.as_str(), "quote");
    assert_eq!(both.module_revisions[1].revision.get(), 1);
}

#[test]
fn a_state_returning_to_an_older_value_still_advances_the_revision() {
    let mut registry = registry(Arc::new(Clock::default()));
    let mut host = Host {
        quote: 10,
        ..Host::default()
    };
    let original = capture(&mut registry, &host, &["quote.last"]);
    host.quote = 11;
    let changed = capture(&mut registry, &host, &["quote.last"]);
    host.quote = 10;
    let restored = capture(&mut registry, &host, &["quote.last"]);
    assert_eq!(original.module_revisions[0].revision.get(), 1);
    assert_eq!(changed.module_revisions[0].revision.get(), 2);
    assert_eq!(restored.module_revisions[0].revision.get(), 3);
    assert_eq!(restored.capture_revision.get(), 3);
    assert_eq!(
        restored.scopes[&SnapshotScopeId::new("quote.last").unwrap()].value,
        original.scopes[&SnapshotScopeId::new("quote.last").unwrap()].value
    );
}

#[test]
fn captures_own_values_before_the_host_changes_or_is_dropped() {
    let mut registry = registry(Arc::new(Clock::default()));
    let (owned, later) = {
        let mut host = Host {
            quote: 10,
            ..Host::default()
        };
        let mut owned = registry
            .capture(
                &host,
                &InstanceId::from_bytes([9; 16]),
                &scopes(&["quote.last"]),
            )
            .unwrap();
        owned.window_id = Some(WireU64::new(7));
        host.quote = 20;
        let later = capture(&mut registry, &host, &["quote.last"]);
        (owned, later)
    };
    let earlier = owned.into_serialized().unwrap();
    let key = SnapshotScopeId::new("quote.last").unwrap();
    assert_eq!(earlier.scopes[&key].value["value"], 10);
    assert_eq!(later.scopes[&key].value["value"], 20);
    assert_eq!(earlier.window_id, Some(WireU64::new(7)));
    assert_eq!(earlier.instance_id, InstanceId::from_bytes([9; 16]));
    assert_eq!(earlier.capture_revision.get(), 1);
}

#[test]
fn the_exact_capture_budget_is_allowed_and_a_later_fast_capture_keeps_the_worst() {
    let clock = Arc::new(Clock::default());
    clock.step.store(CONTROL_UI_BUDGET_US, Ordering::Relaxed);
    let mut registry = registry(Arc::clone(&clock));
    let host = Host::default();
    let exact = capture(&mut registry, &host, &["quote.last"]);
    assert!(exact.capture_within_budget);
    assert_eq!(exact.capture_elapsed_us.get(), CONTROL_UI_BUDGET_US);
    assert_eq!(registry.performance().budget_violations, 0);
    clock
        .step
        .store(CONTROL_UI_BUDGET_US + 1, Ordering::Relaxed);
    assert!(!capture(&mut registry, &host, &["quote.last"]).capture_within_budget);
    clock.step.store(1, Ordering::Relaxed);
    assert!(capture(&mut registry, &host, &["quote.last"]).capture_within_budget);
    let stats = registry.performance();
    assert_eq!(stats.captures, 3);
    assert_eq!(stats.last_capture_us, 1);
    assert_eq!(stats.worst_capture_us, CONTROL_UI_BUDGET_US + 1);
    assert_eq!(stats.budget_violations, 1);
}

#[derive(JsonSchema)]
struct Unserializable {
    value: u32,
}

impl Serialize for Unserializable {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(format!(
            "cannot encode {}",
            self.value
        )))
    }
}

#[test]
fn serialization_failure_occurs_after_capture_and_does_not_poison_the_registry() {
    let mut registry = registry(Arc::new(Clock::default()));
    registry
        .register_scope(
            SnapshotScopeId::new("quote.failure").unwrap(),
            ModuleId::new("quote").unwrap(),
            1,
            "Failure",
            "Exercise the serialization boundary",
            &["observe"],
            |host: &Host, _| Unserializable { value: host.quote },
        )
        .unwrap();
    let host = Host {
        quote: 42,
        ..Host::default()
    };
    let owned = registry
        .capture(
            &host,
            &InstanceId::from_bytes([1; 16]),
            &scopes(&["quote.failure"]),
        )
        .unwrap();
    assert_eq!(registry.performance().captures, 1);
    let error = owned.into_serialized().err().unwrap();
    assert!(error.to_string().contains("cannot encode 42"));
    let valid = capture(&mut registry, &host, &["quote.last"]);
    assert_eq!(valid.capture_revision.get(), 2);
    assert_eq!(valid.module_revisions[0].revision.get(), 1);
    assert_eq!(
        valid.scopes[&SnapshotScopeId::new("quote.last").unwrap()].value["value"],
        42
    );
}

#[test]
fn exactly_the_maximum_number_of_distinct_scopes_is_captured() {
    let mut registry = registry(Arc::new(Clock::default()));
    let mut requested = Vec::new();
    for index in 0..CONTROL_MAX_SNAPSHOT_SCOPES {
        let scope = SnapshotScopeId::new(format!("quote.reading{index}")).unwrap();
        registry
            .register_scope(
                scope.clone(),
                ModuleId::new("quote").unwrap(),
                1,
                "Reading",
                "One requested reading",
                &["observe"],
                quote,
            )
            .unwrap();
        requested.push(scope);
    }
    let host = Host {
        quote: 17,
        ..Host::default()
    };
    let captured = registry
        .capture(&host, &InstanceId::from_bytes([1; 16]), &requested)
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(captured.scopes.len(), CONTROL_MAX_SNAPSHOT_SCOPES);
    assert_eq!(
        usize::try_from(host.quote_reads.get()).unwrap(),
        CONTROL_MAX_SNAPSHOT_SCOPES
    );
    assert_eq!(captured.module_revisions.len(), 1);
    assert_eq!(captured.module_revisions[0].revision.get(), 1);
    for scope in &requested {
        assert_eq!(captured.scopes[scope].value["value"], 17);
    }
    assert_eq!(
        captured.omitted_scopes,
        scopes(&["depth.book", "quote.last", "quote.summary"])
    );
}

#[test]
fn failed_scope_registration_is_atomic_and_allows_a_corrected_retry() {
    let mut registry = registry(Arc::new(Clock::default()));
    let candidate = SnapshotScopeId::new("quote.corrected").unwrap();
    for (title, description, permissions) in [
        (" ", "Valid description", &["observe"][..]),
        ("Reading", "\t", &["observe"][..]),
        ("Reading", "Valid description", &["INVALID PERMISSION"][..]),
    ] {
        assert!(
            registry
                .register_scope(
                    candidate.clone(),
                    ModuleId::new("quote").unwrap(),
                    1,
                    title,
                    description,
                    permissions,
                    quote,
                )
                .is_err()
        );
        assert_eq!(registry.descriptors().count(), 3);
        assert!(
            registry
                .descriptors()
                .all(|descriptor| descriptor.scope_id != candidate)
        );
    }
    registry
        .register_scope(
            candidate.clone(),
            ModuleId::new("quote").unwrap(),
            1,
            "Reading",
            "Corrected metadata",
            &["observe"],
            quote,
        )
        .unwrap();
    let result = capture(
        &mut registry,
        &Host {
            quote: 73,
            ..Host::default()
        },
        &["quote.corrected"],
    );
    assert_eq!(result.scopes[&candidate].value["value"], 73);
    assert_eq!(registry.descriptors().count(), 4);
}

#[test]
fn duplicate_module_registration_does_not_replace_its_revision_projection() {
    let mut registry = registry(Arc::new(Clock::default()));
    let mut host = Host {
        quote: 10,
        depth: 1,
        ..Host::default()
    };
    capture(&mut registry, &host, &["quote.last"]);
    assert!(
        registry
            .register_module(module("quote"), |host: &Host| host.depth)
            .is_err()
    );
    host.depth = 2;
    let unchanged = capture(&mut registry, &host, &["quote.last"]);
    assert_eq!(unchanged.module_revisions[0].revision.get(), 1);
    host.quote = 11;
    let changed = capture(&mut registry, &host, &["quote.last"]);
    assert_eq!(changed.module_revisions[0].revision.get(), 2);
    assert_eq!(registry.module_descriptors().count(), 2);
}

#[test]
fn duplicate_scope_registration_keeps_the_original_projector_and_schema_version() {
    let mut registry = registry(Arc::new(Clock::default()));
    assert!(
        registry
            .register_scope(
                SnapshotScopeId::new("quote.last").unwrap(),
                ModuleId::new("quote").unwrap(),
                99,
                "Replacement",
                "Must not replace the original",
                &["observe"],
                depth,
            )
            .is_err()
    );
    let host = Host {
        quote: 10,
        depth: 90,
        ..Host::default()
    };
    let captured = capture(&mut registry, &host, &["quote.last"]);
    let scope = &captured.scopes[&SnapshotScopeId::new("quote.last").unwrap()];
    assert_eq!(scope.value["value"], 10);
    assert_eq!(scope.schema_version, 2);
    assert_eq!(host.quote_reads.get(), 1);
    assert_eq!(host.depth_reads.get(), 0);
}

struct CaptureClock {
    readings: [Duration; 2],
    next: AtomicU64,
}

impl HostClock for CaptureClock {
    fn unix_ms(&self) -> i64 {
        -100
    }

    fn monotonic(&self) -> Duration {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        self.readings[usize::try_from(index).unwrap()]
    }
}

#[test]
fn unusual_clock_readings_are_bounded_without_overflow_or_negative_elapsed_time() {
    for (start, end, expected_us, within_budget) in [
        (Duration::from_secs(5), Duration::from_secs(4), 0, true),
        (Duration::ZERO, Duration::from_nanos(999), 0, true),
        (Duration::ZERO, Duration::MAX, u64::MAX, false),
    ] {
        let clock = Arc::new(CaptureClock {
            readings: [start, end],
            next: AtomicU64::new(0),
        });
        let mut registry = ProjectionRegistry::new(clock.clone());
        registry
            .register_module(module("quote"), |host: &Host| host.quote)
            .unwrap();
        registry
            .register_scope(
                SnapshotScopeId::new("quote.last").unwrap(),
                ModuleId::new("quote").unwrap(),
                1,
                "Reading",
                "Capture against an injected clock",
                &["observe"],
                quote,
            )
            .unwrap();
        let result = capture(&mut registry, &Host::default(), &["quote.last"]);
        assert_eq!(result.capture_elapsed_us.get(), expected_us);
        assert_eq!(result.capture_within_budget, within_budget);
        assert_eq!(result.captured_at_unix_ms, -100);
        let stats = registry.performance();
        assert_eq!(stats.last_capture_us, expected_us);
        assert_eq!(stats.worst_capture_us, expected_us);
        assert_eq!(stats.budget_violations, u64::from(!within_budget));
        assert_eq!(stats.captures, 1);
        assert_eq!(clock.next.load(Ordering::Relaxed), 2);
    }
}
