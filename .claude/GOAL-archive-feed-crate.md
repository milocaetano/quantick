# Mission: `quantick-feed` — the feed contract and its venue adapters leave the UI crate

Extract `crates/app/src/feed/` — the `FeedEvent` / `FeedCommand` /
`FeedCapabilities` / `FeedHandle` contract and the Binance, Hyperliquid,
MetaTrader, bridge, replay and stall adapters — out of `crates/app` into a new
crate `quantick-feed` that sits above the three venue crates and below `app`.

**Why it matters.** `feed/` is the largest egui-free island left in
`crates/app` after the order-flow extraction. It is the port every venue
implements and the port the rule *replay is a source, not a chart mode*
depends on, yet it is defined in the leaf of the dependency graph — so no crate
below `app` can be tested against it, and issue #273 (a language-neutral native
adapter protocol) has nowhere to dock.

**Tier:** `medium`. A pure move, but far past the `small` ceiling — eight
files, 8,052 lines, 29 consumer files, a new workspace member and four
registration surfaces. Nothing here is a judgement call about behaviour, so it
does not earn `high`; the size alone means `delivery-review` runs.

## Request ledger

Atomic asks, from the mission brief `C:\src\mission-feed-crate.md` and the
`/mission` invocation. Numbers are stable for the life of the mission.

| # | Ask |
| --- | --- |
| R1 | Create a new crate `crates/feed`, package `quantick-feed`, depending on `engine`, `orderbook`, `replay`, `feed-binance`, `feed-hyperliquid`, `feed-mt5` plus `tokio`, `crossbeam-channel`, `serde`, `serde_json`, `tracing`, `dirs`, all from `[workspace.dependencies]`. Never `egui`, never `pine`. |
| R2 | Its README states it is the feed *host* — the level of the graph that owns runtimes — and that everything below it stays clock-free. |
| R3 | Move the eight files (`mod.rs`, `binance.rs`, `hyperliquid.rs`, `metatrader.rs`, `mt5_bridge.rs`, `ohlcv_plan.rs`, `replay.rs`, `stall.rs`) into the new crate, with their inline tests. |
| R4 | `FeedEvent`, `FeedCommand`, `FeedCapabilities`, `FeedHandle`, `FeedSource`, `FeedNotice`, `FeedGap`, `FeedLatency` become the crate's public surface. |
| R5 | The feed-owned config types travel: `FeedCapabilities`, `ProviderKind`, `MetaTraderSettings`, `Mt5SideSource`, `Mt5Endpoint` move into the new crate. |
| R6 | `crates/app/src/config.rs` re-exports those types so its own callers do not change, and shrinks by the moved definitions. |
| R7 | `spawn` takes the slice of config it reads, not `&AppConfig`. |
| R8 | Cut the clock edge: `crate::metrics::wall_clock_ms` moves with the crate or is passed in. |
| R9 | Cut the documents-folder edge: the MT5 clock-cache location becomes a parameter the app hands in, never a reach into `crate::paper_home::shelf_dir()`. |
| R10 | The doc-comment links to `crate::tab` and `crate::theme` become plain prose or intra-crate links. |
| R11 | Consumers change `use` paths only — no behaviour edits in the ~29 app files that name `crate::feed`. |
| R12 | Register the crate: root `Cargo.toml` members, `crates/app/Cargo.toml`. |
| R13 | Update the `AGENTS.md` graph (`app --> feed`, `feed --> feeds`, `feed --> replay`, `feed --> engine`) and *The map* table row. |
| R14 | Update `crates/pine/tests/workspace_deps.rs` expected edges. |
| R15 | Run `--tighten` on the size baseline for whatever shrank. |
| R16 | Respect the parallel branch `fix/tests-own-their-scratch`: whichever lands first, the PR body says which, and the two PID-keyed test scratch helpers (`replay.rs`, `mt5_bridge.rs` `TempTree`) are handled accordingly. |
| R17 | Read the brief in full and build this ledger from it — *so that* the mission is graded against the brief's own claims, each re-verified rather than trusted. |

**Ledger corrections made while verifying (R17).** The brief's evidence table
was measured at `4e47ca4`; this branch is cut from `d551813`. Three claims
changed:

- Claim 1 said 8,044 lines; the true count is **8,052** (`binance.rs` 549,
  `hyperliquid.rs` 508, `metatrader.rs` 2,786, `mod.rs` 814, `mt5_bridge.rs`
  836, `ohlcv_plan.rs` 308, `replay.rs` 1,623, `stall.rs` 628). PR #287 added
  the eight hook-declaration lines.
