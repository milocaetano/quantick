# Mission — MetaTrader tape latency

Cut MetaTrader tape latency to under one second on a live B3 tape, and make
whatever delay remains attributable to a named hop instead of hiding inside a
single `arrival` number.

## Why this mission exists

The trader reports the chart running seconds behind MetaTrader. The status bar
shows `arrival 18112 ms` while the same bar reads `60 fps · 16.7 ms · cpu
2.4 ms` — the renderer is idle, so the eighteen seconds are spent somewhere
between the terminal's tick database and the UI drain, and today nothing says
where. `arrival` measures the whole chain at once:

    MT5 terminal -> EA pump (OnTick + 200 ms timer, CopyTicks capped at 4096)
      -> 16 KB out-buffer -> SocketSend (5 s timeout, terminal main thread)
      -> NDJSON decode -> mpsc(4096) -> app pump -> mpsc(4096) -> frame drain

Any fix chosen without splitting that chain is a guess.

## Acceptance criteria

1. **The chain is split and measured.** The bridge stamps what it sends with
   its own millisecond server clock, so `arrival` decomposes into at least
   *cursor lag* (newest tick the terminal holds -> newest tick the bridge has
   sent), *terminal lag* (tick instant -> bridge send) and *transport lag*
   (bridge send -> chart). Each is measured on every print and published as a
   bounded-rate sample, so the split costs the per-trade path nothing.
2. **The split is readable without a mouse.** The new figures reach the
   `control` health view and therefore `quantick_get_diagnostics`, alongside
   the existing `arrival_latency_ms`, and the status bar names the responsible
   hop rather than only the total.
3. **Every EA-side delay floor is removed or justified in a comment.**
   `CopyTicks` drains to exhaustion within a pass instead of stopping at one
   capped batch; the timer cadence and the send timeout are each either
   lowered or defended in writing against the seconds-scale symptom.
4. **Backpressure is named, never absorbed.** A full channel, a blocked
   socket send or a drain that falls behind emits a structured `event_code`
   and shows on the health view; silence while seconds accumulate is a defect.
5. **The per-trade path does not get slower.** Every touched path classified
   by rate (per-trade / per-depth / per-frame / rare) in the plan, no added
   per-trade wall-clock read or allocation, and measured evidence — a dense
   synthetic tape driven through the real decoder and app pump, branch vs. a
   `main` control run, numbers in the PR body.
6. **Ships clean.** `cargo fmt --all -- --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo build --workspace` and `cargo test
   --workspace` all exit 0 after rebasing on latest `main`; `ruff check
   --select F` over `bridge/mt5/` and `tools/mt5/` clean if either is touched;
   `arch-review` run with every Blocker and Should-fix resolved or deferred in
   the PR body; `visual-qa` PASS and `trader-ux-review` with no unresolved
   Blocker for the status-bar change, each new surface reachable by an
   `ui-harness` env hook added in the same change; PR opened.

Everything written into the repository for this mission is in English.

## Out of scope

Rewriting the transport (shared memory, a native DLL, a second socket), and
anything that improves latency by dropping prints. Merging the PR — that stays
the trader's call.
