---
name: trader-ux-review
description: Review a feature or flow through the eyes of the traders who use quantick — fixed personas, order-flow-specific heuristics, and trader-severity ratings. Answers "can a trader actually operate with this?", not "does it render?" (that is visual-qa). Use for any change a trader touches mid-session — panels, popups, hotkeys, trading actions, chart interactions — or when the user asks for a UX review.
---

# Trader UX review — the user is watching a live tape, not your feature

A trader's attention is the scarcest resource in this product. Every review
question reduces to: **does this cost attention, clicks, or trust at a
moment the market is moving?**

Judge from screenshots and flows produced via `visual-qa` / `ui-harness`,
plus reading the interaction code. Review *flows*, not screens: entering a
trade, adjusting a stop, switching timeframes — walk each affected flow
start to finish as each persona.

## The personas

Review as all three; they disagree, and the disagreements are the findings.

**Rafa — order-flow scalper (WIN, tick/volume bars, seconds matter).**
Eyes locked on tape + book + bubbles. Decides in under a second, acts by
hotkey, never by menu. Anything that interrupts the stream mid-decision —
a popup stealing focus, a confirm dialog on a flatten, a layout jump on bar
close — is money lost. Judges every control by "can I hit it without
looking?" and every piece of information by "is it true *now*?".

**Marina — swing/context trader (BTC + índice, multiple timeframes).**
Works across tabs and split panes; compares, measures, draws. Cares about
consistency (the same gesture must mean the same thing in every pane),
persistence (her drawings, presets and layouts must survive a restart), and
honest history (backfilled or inferred data clearly marked). Tolerates
menus; does not tolerate relearning.

**Duda — newcomer on paper trading (learning the platform).**
First week. Doesn't know the hotkeys or the jargon. Needs discoverability
(can she find the action without the manual?), safe defaults (a mis-click
must not do something irreversible), and states that explain themselves (a
disabled button with no reason reads as a bug). If Duda misreads a label as
financial fact — a simulated P&L that looks real, an inferred side shown as
truth — that is the worst finding in this file.

## Heuristics (order-flow specific)

- **Interruption budget: zero.** Nothing steals keyboard focus or covers
  price/tape/forming bar uninvited. Confirmations only for destructive,
  irreversible acts — and never on the exit path of a losing trade
  (flatten must be instant; precedent: flatten-on-reset, Shift+X).
- **Action cost.** Count clicks/keys from intent to done for the critical
  actions (enter, stop, flatten, switch symbol/timeframe). Critical = 1
  gesture. Compare against the flow before the change: any regression in
  gesture count is a finding.
- **Glance cost.** The key number of the feature must be readable from the
  chart without leaning in, at the moment it matters. Where does the eye
  have to travel?
- **Trust.** Data honesty is UX: inferred, simulated, delayed or
  incomplete data is labelled at the point of reading, not in a tooltip.
  Sim results say sim; tape-proven fills say how they filled.
- **Stability under fire.** Fast tape, bar close, feed reconnect: does the
  layout hold still? Do controls stay where muscle memory expects them?
- **Latency is UX.** A frame hitch during a fast tape *is* an interruption
  — Rafa reads flow from motion, and a stutter breaks the read exactly when
  the information peaks. A feature that looks perfect in a screenshot but
  drops frames under a dense tape fails Rafa even with zero visual
  findings. Repo priority order applies here too: never trade runtime for
  prettiness.
- **One language.** Same chips, same button pairs, same placement grammar
  as existing surfaces. A trader configures once and expects the dialect
  everywhere.

## Severity — in trader terms

- **Blocker** — can cost money or the moment: focus theft mid-tape, an
  extra gesture on flatten/stop, simulated or inferred data readable as
  real, an irreversible act one mis-click away.
- **Should-fix** — costs attention or trust: layout jump, inconsistent
  gesture, unexplained disabled state, key number needing a menu.
- **Consider** — polish that would make a persona faster but blocks no one.

## Output

Per affected flow: the persona walk (what each persona hits, in one or two
sentences each — only where they diverge), then findings ranked by
severity, each with the flow, the persona who suffers, the evidence
(screenshot or `file:line`), and the concrete fix. Close with one line per
persona: can Rafa trade through it, can Marina keep her workspace, can Duda
figure it out alone? A clean review says so briefly — never pad.

Blockers and Should-fixes feed the same gate as `arch-review`: resolve or
explicitly defer in the PR body.
