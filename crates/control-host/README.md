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
- `clock` — `HostClock`, the port time arrives through.

It depends on `quantick-control` only. It never reads a clock, spawns a
thread or links a UI toolkit; `crates/guards/src/headless.rs` checks that.
The desktop app supplies the host state, the clock, the authority table, the
handlers and the gateway's socket loop.
