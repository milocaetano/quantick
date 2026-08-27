# Mission

Give the region strategy an audible **signal alarm**: alert the trader the
moment the trigger signal occurs — at bar close, or at a configurable share of
the forming bar's progress — with repeat control and a choice of system
sounds, so the trader can hear the opportunity in time to execute it on
another platform.

## Why the alarm is not the order

The alarm fires on the **signal**, never on the order placement. The trader
executes on a different platform; the local paper order (when there is one) is
a bookkeeping side effect. Two consequences the design must honour:

- With the mid-bar option on, the alarm deliberately fires *before* the
  strategy could ever place an order — the strategy still judges closed bars
  only. That head start is the feature.
- The alarm's gates are the opportunity's gates, not the account's: trigger
  fired + correct side + region active. A busy paper account or a spent
  one-shot instance silences an order, never the alarm.

## Decisions taken with the trader (2026-08-27)

1. **Gates** — signal + region + side only. `account_flat` and the armed
   state machine's `Done`/`Fired` do not suppress the alarm.
2. **Sounds** — the platform's own sounds only, chosen from a named set. No
   audio engine, no new dependency: `crates/app/src/audio.rs` refuses one by
   design and that refusal stands. The choice is a port, so a future file-backed
   sound docks without surgery.
3. **Alarm-only mode** — an instance may watch, alarm and draw without ever
   emitting a command to the simulator.
4. **Provisional signals are labelled** — a mid-bar alarm is a *preview*: the
   bar may stop qualifying before it closes. The badge says "preview", and a
   preview that fails to confirm at close is reported visually (no second
   sound). Data honesty: inferred data is labelled, never silently patched.

## Acceptance criteria

### Mission-specific

1. The arming dialog carries an **"alarm on signal bar"** checkbox, off by
   default, so every strategy armed before this change behaves exactly as it
   does today.
2. **When** it alarms is configurable: on bar close, or once the forming bar
   has passed a configurable share of its closing measure (the trader's
   example: a 2000-tick chart set to 70% starts judging past tick 1400).
   The share is expressed against `engine::BarProgress`, so the same control
   reads correctly on tick, volume, dollar and time bars.
3. A bar rule that runs toward **no fixed threshold** (an adaptive rule, whose
   `BarBuilder::progress()` is `None`) cannot honour a share gate. The app says
   so in the dialog and falls back to on-close alarms rather than inventing a
   percentage.
4. **Repeat control**: either at most one alarm per bar, or a cooldown of N
   seconds between alarms. One of the two is always in force — a mid-bar alarm
   never repeats on every print.
5. **Sound choice**: the trader picks from a named set of platform sounds and
   can audition the pick from the dialog. A build whose platform has no sound
   reports that instead of pretending the alarm was heard.
6. **Alarm-only mode**: an armed instance can be configured to emit no
   simulator commands at all. Its badge says so.
7. A mid-bar alarm is labelled **preview** on the chart; if the bar closes
   without confirming the signal, the app says that too.
8. The alarm settings round-trip through the strategy bank (`StoredPreset`),
   and a bank file written before this change still loads with the alarm off.
9. The alarm fires on signal + region + side alone: covered by a test where the
   paper account is not flat and the alarm still sounds.
10. The mid-bar judgement never reaches the armed state machine: covered by a
    test proving a preview signal emits no `sim::Command`.

### Standard gates

- **English throughout** — every identifier, comment, UI string, test name and
  doc line. Graded by `arch-review` dimension 8, enforced by
  `crates/app/tests/language_guard.rs`.
- **Four checks green** after rebasing on latest `main`: `cargo fmt --all --
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`.
- **Performance impact declared** — every touched path classified by rate
  (per-trade / per-depth / per-frame / rare) in the plan, not in the review.
  The mid-bar evaluation is a **per-trade** path and must be gated cheaply:
  no trigger work before the progress share is reached, and none while the
  repeat gate is closed.
- **Hot-path evidence** — `APP_HEALTH_SUMMARY` fps/frame_avg under a dense
  tape against a `main` control run, numbers in the PR body.
- **UI harness** — every new or changed surface reachable by an env hook added
  in this same change (`ui-harness`).
- **visual-qa** — all surfaces PASS, or defects explicitly accepted.
- **trader-ux-review** — no unresolved Blocker.
- **new-extension** — the sound choice is a named port with registration-only
  edits and a fake second implementation under test; defaults preserve today's
  behaviour; blast radius (added vs. edited files) stated in the PR body.
- **The second operator** — the alarm is configurable, armable and readable
  without a mouse, per `arch-review`'s *The second operator* criteria.
- **Test-first in the kernel** — `quantick-strategy` is a pure domain crate:
  fixture bars plus expected output written before the code, determinism
  guarded (no wall clock inside the kernel; the cooldown's clock is the app's,
  injected).
- **arch-review** run over `git diff main...HEAD`, every Blocker and
  Should-fix resolved or deferred in the PR body.
- **PR opened** — the mission ends at an open PR with green CI. Merging is the
  trader's call.

## Ground

- Branch: `feat/strategy-signal-alarm`
- Worktree: `../quantick-worktrees/feat-strategy-signal-alarm`
- Cut from: `origin/main` @ c47de18
