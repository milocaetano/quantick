# F1 source review at 4c394f63

Step 0: direct inspection under the Codex compatibility mapping, medium requested scope for the valid high tier; no native effort parser was invoked, so native effort verification is not applicable. Zero confirmed correctness findings. All nine architecture dimensions and six AI dimensions inspected. This is an independent source review, not a score assessment.

Identity checked before and after inspection:

- HEAD: `4c394f63dc8fba5e68e221f3d680feb2e20bb8b3`
- Tree: `a8cebfcffb26dc8a384bcc840895c8b2854c17e2`
- Branch: `fix/feed-gap-honesty`
- Worktree: `C:/src/quantick-worktrees/fix-feed-gap-honesty`
- Base: `origin/campaign/outside-eight` at `05bc95ecfa36339be75411b251edf67cffa79992`
- Shared review key: `92028dcb3ed745ff31fe767e9a743932808a78b8`
- Clean tracked/untracked status at both checks. `gh pr list --head fix/feed-gap-honesty --state open` returned no PR during this inspection. Child issue is #474.
- Scope: entire declared-base diff, 27 files; no source authorship by this reviewer. No Cargo invocation, worktree edit, marker write, or GitHub publication performed.

## Finding

**F1-AR1 — Should fix — hardcoded-values — `crates/app/src/pane/axes_and_chrome.rs:692`.** The new `3.0 * SEAM_LABEL_PT` configures the gap caption's footer clearance inline. The architecture skill's hardcoded-values rule explicitly requires renderer tuning values to have a named owner and rationale. This belongs at module top, not in user configuration: define a value such as `GAP_CAPTION_BOTTOM_CLEARANCE_PX`, derived from the existing font size, with a comment explaining the three text rows of clearance. Use that constant in the existing expression. The `pane.height() / 2.0` midpoint is geometry and is not a separate finding. This finding does not claim that the chosen clearance is visually wrong. Attempts: 0 known for this finding; disposition: open. Coordinator retains campaign-wide repair counts.

No confirmed correctness or AI-dimension FAIL/WEAK findings. The historical visual findings F1-V1 and F1-V2 remain coordinator-owned; this review verifies their source changes and tests, not their pixel disposition.

## Architecture verdict

- Correctness: ordered continuity events precede the corresponding trade at `feed/src/binance.rs:175` and `feed-mt5/src/stream/connection.rs:489`; the MT5 host forwards the typed anomaly. High-watermark timestamps only advance with sequence IDs. Initial attachment has no assumed missing prefix; duplicates/backwards IDs have separate classification; MT5 session resets do not pretend to preserve sequence identity. The maximum-u64 guard avoids arithmetic overflow. No confirmed correctness finding remains.
- Docking: the provider-neutral `FeedContinuity` payload and existing `FeedEvent` channel are the seam. The next source can emit it without adding an app provider switch. This adds a domain event to an existing port, not a closed capability registry. No reverse crate edge or copied aggregator is introduced.
- Performance: per Binance trade and per MT5 wire tick, detection uses scalar constant work and no new wall-clock read. Anomalies add a bounded channel event. Gap retention moves at most 31 entries on eviction (`MAX_REMEMBERED_GAPS = 32`). Per frame, existing rendering performs bounded gap iteration with a binary bar search; the caption formatter replaces the existing coarse formatter. Observer snapshot/revision work adds four scalar counters per tab. No depth-update path changes. Inspected raw paired benchmark logs: Binance medians 528.59/531.20 ns; MT5 printed values 1564.36/1564.61 ns. These are narrow, noisy shared-host benchmarks, not end-to-end latency proof. No confirmed hot-path regression identified.
- Operability: `health.summary` and `feed.status` retain their registry entries (`app/src/control/health.rs:189`, `control/feed.rs:181`) and expose additive typed counters/bounds. The health revision includes the integrity state through `snapshot(app)` (`health.rs:228`). No new mutating action or permission path is introduced; repeated snapshot reads remain safe.
- Proof: production-host reconnect test `binance_automatic_reconnect_reports_loss_on_the_host_channel`; MT5 socket tests `sequence_loss_precedes_live_data_and_survives_quote_only_ticks`, `sequence_duplicates_and_backwards_ids_are_not_claimed_as_missing`, and `late_mt5_reorder_cannot_move_the_next_gap_backwards`; app drain/snapshot test `confirmed_feed_loss_reaches_gap_and_health_snapshots_without_changing_trades`; pure duration test `gap_captions_preserve_exact_millisecond_bounds`; real paint-shape test `short_gap_captions_stay_exact_and_clear_of_loading_and_footer_chrome`. Expected IDs, counts, timestamps and text are fixture literals, not generated from the implementation. Existing replay golden assertions remain intact, with newly unexpected anomalies explicitly rejected.
- Accumulation: 23 existing files modified and four added; the largest existing-file edit is the integration fixture `feed-mt5/tests/bridge_server.rs` (+132/-2). Largest existing production file by edited-line count is `feed-mt5/src/session.rs` (+72/-2), chiefly its test-only benchmark. No changes to `QuantickApp` or ratchet ceilings. Tab receives one feed integrity value consumed by the feed lifecycle and observer projection. New continuity implementation is 128 lines; its tests are separately gated. Existing production-containing files `feed/src/lib.rs` (+66/-4), `feed/src/metatrader.rs` (+55), and `app/src/control/feed.rs` (+15/-44) account for plumbing, tests and the relocation of feed-owned notice classification.
- Language: authored code, comments, documentation, branch and commit messages were read and are English; the preserved Portuguese user quotation is marked and attributed. PR prose was unavailable because no PR existed. Historical guard output named `guards-current.log` is a failure before the owner repaired UI-free growth, so it is not cited as PASS. The completed current-tree workspace test log includes the successful guard suites; this is distinct from my prose inspection.

