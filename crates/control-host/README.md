# quantick-control-host

The headless half of the Quantick control-plane host. `quantick-control` is
the transport-neutral contract; this crate is the host machinery built on it
that needs no UI:

- `projection` — the snapshot projection registry, generic over the host
  state its projectors read, with scope validation, revision tracking and the
  capture budget;
- `admission` — capability registration with compiled schemas, and the fixed
  order of admission checks every request passes before a handler runs, each
  step returning the only value the next accepts;
- `catalogue` — the snapshot scopes a projection registry declares, as the
  contract publishes them;
- `idempotency` — the per-connection store that replays a keyed call's
  recorded outcome instead of executing it twice;
- `journal` — the bounded semantic event journal and its change signal;
- `evidence` — retained canonical bundles and typed chunk pages, with count,
  byte and expiry bounds, current-grant checks and withdrawal epochs;
- `clock` — `HostClock`, the port time arrives through.

It depends on `quantick-control` only. It never reads a clock, spawns a
thread or links a UI toolkit; `crates/guards/src/headless.rs` checks that.
The desktop app supplies the host state, the clock, the authority table, the
handlers and the gateway's socket loop.

Evidence producers pass canonical bytes, their aggregated permissions and a
caller-supplied expiry to `evidence::RetainedBundle::new`. Its private fields
keep digest, byte count and chunks consistent. `EvidenceStore` accepts the
capture's collection epoch and current time, and rechecks the reader's grant
and resource cursor for every page. Capture admission and collecting pixels,
configuration or application state remain the caller's responsibility.

The desktop capture/manifest, response-worker paging and access-withdrawal
adapters use this same store. `tests/evidence_resource.rs` exercises its
public API with an independent canonical byte producer and fixed digest
expectations; that fixture is not another production host. None of this
adds work to a frame that did not request evidence.
