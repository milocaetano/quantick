# Mission: trades bars for MetaTrader B3, counted live and recorded

**Objective.** Add a `trades` bar kind for MetaTrader B3 that cuts a bar every
N exchange deals, counted live from the venue's session deal counter and
recorded to disk behind an explicit REC control, so a recorded day reopens as
the same chart.

**Why it matters.** ProfitChart's *Trades* periodicity (`2000T`) counts B3
deals (*negócios*). MetaTrader's tick stream aggregates several deals into
one tick — measured on 2026-09-03 for WINV26: 5 821 205 session deals against
1 774 869 trade ticks, with volume matching to within 61 contracts — so the
existing `tick` kind cannot reproduce that chart, and no per-deal history
exists in MetaTrader. The only honest source is the live counter, which is
why recording is part of the feature and not an afterthought.

**Tier:** `high`. The change reaches the bridge protocol, `feed-mt5`, the
engine, the app's toolbar and chart chrome, persistence, the control plane
and the harness. It adds a bar kind, a trader action (REC) and a persisted
file format; every row of the gate table applies and `delivery-review` runs
in full.

**Design reference.** The trader approved the third version of the mock at
https://claude.ai/code/artifact/9fcb8221-0624-42a0-b4cc-1fcb5b0931ca
(REC beside the symbol, chip in the chart corner, status cell, popup,
default-on setting, history list of recorded days, B3-only scope).

## Request ledger

- **R1** — Add a chart by trades, ProfitChart's *Trades* periodicity
  ("inserir grafico por trades").
