//! `docs/control-plane/retry-matrix.md`, rendered from the registry and the
//! declared rows.
//!
//! Row order is the registry's, not the table's: the renderer walks the
//! registered capabilities and asks [`super::rows::readback`] for each one's
//! row, so which family declared a row cannot move a line of the document.

use super::drift::read_needs;
use super::reach::{Holder, holder};
use super::rows::readback;
use crate::readback::Readback;
use crate::retry_matrix::GENERATED_MARKER;

use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control_host::contract::CapabilityContract;

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// What the gateway does with a key for this capability.
pub fn enforced(policy: IdempotencyPolicy, grantable: bool) -> &'static str {
    match (grantable, policy) {
        (false, _) => "refused before dispatch: no grant reaches it",
        (true, IdempotencyPolicy::Optional) => "deduplicated per connection",
        (true, IdempotencyPolicy::Required) => "key required; deduplicated per connection",
        (true, IdempotencyPolicy::Forbidden) => "key refused; not retryable, read back",
    }
}

pub fn policy_name(policy: IdempotencyPolicy) -> &'static str {
    match policy {
        IdempotencyPolicy::Forbidden => "forbidden",
        IdempotencyPolicy::Optional => "optional",
        IdempotencyPolicy::Required => "required",
    }
}

pub struct RenderedRow<'a> {
    /// The newest registered version, whose policy and reach the row shows.
    descriptor: &'a CapabilityDescriptor,
    /// Every registered version, oldest first.
    versions: Vec<u32>,
    row: &'static Readback,
    holder: Holder,
}

/// Render the committed contents of `docs/control-plane/retry-matrix.md`
/// for the registry `contract` serves; [`super::drift::drift`] says first
/// whether the rows and that registry agree.
#[must_use]
pub fn render<P>(contract: &CapabilityContract<P>) -> String {
    // The registry iterates by `(id, version)`, so the last descriptor seen
    // for an id is its newest version.
    let mut newest: BTreeMap<&str, (&CapabilityDescriptor, Vec<u32>)> = BTreeMap::new();
    for descriptor in contract
        .registry()
        .capabilities()
        .filter(|descriptor| !descriptor.read_only)
    {
        let entry = newest
            .entry(descriptor.id.as_str())
            .or_insert((descriptor, Vec::new()));
        entry.0 = descriptor;
        entry.1.push(descriptor.version);
    }
    let registered = newest.len();
    let rows: Vec<RenderedRow<'_>> = newest
        .into_values()
        .filter_map(|(descriptor, versions)| {
            Some(RenderedRow {
                descriptor,
                versions,
                row: readback(descriptor.id.as_str())?,
                holder: holder(contract, descriptor)?,
            })
        })
        .collect();
    let mut out = String::new();
    out.push_str("# Control plane retry matrix\n\n");
    out.push_str(GENERATED_MARKER);
    out.push_str("\n\n");
    out.push_str(PREAMBLE);
    out.push_str(&summary(&rows));
    out.push_str(&table(contract, &rows));
    let _ = write!(
        out,
        "\n{registered} mutable capabilities registered, {} with a readback.\n",
        rows.len()
    );
    out
}

