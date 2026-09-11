# Mission: discreet SL/TP tags

**Objective.** Shrink the in-plot tag on stop-loss / take-profit lines into a
discreet pill in the same grammar as a resting order's tag, so the labels stop
covering the chart. Today every leg tag carries the full statement
(`#3 SL 99.50 -5.0 pts · on fill` plus a ✕) at all times, while a resting
order rests as a short pill and opens only under the pointer; with a position
and two orders bracketed, the tags bury the candles they sit on.

**Tier:** medium — raised from `small` mid-mission. `paper_trading.rs` sat
exactly at its size ceiling, so the leg painter moved into its own module
(`paper_trading/leg_tag.rs`), as the size guard asks. The move alone put the
diff past `small`'s 300-line exemption (about 520 changed lines). The work is
still one painter in `quantick-app`: no engine, no order semantics changed.
The diff was not shrunk to stay `small`; the tier went up.

## Request ledger

- **R1** — make the label on the TP and SL lines smaller ("diminuir tamanho da
  label").
- **R2** — discreet, the same as the order tag on the chart ("algo discreto
  igual a ordem").
- **R3** — purpose, judges the others: the labels must stop covering the chart
  ("ela tampa quase tudo").
- **R4** — use five agents to settle the best design, including the opinion of
  a trader who looks at how it turned out on screen.

## Assumptions

- **S1** — "the TP/SL label" is the in-plot tag on a leg line. The gutter
  price chip is not it: the resting order keeps the same chip, so "like the
  order" keeps it. Safe: the chip is one price wide and already the order's
  own grammar.
- **S2** — "like the order" means the order tag's two states: a short pill at
  rest, the full statement (and the ✕) when the pointer is on the line's row.
  Safe: it is the only other tag grammar on the chart, and it is reversible in
  one edit.
- **S3** — the five agents are one parallel panel over the *before* captures;
  its two trader seats then grade the *after* captures. Safe: it is the count
  asked for, and the after-look is the trader opinion the request names.
- **S4** — what the pill keeps at rest was decided by the panel (R4), not
  here: the leg word, its signed points, and the order id on a pending leg;
  pending legs are ghost pills. Record:
  `.claude/evidence/discreet-bracket-tags/panel.md`.
- **S5** — *wanted to ask (a narrowing)*: ladder rungs and the cmd aim preview
  keep their current tags. Reading taken: the complaint's scene is the whole
  bracket (a position's and an order's two legs); rungs exist only under a
  laddered strategy, and their press path carries a separate pre-existing
  defect the engineer seat found; the aim preview is transient and already a
  single label. Stated in the PR body as deferred, so the trader can overrule
  it.
- **S6** — `QUANTICK_PAPER_ORDER_HOVER` now forces the leg tags open too, so a
  capture reaches their open form. Safe: capture-only hook, default off.

## Acceptance criteria

- [x] **A1** — at rest (no pointer on its row, no drag) a leg tag paints a
      short pill with no ✕, in the order pill's height and font, and its text
      is shorter than today's resting statement.
      *Evidence:* tests `a_bracket_leg_tag_is_a_pill_until_the_pointer_reaches_it`
      and `a_position_leg_rests_as_the_leg_and_its_points`, plus the
      pixel-measured tag width before vs. after.
      → `crates/app/src/paper_trading/tests/mod.rs`, `panel.md`. *(R1, R2, R3)*
- [x] **A2** — the full statement (id, price, points, `on fill`, R:R while
      dragging) and the ✕ are still reachable: they appear when the pointer is
      on the leg's row, and a ✕ press is offered exactly when the ✕ is painted.
      *Evidence:* tests `a_leg_offers_its_clear_exactly_while_it_paints_one`
      and `a_working_orders_legs_are_draggable_and_clearable` (no clear at
      rest, the clear once the row is open).
      → `crates/app/src/paper_trading/tests/mod.rs`. *(R2)*
- [x] **A3** — before/after captures of a bracketed order and of a position
      show the leg tags covering materially less of the plot.
      *Evidence:* same-tape capture pairs, tag widths 248 → 83 px and
      157 → 59 px. → `.claude/evidence/discreet-bracket-tags/panel.md`, PR
      body. *(R1, R3)*
- [x] **A4** — a five-agent panel's recommendation is recorded, and the trader
      seats' read of the after captures has no unresolved Blocker.
      *Evidence:* panel synthesis and both trader verdicts.
      → `.claude/evidence/discreet-bracket-tags/panel.md`. *(R4)*

## Injected gates

- [ ] **G1** — every artifact in English (`CLAUDE.md`; arch-review dimension 8,
      `crates/guards/src/language.rs`). → arch-review verdict.
- [x] **G2** — fmt, clippy, build and test green after rebasing on latest
      `main`. → PR body.
- [x] **G3** — performance impact declared: the tags paint per frame, per
      visible leg. At rest the change formats a shorter string, and it adds
      one row-hit test per existing leg per frame in `fill_open_legs`, pushed
      into the already-reused `open_tags` buffer. No new allocation class and
      no per-trade or per-depth path. → PR body.
- [ ] **G4** — `arch-review` run, every Blocker/Should-fix resolved or deferred
      in the PR body. → PR body.
- [x] **G5** — user-visible: the surface is reached by existing hooks
      (`QUANTICK_PAPER_ORDERS`, `QUANTICK_PAPER_ORDER_BRACKET`,
      `QUANTICK_PAPER_ORDER_HOVER` — the last now opens the legs too, registry
      regenerated). Visual check of the same-tape captures: tags inside the
      pane, ghost ink legible. Trader review with no unresolved Blocker.
      → PR body, `panel.md`.

Not applicable: hot-path evidence (no per-trade or per-depth path is
touched); `new-extension` (no capability added, and `leg_tag.rs` is a module
moved out of the trunk, not a port); second-operator (no new action:
clearing a leg keeps its existing named routes); engine determinism (no
engine code).

## Closing steps

- **C1** — `delivery-review` completeness pass (inline, `medium`) returns
  PASS.
- **C2** — the PR is open, ready, CI green, with the evidence in its body.

## The request as received

Attributed quotation of the trader's request, verbatim (exempt from the
English rule as a marked quotation):

> small diminuir tamanho da lable que fica no tp e sl macado no gracio. Quero
> algo discreto igual a ordem que fica no grafico. Pois ela tampa quase tudo.
> Use 5 agentes para definir o melhor design e opinao de um trader que vai
> olhar graficamente cmo ficou