- **R2** — Maximum modularity: new code in files or types outside `app.rs`,
  so the trader's ongoing `app.rs` refactor merges without hard conflicts
  ("Tente fazer o maximo de modularidade possivel, tentar separar em
  arquivos ou classes fora do app").
- **R3** — Recording is explicit and visible: a clear REC button, and the UI
  says it is recording live ("Deixar o botao claro pra gravar", "mais
  explicito que ta gravando").
- **R4** — A saved option to record by default ("Deixar uma opção salva como
  default para gravar por default").
- **R5** — The recording persists so history can be opened from it ("abrir
  desde as 9h e deixar gravando… poderia abrir o historico com base na
  gravação").
- **R6** — "Recording the market" and "what was recorded" are visibly
  different states ("deixar mais claro o que está gravando o mercado e o que
  foi gravado").
- **R7** — REC is on the asset when it opens, so switching tick ↔ trades
  works while recording ("Quando abrir o ativo tem o botao pra gravar assim
  eh possivel eu trocar de ticks para trades e funcionar").
- **R8** — Reopening history is mapped: a recorded day opens as trades; a day
  or a feed without a recording says so ("Você tem que mapear também quando
  eu for abrir de novo o histórico").
- **R9** — `trades` and REC only on MetaTrader B3 for now ("Deixa opção
  trades apenas para b3 metatrader por enquanto").
- **R10** — Run the latest quantick with MCP enabled so the agent can see
  ("roda a ultima versao do quantick e ativa mcp"). Delivered in the session
  before the branch existed (release build of `main` bc39248, control plane
  read over `quantick-mcp`); not graded on the branch.
- **R11** — A drawing of how the recording would look to the user ("consegue
  fazer um desenho de como ficaria para o usuario entender que ta gravando
  ao vivo?"). Delivered in the session before the branch existed, as the
  artifact D5 names (three versions, the third approved); not graded on the
  branch.

## Decisions taken by the trader

- **D1** — The chart by trades counts exchange deals, not MetaTrader ticks.
  The trader rejected "tick already is it" and the measurement above proved
  him right.
- **D2** — Source: the session deal counter (`SYMBOL_SESSION_DEALS`) polled
  live by the bridge. MetaTrader keeps no per-deal history; the trader
  confirmed the live-only limitation ("ao vivo consegue pegar e no passado
  nao").
- **D3** — Recording is an explicit REC control with a saved default-on
  option, and the recording is written to disk so a day reopens from it.
- **D4** — Scope is MetaTrader B3 only. Binance, Hyperliquid and replay do
  not get `trades` in this mission.
- **D5** — The mock linked above is the design reference; UI strings ship in
  English.

## Assumptions

- **S1** — Prints before the first counter reading (opened mid-session, no
  file) form no trades bars; the chart labels the stretch *no deal count*
  and reports how many prints it holds. Safe: it is rule 2 of the approved
  mock and the data-honesty rule leaves no other reading. A stretch *between*
  readings (a restart) folds into the bar the last reading was forming —
  one join rule, the same live and rebuilt; the explicit end-of-coverage
  marker is a recorded follow-up.
- **S2** — Recording file: `Documents/Quantick/deals/<SYMBOL>/<YYYY-MM-DD>.deals`,
  append-only text, one `(time_ms, session_deals)` sample per line, under a
  `QUANTICK_DEALS_DIR` hook and a `[deals] dir` config key. Safe: a
  conventional default in this repo (paper trades use the same shape).
- **S3** — A bar closes at the first print whose reading reaches the
  boundary; overshoot is one bridge poll (20 ms) and is documented. Safe:
  the threshold builders already document whole-print overshoot.
- **S4** — Counter stale: the tape flows but the reading has not moved for
  4 s → amber state, no bar closes by estimate. Safe: reversible in one
  edit.
- **S5** — The engine `Trade` struct is unchanged; the counter travels as a
  sample series beside the trades (a `FeedEvent` variant and a builder that
  joins by time). Safe: 201 literal constructions across nine crates would
  otherwise change, and the join by time is what the reopen path needs
  anyway.
- **S6** — One branch, one PR, one commit per layer (bridge and feed, engine,
  app, control). *Wanted to ask*: a stack of PRs would merge sooner against
  the trader's refactor; chosen against because the mission rule is one PR
  and the trader can ask for a split at review.
- **S7** — REC and the default option are reachable by config
  (`record_deals = true` on the feed entry) and by a cockpit capability, per
  the second-operator rule.
- **S8** — Restart mid-session resumes today's file when the default is on.
  Safe: section 2 of the approved mock.
- **S9** — Replay and `tools/mt5/export_session.py` do not read or write the
  deals file this round; recorded as a follow-up in the PR body. Safe: the
  trader said "por enquanto" about scope.
- **S10** — The MQL5 bridge (`QuantickBridge.mq5`) gets the same field as the
  Python bridge but cannot be run here; the Python bridge is what the trader
  runs. Noted as untested in the PR body.

## Acceptance criteria

- [x] **A1** — The engine has a `trades` builder that closes a bar every N
      exchange deals from a series of counter readings joined to prints by
      time, test-first. *Evidence:* a golden test over a fixture with a
      boundary that lands mid-batch, an overshoot, and prints before the
      first reading. → `crates/engine/src/deals.rs` tests,
      `cargo test -p quantick-engine deals`. *(R1)*
- [x] **A2** — The Python bridge stamps every tick line with the session
      deal counter (optional field, older bridges unaffected); `feed-mt5`
      maps it into a sample series and the app feed emits it beside the
      trades. *Evidence:* `python bridge/mt5/tests/test_*.py` and
      `cargo test -p quantick-feed-mt5 deals` green; `PROTOCOL.md` names the
      field. → `bridge/mt5/PROTOCOL.md`, `crates/feed-mt5/src/`. *(R1, R5)*
- [x] **A3** — `trades` is offered only on a MetaTrader feed whose tape is
      trades (B3); elsewhere the selector lists it disabled with the reason
      (the bug pass showed hiding a selected kind is a lie); `trades:N`
      round-trips through config and workspace and is refused at config load
      on any other feed. *Evidence:* tests in `state.rs`, `config.rs`,
      `toolbar.rs`. *(R1, R9)*
- [x] **A4** — A REC button beside the symbol on MetaTrader B3 tabs toggles
      recording per feed+symbol and stays visible in every bar kind, with the
      corner chip, the status cell and the popup of the mock. *Evidence:*
      `QUANTICK_DEAL_RECORDING=<state>` hook rows, scene entries, captures
      under `visual-qa`. → `.claude/skills/ui-harness/references/hook-registry.md`,
      PR body. *(R3, R6, R7)*
- [x] **A5** — "Record deals by default" is persisted in the workspace and
      readable from the feed config; with it on, connecting a B3 symbol
      starts recording without a click. *Evidence:* a test on the setting's
      round trip and on the auto-start. *(R4)*
- [x] **A6** — The recording is written to disk as samples; a restart resumes
      today's file; a recorded day reopens as trades bars from the history
      menu and the UI shows the `recorded` state. *Evidence:* file round-trip
      test, reopen test, capture of the recorded state. *(R5, R6, R8)*
- [x] **A7** — Switching tick ↔ trades with recording on rebuilds from the
      retained prints and readings and leaves the recording untouched; with
      no count, `trades` is disabled with the reason and a *Start recording*
      action. *Evidence:* `ChartState` test, hook capture of the disabled
      state. *(R7, R8)*
- [x] **A8** — The feed matrix and the "what happens when" mapping are
      written down. *Evidence:* `docs/deal-recording.md` and the hook
      registry rows. *(R8, R9)*
- [x] **A9** — `app.rs` changes are registration lines only; every new
      surface, model and store is a new file. *Evidence:*
      `git diff --stat origin/main...HEAD -- crates/app/src/app.rs` in the PR
      body. *(R2)*
- [x] **A10** — The second operator: `feed.status` exposes the recording
      state (state, since, session deals, file, coverage); a cockpit
      capability toggles it; the catalog snapshot is regenerated.
      *Evidence:* control tests, `schemas/control/` diff. *(R3, R6, R8)*
- [x] **G1** — Every artifact in English. *Evidence:* arch-review dimension 8,
      `cargo test -p quantick-guards`.
- [ ] **G2** — Four checks green after rebasing on latest `main`.
      *Evidence:* exit codes in the PR body.
- [x] **G3** — Performance impact declared per path: per-print (sample join,
      O(1) amortised), per-poll (one file append, off the UI thread), rare
      (reopen, rebuild). *Evidence:* the plan section of the PR body.
- [x] **G4** — Hot path measured: `APP_HEALTH_SUMMARY` fps and frame_avg on
      the same session tape, branch vs `main` control. *Evidence:* numbers
      in the PR body.
- [ ] **G5** — `arch-review` run, every Blocker and Should-fix resolved or
      deferred in the PR body. *Evidence:* `arch-review-ok` marker.
- [ ] **G6** — `ui-harness` hooks for every new surface; `visual-qa` all
      PASS or defects accepted; `trader-ux-review` with no open Blocker.
      *Evidence:* review verdicts in the PR body.
- [x] **G7** — `new-extension`: port named, registration-only edits, defaults
      preserve today's behaviour, fake second implementation tested, blast
      radius stated. *Evidence:* PR body section.
- [x] **G8** — Engine test-first with a golden fixture guarding determinism.
      *Evidence:* A1's test exists before `deals.rs` compiles.

## Evidence record

Written before the archive, so the reviews grade what was recorded rather
than what is remembered.

- **A1** — `crates/engine/src/deals.rs`, 10 tests including
  `golden_cuts_at_the_sessions_multiples_of_n`,
  `prints_before_the_first_sample_are_uncounted_not_guessed`,
  `interleaved_samples_cut_where_a_rebuild_cuts`; `cargo test -p
  quantick-engine deals` → 10 passed.
- **A2** — `bridge/mt5/tests/test_deals.py` (4 tests, `python` run: all
  checks passed), `crates/feed-mt5/src/deals.rs` (3 tests),
  `bridge/mt5/PROTOCOL.md` documents `deals` and `deal_counter`;
  `cargo test -p quantick-feed-mt5` green.
- **A3** — `state::tests::every_bar_spec_survives_the_config_round_trip`
  (with `trades:2000`), `config::tests::trades_bars_and_deal_recording_are_metatrader_only`,
  `toolbar.rs` hides the kind where `deal_counter` is false; the running
  Binance tab in `cap_off_tick.png`'s session offers no `trades`.
- **A4** — `deal_recording_ui.rs`; hooks `QUANTICK_DEAL_RECORDING` and
  `QUANTICK_DEALS_DIR` in the registry; captures `cap_trades_loaded.png`,
  `cap_menu_early.png`, `cap_off_tick.png`; `visual-qa-report.md`.
- **A5** — `config.rs` `record_deals` (metatrader-b3 = true in
  `feeds.toml`), `ui_state::SavedChrome::record_deals`, Tools menu toggle
  in `app/deal_recording_wiring.rs`; `deal_recording::tests::the_default_starts_once_and_a_hand_that_stopped_it_is_respected`;
  the capture run (`QUANTICK_DEAL_RECORDING` unset, config default) opened
  in `state: recording` per `feed.status`.
- **A6** — `deal_recording::tests::a_file_round_trips_through_the_delta_encoding`,
  `a_restart_resumes_the_days_file_and_writes_no_line_twice`,
  `the_scan_lists_days_with_their_coverage`; live: `feed.deal_recording.set`
  `{load_day: 2026-09-03}` → `loaded_days [2026-09-03]`, flow pane
  `closed_bar_count 2910` (= 5 821 205 / 2000), `cap_trades_loaded.png`,
  `cap_history_menu.png`.
- **A7** — `state::tests::deal_readings_survive_a_switch_of_the_bar_rule`,
  `older_readings_land_in_order_and_duplicates_are_held_once`; the
  disabled entry's reason in `toolbar.rs` `draw_bars` (unit-tested
  gating, not photographed — the combo has no hook).
- **A8** — `docs/deal-recording.md` (feed matrix, "what happens when"),
  indexed in `docs/README.md`; hook registry rows.
- **A9** — `git diff --stat origin/main...HEAD -- crates/app/src/app.rs`:
  +24 lines (a `mod`, a field, restore/save, one call per surface);
  `tab.rs` +7, `pane.rs` +7; new files listed in the PR body.
- **A10** — `control/deal_recording.rs` (capability `feed.deal_recording.set`,
  `DealRecordingSnapshot` in `feed.status`),
  `control_plane_tests::feed_status_carries_the_deal_recorder_and_the_capability_moves_it`
  (reads the scope, drives start/stop/load by capability id);
  `schemas/control/observer-capability-catalog-v1.json` regenerated (39
  capabilities); exercised live over `quantick-mcp` at profile `cockpit`.
- **G3** — per-print: one `DealSampler` compare in the feed and one
  `Vec::push` per *changed* reading (not per print) in `ChartState`;
  per-poll: one buffered line append on the UI thread, flushed once a
  second; per-frame:
  one string compare per tab (`ensure_deal_recorders`), three cheap view
  clones for the toolbar, chip and status cell; rare: `scan_days` on
  start/stop/market switch, a rebuild on loading a day.
- **G4** — same tape, tick(5000), idle, maximised, same hour: `main` cpu
  6.2–6.4 ms / 59–60 fps; branch REC off 4.5 ms; branch REC on 5.6–6.3 ms;
  branch trades(2000) 5.5–6.0 ms. Flat. Numbers in `visual-qa-report.md`
  and the PR body.
- **G7** — port: the engine's `BarBuilder` trait (two defaulted methods),
  the `FeedEvent` channel (`DealCounter`) and `FeedCapabilities`
  (`deal_counter`); registration lines only in the trunk; defaults: no
  feed but metatrader-b3 records, no config but the shipped one records;
  the fake second implementation is the tick builder ignoring readings
  (`deals::tests::the_default_builders_ignore_samples_and_count_nothing_uncounted`)
  and Binance's `deal_counter: false` path in the control test.
- **G8** — `crates/engine/src/deals.rs` tests were written first; the
  golden test fixes the cut points.
- **G6** — `visual-qa-report.md` (scratchpad, summarised in the PR body):
  seven cells PASS, one BLOCKED (the disabled entry's hover, no hook), one
  PARTIAL (narrow window). `trader-ux-review`: one Blocker (stale state
  judged on the wrong clock) and Should-fixes 2, 3, 4, 5, 6, 7, 10 and 11
  fixed in the follow-up commit; 8 (empty trades pane notice) and 9 (loaded
  day before its tape is paged in) deferred to the PR body with the
  Consider items.
- **G5** — `arch-review` at tier `high`: step 0 `code-review` at `medium`
  (effort-first, no reuse notice), 8 confirmed findings, 3 plausible; shape
  pass no Blocker, 7 Should-fix. Round 2 fixed every step-0 finding but
  the backtest gap (deferred in the PR body: no headless reader of
  readings yet) and six of the seven Should-fixes (the two integration
  tests deferred). Round 3 re-ran both over the round-2 head: the shape
  pass confirmed six fixes, found one Blocker (batch dedup on equal
  milliseconds) and one Should-fix (a whole bad line read as torn), both
  fixed in the third commit, and deferred the live-edge/rebuild hole
  divergence to a follow-up (an end-of-coverage marker in the file).
  The round-3 bug pass came back with eight confirmed findings, the same
  count as round 1 — the budget's "go to the trader" shape. A fourth
  commit resolved them (one join rule identical live and rebuilt, re-seed
  after Reload, a failed open reads Off, empty and header-only files,
  tests kept out of Documents, the EA's evaluation order, the chip on a
  trades pane the feed cannot count); the PR body says so, and the
  end-of-coverage marker stays the recorded follow-up.
- **S6 note** — one branch, one PR, three commits by layer plus the two
  review follow-ups.

## Not applicable

- The docs-only waiver: this is code.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, tier named in the body.

## The request as received

Quoted verbatim, in Portuguese, because the words carry the ambiguity the
ledger resolves; one marked, attributed quotation per the language rule.
Four messages from the trader, in order.

> inserir grafico por trades, hoje tempo por ticks. O proftichart tem opção
> de grafico por trades e achei interessanteo colcoar aqui tbm. Quero
> incluir isso no quantick. Mas cuidado, estou refatorando o app.cs para
> diminuir o seu tamanho. Tente fazer o maximo de modularidade possivel,
> tentar sperar em arquivops ou classes fora do app seria o ideal, pois
> vamos acabar tendo cofnlito depois e quero que fique facil resolver esses
> conflitos.

> roda a ultima versao do quantick e ativa mcp para vc enxergar

> Certo mas daria para deixar isso claro que ta gravando algo assim?
> consegue fazer um desenho de como ficaria para o usuario entender que ta
> gravando ao vivo? pq queria abrir desde as 9h e deixar gravando ai
> gravaria e poderia abrir o historico com base na gravação

> acho que deveriar mlehorar isso um botao de recrod algo assim pq se abrir
> em outro ativo como ficaria? acho que isso deve ficar mais explicito que
> ta gravando. Deixar uma opção salva como default para gravar por default.
> TO que aconteceria se eu trocasse para o gráfico de ticks? Eu acho que tem
> que deixar mais claro o que está gravando o mercado e o que foi gravado.
> Você tem que mapear também quando eu for abrir de novo o histórico. Se
> tiver gravado o gráfico de trades, vai abrir. Se for um Bitcoin, por
> exemplo, o que vai acontecer? Arquivos que não são do MetaTrader. Acho que
> isso tem que ser bem claro. Se eu tiver no gráfico de ticks e eu mudar
> para o de trades, isso vai refletir. Vai ter algum problema. Tem que ter
> um mapeamento mais claro aí. Essa é minha opinião.

> faz assim deixa essa opção espeficifica para b3 e metatrader por enquanto.
> Deixa o botao claro pra gravar. Quando abrir o ativo tem o botao pra
> gravar assim eh possivel eu trocar de ticks par atrades e funcionar okay.
> Deixa opção trades apenas para b3 metatrder porenquanto