## AI dimensions

1. Modular: PASS — `feed/src/continuity.rs:12` and `:43` own the data and cumulative reduction; provider translation remains below app. Blast radius is described above.
2. Decoupled: PASS — app consumes `FeedEvent::Continuity` at `app/src/tab/feed.rs:470`, not concrete Binance or MT5 tracker types. The adapters retain their established dependency direction.
3. AI-ready: PASS — additive typed projections at `app/src/control/health.rs:55` and `:302`, existing scope registration at `:189`, and feed bounds through `app/src/control/feed.rs:181`. This is observable feed evidence, not a new command requiring a request/error/actor abstraction.
4. Agent-tested: PASS — run `cargo test -p quantick-feed binance_automatic_reconnect_reports_loss_on_the_host_channel` or `cargo test -p quantick-feed-mt5 sequence_loss_precedes_live_data_and_survives_quote_only_ticks`. They exercise real local transports below app, including disconnect/skipped-ID error conditions. Deterministic pure cases cover count saturation, reversed timestamps, first attachment, duplicate IDs and reconnection unknown loss. Socket timeouts bound test termination; they do not determine expected financial output. This verdict concerns test design, not an assertion that this reviewer ran them.
5. Extensible: PASS — `feed/src/lib.rs:150` accepts provider-neutral continuity evidence. A future source with its own sequence vocabulary maps into the same payload without changing consumers. No additional provider, indicator, or mutation capability registry is introduced.
6. Scalable: PASS — bounded scalar tracker state (`feed/src/continuity.rs:73`), saturating counters (`:57`), and bounded retained gaps (`feed/src/lib.rs:452`, `app/src/tab/feed.rs:1005`). Trade/tick detection is independent of session length; no silent queue dropping is added. Rendering rates and observer work are documented above.

Top fix: `app/src/pane/axes_and_chrome.rs:692` — name the footer-clearance policy; closes F1-AR1. No AI dimension needs flipping.

## Evidence and remaining limits

Directly inspected the owner's source-tree identity and completed validation status/logs: current tree `a8cebfcf...` recorded successful ordered fmt, clippy, build and workspace test (17:04:09 to 17:08:48 America/Sao_Paulo, 2026-09-14). The test log completes through the final doc-test suites and includes the successful guard suites. Previous successful loops and benchmark artifacts remain historical. This is verification of the owner's execution, not a Cargo run by this reviewer. No final-head CI PASS is inferred from local output. Final-head CI, coordinator pixel observations, and subsequent code/evidence/archive delta review remain due before canonical publication. No review marker or PASS footer is issued with F1-AR1 open.

The replay-v1 format does not preserve these live anomaly events; the source/evidence explicitly identifies the separate history-export process and existing issue #226. This child changes neither replay trade ordering nor its completeness claims. No score rubric, scoring skill or measurement code is changed or assessed here.

ARCH-REVIEW: SHOULD-FIX
