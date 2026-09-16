# Additive consumer probe

`crates/chart-interaction/src/tests.rs` contains
`a_headless_executor_consumes_the_same_effect_and_completion_event`.
The 26-line test body/function defines a Recorder consumer, receives the real
Place effect, retains its typed request, returns the correlated Completed
event, and verifies the production model consumes the selection.

Observed change cost for this second consumer: one test file, 26 lines plus
its test attribute; zero production files, zero registry edits, zero egui,
network or clock dependencies. The production owner and app are not modified
to install the consumer. This is an additive executor probe, not a claim that
adding a new drawing action would cost the same. A new action still requires
its domain enum case, tool/contract registration, permission and UI metadata
and corresponding tests; no universal registration shortcut is promised.

The current test command is `cargo test -p quantick-chart-interaction`.
The initial 13-test suite passed without building app; the v4 suite contains
14 passing tests, including the fractional-slot regression. Before extraction,
the lifecycle tests imported the toolkit-bearing drawing_chrome module and
required quantick-app. This measures test/consumer coupling, not UI quality,
transport admission, runtime performance, or an architecture score.