pub const PREAMBLE: &str = concat!(
    "What a client does when a call that changes something did not answer:\n",
    "read the state back unless cancellation proved it never started. One row\n",
    "per mutable capability the running application registers; read-only\n",
    "capabilities change nothing and need no reconciling.\n",
    "\n",
    "This file is generated; a hand edit is a guard failure, not a correction.\n",
    "Each row lives beside the capability it reconciles, in that family's own\n",
    "module under `crates/control-schema/src/` — the drawings in `analysis.rs`,\n",
    "the ticket in `trade.rs` — and `retry_matrix::rows::FAMILIES` is the list\n",
    "that joins them. The generator refuses to render while any row disagrees\n",
    "with the registry. To change a row, change it in its family module, or\n",
    "change the capability, and regenerate:\n",
    "\n",
    "```sh\n",
    "cargo run -p quantick-app -- --dump-retry-matrix \\\n",
    "  > docs/control-plane/retry-matrix.md\n",
    "```\n",
    "\n",
    "`cargo test -p quantick-app retry_matrix` compares this file with the\n",
    "generator and every row with the registry; `cargo test -p quantick-guards`\n",
    "compares it with the [capability inventory](capability-inventory.md) in a\n",
    "second.\n",
    "\n",
    "## Reading a row\n",
    "\n",
    "**Versions** lists every registered version of the capability; the row\n",
    "holds for all of them, and **Policy** and **Reach** are the newest's. Call\n",
    "the newest: a version is added when the previous one's contract had to\n",
    "change, and the older one is kept only so an existing client does not\n",
    "break.\n",
    "\n",
    "**Policy** is the `idempotency` the descriptor publishes. **Enforced** is\n",
    "what the gateway does with it:\n",
    "\n",
    "- *deduplicated per connection* — a call carrying an idempotency key is\n",
    "  recorded against the connection that made it. The same key with the same\n",
    "  input answers the first call's recorded outcome without acting again; the\n",
    "  same key with different input is `control.idempotency_conflict`; a retry\n",
    "  that arrives while the first call is still running is\n",
    "  `control.request_in_progress` (non-retryable; reconcile). A call the\n",
    "  application began and had still not answered a full request window after\n",
    "  its deadline records a non-retryable `control.timeout` whose next step is\n",
    "  to read the state back: its outcome is unknown, and retrying the key cannot\n",
    "  learn it.\n",
    "- *key refused; not retryable, read back* — a call carrying a key is\n",
    "  refused before dispatch (`control.invalid_request`). Sending the call\n",
    "  twice acts twice, so a client whose call did not answer reads the state\n",
    "  back instead.\n",
    "- *refused before dispatch: no grant reaches it* — the capability's\n",
    "  ceiling is one no grant hands out, so no connection can call it, keyed or\n",
    "  not. The row records what its readback will be when that changes.\n",
    "\n",
    "**Reach** is the lowest profile whose ceiling admits the capability.\n",
    "Every readback is readable by a connection granted the capability's own\n",
    "permissions and the default read grant (the generator refuses a row that\n",
    "is not), so reconciling never needs a scope the trader did not already\n",
    "tick for the call. A capability no grant reaches is held to its ceiling\n",
    "instead, and its row says which further scope its readback needs.\n",
    "**Readback** is the read that shows the effect — a `snapshot.read` scope,\n",
    "or `events.read` filtered to one journal event kind — and **Field** the\n",
    "path within it, `[]` stepping into an array. **Applied when** says what the\n",
    "field shows if the call took effect. **Proven by** names the transport\n",
    "tests, in `crates/app/src/app/tests/retry_readback_tests.rs`, that exercise\n",
    "the row through the real local gateway.\n",
    "\n",
    "To reconcile, take the reading *before* the call — the scope's value, or\n",
    "an `events.read` cursor at `latest` — and compare it with the same read\n",
    "afterwards. A snapshot read can be retried freely: reads are read-only.\n",
    "\n",
    "## The guarantee ends with the connection\n",
    "\n",
    "Deduplication is scoped to the connection that made the call. A client\n",
    "that reconnects arrives as a new principal, and its keys are new keys: the\n",
    "same key re-executes rather than replaying. That is deliberate — the only\n",
    "identities that survive a reconnect are the bearer token, which every\n",
    "client of one grant shares, and a client-supplied name nothing\n",
    "authenticates, and scoping records by either would let one client replay\n",
    "another's answer. Widening it needs a durable client identity the\n",
    "handshake proves, which is a contract change tracked in\n",
    "[#362](https://github.com/milocaetano/quantick/issues/362). Until then a\n",
    "client whose connection dropped with a call in flight reconciles *every*\n",
    "row — `optional` ones included — through its readback, never by retrying\n",
    "the key on the new connection.\n",
    "\n",
    "## Missing replies and retry advice\n",
    "\n",
    "A mutation that may have started returns `retryable: false` and\n",
    "`details.outcome: unknown` on deadline. A gateway cancellation that wins\n",
    "before dispatch returns `details.outcome: not_started` and can be retried;\n",
    "the cancelled request cannot later act. A key deduplicates while its\n",
    "reservation or terminal record is retained, but another principal's\n",
    "traffic can evict that record before its TTL; a missing result therefore\n",
    "requires readback even with a key. Transport loss is non-retryable for every\n",
    "mutation, including keyed calls. Read the row below to reconcile.\n",
    "An oversized terminal result retains a compact non-retryable uncertainty\n",
    "refusal under its key instead of allowing another execution.\n",
    "\n",
    "`quantick_invoke` accepts an optional `idempotency_key` (1 to 128 printable\n",
    "ASCII bytes) and passes it unchanged in the request envelope. The runtime\n",
    "enforces the row's key policy. The MCP adapter learns read-only retry\n",
    "classification from the runtime registry; unknown metadata is conservative.\n",
    "\n",
    "Real post-start deadline and post-execution socket-loss proofs live in\n",
    "`crates/app/src/app/tests/mutation_uncertainty_tests.rs`; adapter-to-wire\n",
    "key and lost-reply proofs live in `crates/mcp/tests/fake_gateway.rs`.\n",
    "\n",
);

pub fn summary(rows: &[RenderedRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str("## Coverage\n\n");
    let mut classes = BTreeMap::<&str, usize>::new();
    for row in rows {
        *classes
            .entry(enforced(row.descriptor.idempotency, row.holder.grantable))
            .or_default() += 1;
    }
    let _ = writeln!(out, "| Enforced | Capabilities |");
    let _ = writeln!(out, "| --- | --- |");
    for (class, count) in &classes {
        let _ = writeln!(out, "| {class} | {count} |");
    }
    let _ = writeln!(out, "| **Total** | **{}** |\n", rows.len());
    out
}

fn table<P>(contract: &CapabilityContract<P>, rows: &[RenderedRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str("## Capabilities\n\n");
    let _ = writeln!(
        out,
        "| Capability | Versions | Policy | Enforced | Reach | Readback | Field | Applied when | \
         Proven by |"
    );
    let _ = writeln!(
        out,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for rendered in rows {
        let row = rendered.row;
        let reach = if rendered.holder.grantable {
            format!("`{}`", rendered.holder.profile)
        } else {
            format!("none (`{}` ceiling)", rendered.holder.profile)
        };
        let mut source = match (row.scope, row.event) {
            (Some(scope), _) => format!("`{}` `{scope}`", row.read),
            (None, Some(event)) => format!("`{}` `{event}`", row.read),
            (None, None) => format!("`{}`", row.read),
        };
        let mut granted = quantick_control_host::authority::default_grant();
        granted.extend(rendered.descriptor.required_permissions.iter().cloned());
        let beyond: Vec<String> = read_needs(contract, row)
            .difference(&granted)
            .map(|permission| format!("`{}`", permission.as_str()))
            .collect();
        if !beyond.is_empty() {
            let _ = write!(source, " (also needs {})", beyond.join(", "));
        }
        let proof = row
            .proven_by
            .iter()
            .map(|test| format!("`{test}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let versions = rendered
            .versions
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | `{}` | {} | {} |",
            rendered.descriptor.id.as_str(),
            versions,
            policy_name(rendered.descriptor.idempotency),
            enforced(rendered.descriptor.idempotency, rendered.holder.grantable),
            reach,
            source,
            row.field,
            row.applied_when,
            proof,
        );
    }
    out
}
