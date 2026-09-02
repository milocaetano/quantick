# Dimension 1 and dimension 9 — docking, and where the docking landed

Read before writing a modularity or accumulation finding.

The question to answer for every new capability:

> Could a second implementation of this be added by writing a new file and one
> registration line, without editing any existing behaviour?

If not, say exactly which file the next author would be forced to open, and
what port would have prevented it.

Look for:

- **Type switches that grow.** `match` over a closed enum, `if is_replay`,
  `if feed == "binance"` — every new variant reopens the same function. The
  repo's own answer is capability-driven behaviour: UI gates on
  `FeedCapabilities`, never on which source is playing. Follow that pattern.
- **Ports vs. concrete types.** Does a consumer depend on a trait it needs, or
  on a concrete producer it happens to have? Downcasting or matching on a
  concrete feed type downstream is a broken port.
- **Dependency direction.** One way: `app` → `pine` → `indicators` →
  `engine`, `app` also on `orderbook` / `replay` / `feed-*`, and `feed-*` →
  `engine` / `orderbook` only. Feed crates never depend on each other. A
  reverse edge is a blocker, no exceptions.
- **Forked logic.** Chart, backtest and bot share one aggregator code path.
  Any per-consumer copy of bar building is a blocker.
- **Blast radius.** Count the existing files the change modifies versus the
  files it adds. A feature that is mostly edits to existing code either found
  a missing abstraction or ignored one — decide which and say so.
- **Additive by default.** Adding a capability must change nothing until it is
  asked for: new options default to today's behaviour, and config presence
  alone never activates anything.

## Dimension 9 — the trunk

Dimension 1 asks whether a capability *can* dock. This one asks where the
docking went. The two are not the same question, and the gap between them is
how this repo acquired a 36,000-line file while every review passed honestly.

A change adds `pointer_compass.rs` — a real new module, a real port question
answered yes — and then adds a field to `QuantickApp`, an init to
`new_with_workspace`, a draw call to `draw_frame` and a hotkey to
`draw_menu_bar`. Four edits, one file, and dimension 1 saw only the new
module. Repeat sixty-eight times: 133 fields, a 1,149-line constructor, and a
struct that *is* the registry — implemented as a struct, so the only way to
extend it is to edit it.

Look for:

- **Growth in the trunk.** `crates/guards/src/size.rs` records a ceiling
  for every file over 1,500 production lines — every line outside a top-level
  `#[cfg(test)]` item — and fails when one grows past it. Check what it counts
  before trusting what it says: the first version of that guard stopped at the
  first `#[cfg(test)]` of any kind, which scored `control/gateway.rs` at 72
  lines of its 4,142 and left five of the largest files in the repo untracked.
  A mechanical half that is blind where the debt is largest is worse than none,
  because a review cites it and stops looking. That guard is this dimension's mechanical half, as `crates/guards/src/language.rs`
  is dimension 8's; the judgement half is yours. Raising a ceiling stays
  legitimate — it is a visible, signed line in the diff — but it is a finding
  to argue with, never a silent act. A branch that raises one without saying
  why in the comment beside it has recorded a decision rather than made one.
- **A registry that is a closed enum.** An entry that cannot join without a
  `match` arm is a type switch wearing a registry's name, and dimension 1's
  first bullet already forbids the shape. Two of the ports `new-extension`
  recommends are exactly this today: `ChartLayer` carries 21 variants across
  264 `ChartLayer::` sites in six files, `DockTab` another 64. Adding a layer
  reopens `app.rs`, `pane.rs`, `tab.rs` and `toolbar.rs`. When a change adds a
  variant, ask what a trait object in a registry would have cost instead — and
  when it adds the *second* variant of a kind, that is the moment the port was
  due.
- **Blast radius in lines, not only files.** `new-extension` §3 counts files
  added versus edited, and a change adding one file while pouring 2,000 lines
  into thirteen others passes that count looking healthy. Count the lines too.
  Mostly-edits by line is the finding dimension 1 already names, one magnitude
  louder.
- **Host or participant?** When a change puts state on the application's root
  struct, ask whether that struct has any reason to know about it — whether
  anything *else* reads the field. State a single surface owns belongs to that
  surface, not to its host. Nine of `app.rs`'s twenty-one `draw_*` surfaces
  touch one or two fields of `QuantickApp`; those are not entangled designs,
  they are modules filed in the wrong place, and saying so is cheap while they
  are still small.

The distinction this dimension turns on: a codebase becomes unmaintainable
from wiring debt long before it does from design debt, and the two look
identical in a file listing. Establish which one is in front of you before
prescribing — an extraction is mechanical and safe, a redesign is neither.