- Claim 1 said zero `egui` uses. True of code; `mod.rs:1` mentions egui in a
  module doc comment, which the move rewrites.
- Claim 2 said PR #287 "rewrites `CLAUDE.md`'s headless clause to admit exactly
  that". **It does not.** `CLAUDE.md:29` still carves out only "the three
  `feed-*` crates". A fourth non-headless crate below `app` therefore needs
  that clause amended — see `R18`.

| # | Ask |
| --- | --- |
| R18 | Amend `CLAUDE.md`'s headless clause so it admits `quantick-feed` as a runtime-owning layer below `app`, since the brief's claim that #287 already did so is false. |
| R19 | Keep the launch-hook registry whole across the move: the four `declare_hooks!` sites in `binance.rs`, `metatrader.rs`, `mod.rs` and `stall.rs` (added by #287) must still reach the generated hook registry and its parity guard. |

## Decisions taken by the trader

- **D1 — The hook registry stays app's; `quantick-feed` exports its `HOOKS`.**
  The new crate defines its own `HookSpec` / `declare_hooks!` (or makes them
  public), `app`'s `OWNERS` table keeps four rows pointing at
  `crates/feed/src/...`, and the parity guard's source scan widens to the feed
  crate. The generated registry keeps all its rows and `app` stays the one
  place that knows every hook. Rejected: moving the hooks module to a shared
  crate (a second refactor riding on this one), and dropping the four
  declarations (re-opens the drift #287 closed).
- **D2 — `wall_clock_ms` moves into `quantick-feed`; `crate::metrics`
  re-exports it.** Exactly the pattern `crates/app/src/metrics.rs:102` already
  uses for `feed_lag_ms` from `quantick-orderflow`, so no app call site
  changes and the clock lives in the layer that owns runtimes. Rejected:
  passing it in as a parameter (churn on internal call paths for two default
  values), and a duplicated copy (two owners of one clock).

## Assumptions

- **S1** — The config slice `spawn` takes (R7) is `&MetaTraderSettings`: it is
  the only field of `AppConfig` the function reads (`feed/mod.rs:652-671`).
  `spawn_live` takes the same. A conventional narrowing the code answers in a
  minute of reading.
- **S2** — The crate directory is `crates/feed` and the package
  `quantick-feed`, as the brief states; the existing `crates/feed-*` venue
  crates are unaffected.
- **S3** — The MT5 clock-cache parameter (R9) is threaded from the app's
  existing `paper_home::shelf_dir()` call at the one site that spawns the
  bridge, so runtime behaviour is byte-identical. *Wanted to ask* — the budget
  went to D1 and D2; the reading taken is "same directory as today, chosen by
  the caller".
- **S4** — `ohlcv_plan`'s items stay crate-private to the new crate unless a
  consumer outside it needs them; only the eight types in R4 are promised
  public. *Wanted to ask* — reversible in one edit.
- **S5** — Tests travel inline with their files rather than being split into
  `tests/`; the brief says "tests travel with the files", and the size ratchet
  counts production lines only.
- **S7** — The four `MetaTraderSettings::endpoint_for` unit tests in
  `config.rs` (and their `listening` helper) travel with the type, since they
  exercise the moved type alone. Every TOML-loading test stays in `app`, where
  `AppConfig` stays. *Wanted to ask* — "the types travel" is read as including
  the tests that are only about them.
- **S6** — Where `fix/tests-own-their-scratch` has not merged by the time this
  branch is ready, this branch carries the PID-keyed helpers as they stand on
  `origin/main` and the PR body says so (R16).

## Acceptance criteria

- [ ] **A1** — A crate `crates/feed` exists with package name `quantick-feed`,
      its `[dependencies]` drawn from `[workspace.dependencies]`, and no
      `egui`, `eframe` or `quantick-pine` among them.
      *Evidence:* `crates/feed/Cargo.toml` quoted in the PR body, plus
      `grep -rnE 'egui|eframe|quantick_pine' crates/feed/src` returning nothing.
      → PR body. *(R1)*
- [ ] **A2** — `crates/feed/README.md` states the crate is the feed *host*, the
      level of the graph that owns runtimes, and that everything below it stays
      clock-free.
      *Evidence:* the file, quoted in the PR body. → `crates/feed/README.md`. *(R2)*
- [ ] **A3** — All eight files live under `crates/feed/src/` and none remains
      under `crates/app/src/feed/`; `git diff --stat` shows them as renames.
      *Evidence:* `git diff --stat -M origin/main...HEAD` in the PR body. *(R3)*
- [ ] **A4** — `FeedEvent`, `FeedCommand`, `FeedCapabilities`, `FeedHandle`,
      `FeedSource`, `FeedNotice`, `FeedGap`, `FeedLatency` are public from
      `quantick_feed`.
      *Evidence:* the `pub use` / `pub` declarations quoted in the PR body. *(R4)*
- [ ] **A5** — `cargo test -p quantick-feed` is green **without building
      `quantick-app`**, and runs at least the **84** test functions the eight
      files held on `origin/main` (`binance` 1, `hyperliquid` 2, `metatrader`
      20, `mod` 6, `mt5_bridge` 14, `ohlcv_plan` 10, `replay` 19, `stall` 12).
      An earlier draft of this line said 65; that count matched only `#[test]`
      and missed every `#[tokio::test]`, which is most of the adapter suites.
      *Evidence:* both counts — the baseline 84 above and the run's own summary
      line — in the PR body. *(R3, R5)*
- [ ] **A6** — `FeedCapabilities`, `ProviderKind`, `MetaTraderSettings`,
      `Mt5SideSource` and `Mt5Endpoint` are defined in `quantick-feed` and
      re-exported from `crates/app/src/config.rs`; `config.rs` shrinks by the
      moved definitions and **no other app file changes because of the config
      move**.
      *Evidence:* the `config.rs` hunk plus `git diff --stat`. → PR body. *(R5, R6)*
- [ ] **A7** — `spawn` and `spawn_live` take `&MetaTraderSettings`, not
      `&AppConfig`.
      *Evidence:* the signature hunk in the PR body. *(R7)*
- [ ] **A8** — `grep -rn 'wall_clock_ms' crates/feed/src` resolves inside the
      crate, and `crate::metrics` re-exports it so no app call site changed.
      *Evidence:* the `metrics.rs` hunk and a `git diff --stat` showing the ~30
      `wall_clock_ms` call sites untouched. → PR body. *(R8, D2)*
- [ ] **A9** — `grep -rn 'paper_home\|shelf_dir' crates/feed/src` returns
      nothing; the MT5 clock-cache directory arrives as a parameter, and the
      one app call site that now supplies it is the sole non-`use` consumer
      change.
      *Evidence:* that hunk, quoted. → PR body. *(R9)*
- [ ] **A10** — `grep -rn 'crate::tab\|crate::theme' crates/feed/src` returns
      nothing.
      *Evidence:* the command's output in the PR body. *(R10)*
- [ ] **A11** — Every consumer file's diff is `use`/path lines only, with a
      closed and enumerated set of exceptions — no consumer changes for any
      other reason:
      (a) the **nine call sites** that now hand in what used to be reached for
      — seven `spawn`/`spawn_live` calls passing `&config.metatrader` and the
      clock-cache directory, and two `clock_cache_path` calls passing the
      shelf. The brief assumed one such site; there are nine, and that is a
      ledger correction, not a scope change (R7 and R9 are inversions, and an
      inverted parameter is supplied by every caller).
      (b) `crates/app/src/hooks.rs`, which loses its `HookSpec` /
      `declare_hooks!` definitions to `quantick-feed` and re-exports them —
      D1 cannot typecheck otherwise, since an `OWNERS` row holding a feed
      module's slice needs one `HookSpec` type and `feed` cannot depend on
      `app` to get it — and whose four `OWNERS` rows now name the new paths.
      (c) `crates/app/src/metrics.rs` and `crates/app/src/config.rs`, which are
      the R8 and R6 re-export sites themselves rather than consumers of them.
      (d) `crates/app/src/main.rs`, where `mod feed;` becomes
      `use quantick_feed as feed;`.
      *Evidence:* `git diff --stat` plus a per-file scan of the consumer hunks
      showing every other changed line names a feed path. → PR body.
      *(R11, R19, D1)*
- [ ] **A12** — Root `Cargo.toml` lists `crates/feed` in `members`, and
      `crates/app/Cargo.toml` depends on `quantick-feed`.
      *Evidence:* both hunks in the PR body. *(R12)*
- [ ] **A13** — `AGENTS.md` *The map* gains a `quantick-feed` row and the
      Mermaid graph gains `app --> feed`, `feed --> feeds`, `feed --> replay`,
      `feed --> engine`.
      *Evidence:* the `AGENTS.md` hunk. *(R13)*
- [ ] **A14** — `cargo test -p quantick-pine workspace_deps` is green with the
      new edges declared.
      *Evidence:* the test output in the PR body. *(R14)*
- [ ] **A15** — `cargo test -p quantick-guards` is green, and the size baseline
      reflects what shrank (`--tighten` run, `!budget` unchanged or lower).
      *Evidence:* the guards output and the `size-baseline.txt` diff. *(R15)*
- [ ] **A16** — The PR body states whether `fix/tests-own-their-scratch` landed
      first, and which version of the two PID-keyed scratch helpers this branch
      carries.
      *Evidence:* that paragraph of the PR body. *(R16)*
- [ ] **A17** — `CLAUDE.md`'s headless clause names `quantick-feed` as a
      runtime-owning layer below `app`, so the shipped graph and the stated
      invariant agree.
      *Evidence:* the `CLAUDE.md` hunk. *(R18, R17)*
- [ ] **A18** — The four `declare_hooks!` sites survive: `app`'s `OWNERS` names
      them at their new `crates/feed/src/...` paths, the generated hook
      registry still lists `QUANTICK_BOOK_DEPTH`,
      `QUANTICK_FAKE_LATENCY_SPLIT`, `QUANTICK_BACKFILL`, `QUANTICK_FEED_GAP`
      and `QUANTICK_FEED_STALL`, and the parity guard scans the feed crate.
      *Evidence:* `cargo test -p quantick-guards` green plus the registry diff
      showing those five rows intact. → PR body. *(R19, D1)*

### Injected gates

- [ ] **G1** — Every artifact this branch authors is in English — code,
      comments, doc comments, tests, README, `AGENTS.md` and `CLAUDE.md` prose,
      commit messages, PR title and body.
- [ ] **G2** — The four checks are green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind a `||`.
      *Evidence:* the four exit codes / outputs, one per line, in the PR body.
- [ ] **G3** — Performance impact declared. This is a pure move: every touched
      path keeps its rate class (per-trade for the adapter loops, per-frame for
      the drain, rare for `spawn`), and no call is added or removed.
      *Evidence:* that classification stated in the PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, with every
      Blocker and Should-fix resolved, or deferred with its severity recorded
      in the PR body.
      *Evidence:* the verdict and the deferral list. → PR body.
- [ ] **G5** — `new-extension`'s crate rules honoured: the port is named, the
      registrations are registration-only edits, and defaults preserve today's
      behaviour exactly (this is a move, so runtime behaviour is unchanged);
      blast radius stated in the PR body.
      *Evidence:* that paragraph of the PR body.

### Not applicable, and why

- **Hot-path evidence (fps / bench numbers).** The change moves code between
  crates without altering a single expression on a per-trade or per-frame
  path, so there is no behavioural delta to measure. `G3`'s declaration stands
  in for it; if any adapter body changes during the move, this stops being
  N/A and a measurement is owed.
- **`visual-qa`, `trader-ux-review`, `ui-harness` new-surface hooks.** No
  user-visible surface changes. The four *existing* launch hooks that move with
  their files are covered by `A18`, which is a registry-integrity criterion,
  not a UI one. No new hook is added.
- **Test-first / golden-determinism discipline.** Not engine territory and no
  new behaviour: the 65 existing tests travel unchanged and are themselves the
  regression net (`A5`).
- **The docs/skills waiver.** Not claimed. This branch ships Rust, so it takes
  the full shape pass; `AGENTS.md` and `CLAUDE.md` ride along with it.

### Closing steps

- [ ] **C1** — `delivery-review` returns PASS.
- [ ] **C2** — The PR is open, its body carries the evidence above, and it
      names the tier `medium`.

## The request as received

Quoted verbatim and untranslated: this is the marked, attributed quotation
`CLAUDE.md`'s English rule exempts, and it is the only source of truth against
which the ledger above can be audited. Every other line of this file is
English.

> medium refactor/feed-crate — extract crates/app/src/feed/ (the FeedEvent /
> FeedCommand / FeedCapabilities / FeedHandle contract and the Binance,
> Hyperliquid, MetaTrader, bridge, replay and stall adapters, 8,044 egui-free
> lines) out of crates/app into a new crate `quantick-feed` that sits above the
> three venue crates and below app. The feed-related config types travel with
> it; the one reach into app's documents folder is inverted into a parameter;
> consumers change `use` paths only; tests travel with the files; graph,
> AGENTS.md map, workspace_deps guard and size baseline are updated. Read
> C:\src\mission-feed-crate.md in full before anything else and build the
> request ledger from it.

The brief at `C:\src\mission-feed-crate.md` is the invocation's cited source;
its scope, acceptance criteria, out-of-scope list and parallel-work notes are
folded into `R1`–`R19` above, with the three corrections recorded under the
ledger.
