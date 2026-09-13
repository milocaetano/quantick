# UI harness evidence

`QUANTICK_QUICK_RANGE_DEMO=active|ready` is registered beside the floating
surface, defaults off, and enters the same `QuickRange` state machine used by
secondary-button input. `active` stages the ruler-equivalent drag preview;
`ready` performs the same state transition plus release. Staging waits for 72
bars so its two anchors always describe a visible market-time interval.

The generated hook registry and prose name the hook, its values, its default,
and its owning module. `cargo test -p quantick-guards` passed, including the
generated hook-registry equality check and unknown-hook coverage.

Final captures used a dedicated build, never the user's executable:

- executable: `C:\src\quantick-agent-target-quick-range\debug\quantick-app.exe`
- SHA-256: `E1F669F492AB2AFB2340C3928587D7ECA326A165A44F896A05589F926E669686`
- built: 2026-09-13 04:08:47 America/Sao_Paulo, after the last runtime edit
- fixture: authored offline BTCUSDT tick-50 replay on alternate MT5 port 19235
- capture: `tools/capture_window.ps1`, narrowed to the exact launched PID
- storage: every run used isolated `QUANTICK_*` state paths

The app emitted healthy frame summaries before every accepted image, and each
opened process was closed by its exact PID. The final matrix was recaptured
after the full workspace checks passed. No input injection was used.

Live control-plane reading is **BLOCKED** in this host run: the final app
published instance `1AGB0-O4wev48ozSvURAgA`, and the pinned adapter completed
MCP initialization, but `quantick_describe` did not return within 15 seconds.
The adapter was stopped and this is not represented as a live scene PASS.
Scene honesty is instead proven by the in-process frame test that observes the
visible `quick_range.fixed_range_profile` control, its rectangle, enabled state,
and `annotate.fixed_range_profile.create` capability ID. Capability discovery,
dispatch, permission refusal, and drawing readback have independent tests.
