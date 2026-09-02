# Dimension 7 — the second operator

Read before writing an agent-surface finding. `CLAUDE.md`'s *Operable without a hand* points here.

quantick is going to grow an embedded assistant: something the trader talks to
mid-session — *read this chart, build me a strategy from that region, put a
trade here, lock the platform down, I am tilting*. It is not being built by
this PR, and this dimension never asks for assistant code. It asks that the
change not **close the door** on one. A capability that exists only as a
gesture is a capability the assistant can never have, and retrofitting it later
means reopening the very file under review.

The question to answer for every user-facing capability:

> Could an operator that is not holding the mouse — a script, a test, the
> future assistant — trigger this action, read back what it did, and discover
> that it exists, without a human clicking?

Three capabilities, each its own finding when it is missing:

- **Act — the action exists as a named call, not only as a gesture.** State
  mutated inside `if response.clicked() { … }` is not an action, it is a
  click. Lift the body into a named function that takes data —
  `place_drawing(tool, anchors, actor)`, `arm_strategy(preset, region, actor)`
  — and let the click call it. The actor rides in the signature, not beside
  it, because of the authorship rule below: a call that cannot say who asked
  cannot honour it. One path, never two: a separate "for the agent" entry
  point drifts from the one the trader uses and the operator ends up driving a
  ghost platform — the same reason `ui-harness` hooks call the manual toggle's
  own function instead of a parallel activation path.
- **Read — the result is legible as data, not only as pixels.** "Analyse this
  chart" is answerable only if bars, drawings, the position, the levels and the
  indicator readings can be enumerated by something that is not looking at the
  screen. A feature whose outcome lives in a widget's private fields, or exists
  only inside the paint call, is opaque — name the accessor or the snapshot
  type that is missing.
- **Discover — the capability announces itself in a registry.** A stable id in
  the *same* registry that feeds the UI, plus declared parameters wherever the
  capability takes any. Two exemplars, and they prove different halves:
  `DRAWING_TOOLS` (`crates/app/src/drawings/mod.rs`) is the id half —
  `QUANTICK_DRAWING_TOOL=<id>` reaches every tool precisely because one
  registry backs both the rail and the hook — while declaring no parameters at
  all. `IndicatorDescriptor::inputs` / `InputSpec`
  (`crates/indicators/src/input.rs`) is the parameter half: stable name,
  title, default and constraints, with the settings panel *generated* from it.
  Two findings live here — a capability that registers itself nowhere, and a
  list kept by hand beside a registry (one for the buttons, one for the agent),
  which diverges on the next PR.

**What the trader authors is data, not a rebuild.** When the new capability is
something the trader (or the assistant on their behalf) writes and varies — a
strategy, an alert, a checklist, a preset, a layout, an indicator of their own
— it belongs in a script or a config file loaded at runtime, not in an `enum`
that only grows through a build. The repo already decided this: `pine` turns a
`.pine` file into an `Indicator` with no recompilation, strategy presets live
in `quantick-strategies.toml`, looks live in `bubbles.toml`. This is **not** a
case against native code. The kernels in `indicators::native` (`ema.rs`,
`cvd.rs`, `avwap.rs`) are the shipped pattern and stay right: a kernel is
written by us, in Rust, once. The finding is narrower — a capability the
*trader* was meant to vary arriving as a compiled variant — and it names the
script or config file that would have avoided the rebuild.

**Where this layer does not go.** Performance (priority 1) outranks the
operability half of this dimension. It does not outrank the authority half
below, which is priority 0. A command is emitted when a *person acts*: a
click, a hotkey, a panel edit, one instruction to the assistant. That is the
only rate this dimension licenses, and it is explicitly **not** "once per
bar" — quantick's bars close on volume, not on a clock, so a small tick-bar
threshold under a dense tape closes them hundreds of times a second, and
`Indicator::preview` runs on the *forming* bar, so script reached from there
is on the hot path whatever the commit/preview contract says (that contract is
about rolling back staged state, not about rate). Classify with dimension 2's
table and nothing else: an interpreter, a string lookup or a dynamic dispatch
table in the aggregator, in `preview`, or in the renderer is a performance
finding first and an operability merit second.

**Authority is declared, and so is the author — at priority 0.** This
paragraph is safety, not shape: it is a precondition, and no performance win
buys it off. An exposed action states which kind it is: observation, cockpit
change, or market/safety action (send an order, lock the platform). The last
two cross the same arming and confirmation the trader crosses — a surface that
lets a non-human operator reach an order by a shorter path than the trader's
is a Blocker. And whatever acted is recorded: an order, a drawing or a preset
produced by something other than the trader's own hand is labelled as such,
under the same data-honesty rule that labels an inferred side. An object the
assistant placed that is indistinguishable from one the trader placed is a
finding.

This is the runtime twin of the `ui-harness` hook rule, not a duplicate of it:
a hook proves a surface can be *reached from a launch*, this dimension asks
whether the action can be *taken while the session runs*. One extraction
usually satisfies both — the hook and the agent call the same named function.

Naming note: the repo's `copilot.pine` is an indicator, not this assistant. Do
not reuse the name for the agent surface.

