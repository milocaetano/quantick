# Mission: paper-trading overhaul — durable history, performance-screen fixes, asset & replay filters, Cmd Trading gesture

- **Branch**: `feat/paper-trading-overhaul`
- **Worktree**: `C:\src\quantick-worktrees\feat-paper-trading-overhaul` (all writes happen there, never in the main checkout)
- **Done** = PR open with green CI and evidence in its body. Merge is NEVER part of the mission (another agent merges; wait for Camilo's word).

## Objective (user's request, condensed)

Paper trading must: save trade history by default to the user's documents folder (Windows Documents / Linux XDG documents), never lose old trades; fix the performance-screen bug where the 2d/3d period filter sometimes showed no old trades; filter by asset; separate replay results from real paper-trading results; get a criterious multi-agent bug hunt; and gain "Enable Cmd Trading" — hold a modifier and a dashed y-locked order line with a clickable label appears, safer than the right-click flow (which stays unchanged).

## Locked decisions (user-confirmed 2026-08-14)

- **Modifiers**: Shift = buy, Ctrl = sell, **by default — user-configurable** (other keys selectable).
- **Cmd Trading default**: enabled ("deixa por default configurado").
- **Reference image** (user-provided): dashed horizontal line at mouse Y, label at the **left end** of the line reading side + quantity ("Compra 100"), **×** affordance at the right end near the price axis. Line sits on the right side of the chart.
- **Semantics**: above current price → Buy **Stop** / below → Buy **Limit**; sell mirrored (above → Sell Limit, below → Sell Stop). Line tracks mouse Y only; X movement never moves it. Hover on label → highlight + pointer cursor. Click places the order.
- **Replay separation**: same performance screen; every trade recorded with its source (real vs replay); screen defaults to **Real only**, filter to view Replay or All.
- **App launches**: authorized for the whole mission for visual-qa/fps runs, under the protocol: `QUANTICK_DEFAULT_FEED=binance`, `QUANTICK_UI_STATE` → scratchpad, kill every launched instance when done (memory `no-agent-app-launches`).
- **Multi-agent bug hunt**: user explicitly asked for a team ("coloque equipe para procurar bugs") → Workflow orchestration is opted-in for that phase.
- **Period filter** (confirmed 2026-08-14): fix the bug AND add a free-text custom period input ("2d", "3d", "12h") alongside the five pills — there is no text field today; the pills are Today/7d/30d/90d/All.
- **History consolidation** (confirmed 2026-08-14): one-time import — copy, never delete originals — from the stored picker dir (`C:\Users\Camillo\OneDrive\Documents\BTCUSDT`) and any legacy cwd-relative `paper-trades` folders into the new Documents default, then clear the picker override. One home for history going forward.

## Acceptance criteria (evidence required for each)

1. **Documents-folder history**: default trade-history location is the user's documents dir (Windows `Documents`, Linux XDG documents, sensible fallback), in a `Quantick` subfolder, created on demand; path overridable via config/env/picker as today. New sessions append — prior history is never overwritten. Existing history consolidated per the locked decision (copy-import, originals untouched, picker override cleared). *Evidence: multi-session accumulation test + path-resolution test green + import test.*
2. **Period filter fixed**: root cause(s) of "old trades sometimes missing" verified in code, stated, and fixed — prime suspects from the map: scope-dependent anchor (:2393), Today at UTC midnight (:279), no auto-refresh on newly closed trades (:2382-2388). Plus the confirmed custom free-text period input ("2d"/"3d"/"12h") with tested parsing; invalid input rejected visibly. *Evidence: regression tests green + root-cause note in PR body.*
3. **Asset filter**: performance screen filters by symbol; history records the symbol per trade. *Evidence: test with ≥2 symbols green + UI reachable via ui-harness hook.*
4. **Replay vs real**: source recorded per trade; default view Real-only; filter Replay/All; CSV format change backward-compatible (old files still load, labeled honestly). *Evidence: test proving replay trades excluded from default view.*
5. **Team bug hunt**: Workflow over `crates/sim` + trading/performance UI with adversarial verification; every confirmed bug fixed with a regression test or explicitly deferred in the PR body. *Evidence: workflow summary + tests.*
6. **Cmd Trading**: toggle in the trading window (default on); modifier-held dashed y-locked line with clickable label per the locked decisions above; label shows side + configured quantity; releasing the modifier hides the preview; click places the order into `quantick-sim` **through the same order-creation path as the right-click flow** (one code path); right-click flow unchanged. Line length (fixed px vs % of chart width) and exact label wording decided in trader-ux-review and recorded here + in the PR. *Evidence: gesture state-machine unit tests + visual-qa matrix (buy/sell × above/below × hover × placed × toggle-off).*
7. **Performance declared and measured**: every touched path classified per-trade / per-depth / per-frame / rare as part of the plan; the overlay is per-frame → `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs a `main` control run, flat or better, numbers in the PR body.

## Standard gates (kind: code change + hot path + user-visible)

- Four checks green after rebasing on latest `main`: `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo build --workspace` · `cargo test --workspace`.
- ui-harness: every new/changed surface reachable by env hook, hooks added in the same change.
- visual-qa: all surfaces PASS or defects explicitly accepted.
- trader-ux-review: no unresolved Blocker (this review also settles line length + label wording).
- Sim/engine territory is test-first: fixture + expected output before code; determinism guarded.
- arch-review over `git diff main...HEAD`; every Blocker/Should-fix resolved or deferred in the PR body. Before `gh pr create`: `git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"`, then archive this file to `.claude/GOAL-archive-paper-trading-overhaul.md`.
- PR opened at the end; **no merge, ever** — Camilo's side handles merging.

## Gate evidence (2026-08-14, session run)

- **Performance (gate 7)**: per-frame overlay measured — `frame_cpu_ms` avg **3.16 (main control) vs 3.04 (branch with the preview forced on)**, max 3.65 vs 3.27, dense binance BTCUSDT tape + book + bubbles + strip + paper demo + report open, identical env both sides. fps pinned at 19 on both (Windows session locked → compositor throttle; environmental, identical for both runs). Flat-or-better: **met**.
- **Trader-ux-review (gate)**: 3 findings, all fixed in commit `79e3445` — Source pill "Both" (two "All" pills a hand-width apart), typed period applies on blur, cmd block states its own key instructions. No Blocker open. Line length decision: fraction (20%) with 120–320 px clamp, upheld. Label text: SIDE kind qty (price on the gutter chip, where every price reads). Ledger replay-marking deferred as Consider (PR body).
- **visual-qa (gate)**: screenshots **BLOCKED by environment** — Windows session locked during the run; PrintWindow captures white, CopyFromScreen photographs the lock backdrop (evidence: shot-copyscreen.png solid blue). Substitute evidence recorded: off-screen render proof test (`the_cmd_preview_paints_line_label_and_price_chip` — dashed segments, side label, price chip in real egui shapes) + 46 paper_trading unit tests + app health under live tape. Screenshots to be retaken in the first unlocked session; declared in the PR body as an explicitly accepted defect.
- **ui-harness (gate)**: registry updated in the same change (`QUANTICK_PAPER_STATE`, dock/report/demo rows moved into the main table, new `QUANTICK_CMD_PREVIEW=buy|sell` hook section).
- **Bug hunt (front 5)**: workflow `paper-trading-bug-hunt` (4 lenses → dedup → 2 adversarial skeptics per finding) launched; results pending.

## Open items

- Trader-ux-review to decide: dashed-line length (fixed px vs % of chart width), exact label text/language, × cancel affordance details. **Decided — see gate evidence.**
- Design: replay-rerun duplication fix (same replay run appends the same trades to the same venue-time-named file — admitted defect at paper_trading.rs:4320).
- Provenance recording (real vs replay) is *data*, not a UI affordance — it does not violate the "gate on FeedCapabilities, never on is-replay" rule, but the journal must learn the session source through clean wiring (note for arch-review).

## Architecture map (exploration, 2026-08-14)

**History persistence**
- Format: `crates/sim/src/history.rs` — pure strings, no fs. v2 = 12 cols (`HEADER` :44); v1 (8 cols) still parses, new fields `None`. `write_header` :85, `write_trade` :93, `parse` :126.
- Writer: `crates/app/src/paper_trading.rs:2775` `fn journal`, per closed trade (`SimEvent::Closed` :2758; also forced flatten :590). Path `<trades_dir>/<sanitized SYMBOL>/<YYYYMMDD-HHMMSS>.csv`, name from **venue time** of first closed trade (:2779-2786). Append-only, header only if file new (:2788-2798); write failure warns once (:2800-2816).
- `resolve_trades_dir` :432 order: env `QUANTICK_TRADES_DIR` (:33) > picker choice stored in `paper-state.toml` > `[paper] trades_dir` (`config.rs:503`) > default `paper-trades` **cwd-relative** (:42).
- **Root cause of "lost history"**: both the default dir AND `paper-state.toml` itself (`ui_state.rs:509` `default_path`) are cwd-relative → a different launch cwd silently switches history and settings. User's stored picker choice today: `C:\Users\Camillo\OneDrive\Documents\BTCUSDT`.
- **Known defect** (:4320): replay re-run → same venue-time filename → append duplicates the same trades in one file.

**Performance screen**
- `draw_report_window` paper_trading.rs:2414; filters `draw_report_filters` :2531 (symbol combo :2544 fed by `list_symbol_folders` :4444 with "All symbols"; period pills :2569; manual refresh :2586). Disk load `load_history(dir, symbol, exclude)` :3495 (sorted :3557); `reload_report` :2373; unreadable rows counted+shown :2490-2500.
- **No text period input**: `ReportPeriod` :235 = Today/7d/30d/90d/All. Apply in `ensure_report_view` :2381; cutoffs `cutoff_ms` :276.
- Bug candidates: (1) **anchor = newest trade of the loaded scope** (:2393) → "All symbols"+30d hides an older symbol entirely; narrowing the combo brings it back — matches the complaint; (2) Today cuts at UTC midnight :279 while ledger renders local tz :2123; (3) report never auto-refreshes on newly closed trades — freshness guard :2382-2388 only reacts to period change.

**Trading tab & order paths**
- Dock tab: `draw_trading_tab` :1483; order entry :1778 (**"Enable Cmd Trading" toggle goes here**; `pill_toggle` helper :3931); pending orders :1947.
- Right-click: `pane.rs:1400` → `context_trade_actions` paper_trading.rs:1407 → `market` :1430/:615 or `place_resting(side, kind, price)` :1457 → :1370 → `Command::PlaceLimit`/`PlaceStop` :1379-1390 → `sim.apply` :1394 → `handle_events` :1398. Armed path: `armed` :1900 → `handle_chart_input` :1110-1118 → `place_armed` :1362 → same `place_resting`. **Cmd Trading must route through `place_resting`** (one order-creation path).
- Input call site `pane.rs:2970-2979`, gated `chrome.paper_owns_input && !tool_armed`. `handle_chart_input` :1075 is **modifier-blind and consumes the primary press** — the gesture must resolve inside it; `ChartInput` :350 needs a modifiers field.

**Modifiers already in use**
- Shift+pointer only with drawing context (`pane.rs:2259` Constrain::Level; :3187 handle drag). Alt+click z-order :3057. **Ctrl pointer: free everywhere.** Keyboard Shift+B/S/R/F/X trading hotkeys (app.rs:3637) — Shift already "means trading". Guards: suppress gesture when a drawing tool is armed or a handle drag is live. egui turns Shift+wheel into horizontal scroll (pane.rs:3323). Headless modifier pattern: `QUANTICK_DRAWING_CONSTRAIN` (app.rs:5592) via `ParkedHand.constrain` (pane.rs:88) — mirror for the gesture's harness hook.

**Replay vs live**
- `FeedCapabilities` (config.rs:127) has no replay flag (by design). `Tab::replay: Option<ReplayLink>` (tab.rs:207, :341, :893) knows; statusbar amber "replay" (statusbar.rs:38-66). **History records no provenance** — needs a v3 column (backward-compatible) + the rerun-duplication fix.

**Config/paths**
- `config::load()` config.rs:1091: `QUANTICK_CONFIG` > `./quantick.toml` > embedded default (crates/app/config/feeds.toml never read at runtime). **No `dirs`/`directories` crate in the workspace** (Cargo.lock clean) — Documents default = new small dependency. Single plug point: `chosen_trades_dir` paper_trading.rs:4437.

**Symbols**
- Symbol lives in folder name + `# symbol=` header (history.rs:86; load fallback to folder :3541), not per-row. `sanitize_symbol` :4275. **Symbol filter already exists** (combo + ledger `ReportScope` :225/:2149) → front 3 = verify with ≥2 assets and fix interactions (anchor bug), not build from scratch.

**ui-harness hooks (existing)**
- `QUANTICK_DOCK_TAB` (app.rs:1248: `trading`, `trades`, `l2`, `bubbles`, `session`), `QUANTICK_PAPER_REPORT_AUTOSTART` (app.rs:1271 → paper_trading.rs:2357), `QUANTICK_PAPER_DEMO` (paper_trading.rs:40/:463 — deterministic command sequence, trades really journal), `QUANTICK_TRADES_DIR` (:33), `QUANTICK_PAPER_STATE` (paper_state.rs:22). New hooks required for the cmd-trading overlay states.
