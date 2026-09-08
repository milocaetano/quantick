# Native indicator boundary evidence

Issue: https://github.com/milocaetano/quantick/issues/338 (campaign #330, Q6).

## Revision and evidence status

Origin: `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`, the verified campaign base before any source edit.
Final candidate identity is recorded after commit in `Q6-validation/final-head.json` and the coordinator's head-pinned checkpoint; a commit cannot embed its own SHA. The frozen source tree and ordered command receipts are recorded before commit. This dossier is not a claim that independent reviews, exact-head CI or campaign integration have passed.

Raw local evidence lives under `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q6-validation/`. The coordinator retains these artifacts with the campaign record; local paths are provenance, not a substitute for independent reproduction at the eventual candidate SHA.

The original complete mission is retained byte-for-byte as `mission-original.md`; its SHA-256 is `d86b6f9151e0d35bb2b9a2547016ee4f1ea3ee9c33ceab694a5d712fb82950bf`. `mission-write-receipt.json` records the clean source state, origin, timestamp and successful pre-edit guard build/app all-target check. The source issue was read live and its full exact text is quoted in the mission.

## Ownership change

Before this change, `QuantickApp::add_native_indicator` prepared `IndicatorSource::Native` and `SavedKind`, allocated directly on the focused pane, then wrote `IndicatorState::slot_kinds` itself. After it, `IndicatorSlots::attach_native` prepares the source and saved kind together, calls `IndicatorHost::add`, and records the full `TabSlot`. The root adapter resolves focus once, supplies the existing pane/context, applies returned layout intent and records one save intent.

`IndicatorHost` still has exactly `add(source) -> SlotId` and `remove(slot)`. `IndicatorSlots` still borrows the same five collections. Neither gains focus, layout storage, library IO or compilation/permission authority. `IndicatorAttachment` replaces the source-specific result name and serves script and native attachment without changing script behavior.

Known and unknown native IDs both reach the worker verbatim, with empty input values so existing catalog defaults apply. Native attachment remains human-owned. Unknown IDs retain visible error slots rather than becoming EMA. Native restore, library watching, settings, script validation and control permission paths are unchanged.

Rates: both edited production app methods are rare attachment operations. The scanner and baselines are repository tooling, outside the running application. No per-trade, per-depth or per-frame production path changes; no hot-path performance claim is needed.

## Exact contracts and independently measured budgets

`crates/guards/extension-shapes-baseline.txt` records normalized declaration visibility, field names/types/visibility in declaration order, current IndicatorSlots lifetime, and exact IndicatorHost method signatures. These are readable token contracts, not hashes or counts. The unchanged shapes comprise QuantickApp's 24 fields, ChartState's 14, IndicatorSlots' five and IndicatorHost's two methods; those counts describe the contract and do not enforce it.

`crates/guards/extension-roots-baseline.txt` has separate root caps. The origin scanner inventory and independent Python measurement agree on every one of 24 explicit declaration/implementation spans after sorting by `(path,start,end,root)`. Origin counts are QuantickApp **10,243**, ChartState **429**. The native adapter extraction lowers QuantickApp to **10,240**; ChartState remains **429**. The recorded combined permissions total is **10,669**, but each individual cap is enforced independently: shrinking ChartState cannot buy a QuantickApp deposit.

Measurement counts the union of production line spans per root per file, then sums files. Nested/overlapping root spans cannot double-charge a line. Contributions credit each line once; their sum equals the aggregate. Moving a complete block to another file preserves its charge. Splitting can add wrapper lines but cannot create headroom. No arbitrary registration-line exemption exists. Existing native toolbar/catalog registration requires no root registration change.

The independent measurement uses anchored origin-only formatted item headers and column-zero item terminators, with the existing top-level production classifier. It is a source-specific independent check, not the enforcing parser or a general Rust tool. Both unsorted inventories survive in `origin-independent-measurement.json` and `011-origin-inventory.log`. `crosscheck-origin.py`, `014-origin-crosscheck.log` and `origin-crosscheck.json` prove exact agreement. The origin source snapshot was copied while HEAD and tracked source were clean, before native source edits.

| Root | File under `crates/app/src/` | Origin span | Origin contribution | Candidate contribution |
| --- | --- | --- | ---: | ---: |
| QuantickApp | `app/chart_layers_wiring.rs` | 16-270 | 255 | 255 |
| QuantickApp | `app/control_host.rs` | 61-530 | 470 | 470 |
| QuantickApp | `app/demo_hooks.rs` | 67-1012 | 946 | 946 |
| QuantickApp | `app/drawing_chrome_wiring.rs` | 82-432 | 351 | 351 |
| QuantickApp | `app/drawing_input.rs` | 19-335 | 317 | 317 |
| QuantickApp | `app/frame.rs` | 55-742 | 688 | 688 |
| QuantickApp | `app/health.rs` | 45-349 | 305 | 305 |
| QuantickApp | `app/indicator_manager.rs` | 99-890 | 792 | 789 |
| QuantickApp | `app/launch_hooks.rs` | 43-830 | 788 | 788 |
| QuantickApp | `app/layout_wiring.rs` | 65-1352 | 1288 | 1288 |
| QuantickApp | `app/layout_wiring.rs` | 1354-1488 | 135 | 135 |
| QuantickApp | `app/layout_wiring.rs` | 1490-1565 | 76 | 76 |
| QuantickApp | `app/menu_bar.rs` | 124-778 | 655 | 655 |
| QuantickApp | `app/paper_wiring.rs` | 18-393 | 376 | 376 |
| QuantickApp | `app/replay_and_history.rs` | 51-538 | 488 | 488 |
| QuantickApp | `app/tabs.rs` | 105-439 | 335 | 335 |
| QuantickApp | `app/toolbar_wiring.rs` | 18-302 | 285 | 285 |
| QuantickApp | `app/workspace_restore.rs` | 16-156 | 141 | 141 |
| QuantickApp | `app/workspace_save.rs` | 30-1124 | 1095 | 1095 |
| QuantickApp | `app.rs` | 188-274 | 87 | 87 |
| QuantickApp | `app.rs` | 288-564 | 277 | 277 |
| QuantickApp | `app.rs` | 566-658 | 93 | 93 |
| ChartState | `state.rs` | 547-591 | 45 | 45 |
| ChartState | `state.rs` | 593-976 | 384 | 384 |

The adapter span changes from `indicator_manager.rs:99-890` to `99-887`; all other protected app spans remain unchanged.

## Supported formatted-source grammar

The dependency-free scanner walks all `.rs` files under `crates/app/src`, including ordinary unregistered sibling files and inline modules. It omits `tests/` and `target/` directories, and reuses `size::production_flags` exactly for top-level `#[cfg(test)]` items. It first lexes the complete file and checks that classifier markers/exclusion ends occur at valid lexical boundaries, so a marker or column-zero brace inside string data cannot silently suppress production scope. Indented test helpers stay conservatively counted as in the existing classifier.

Lexing supports UTF-8, LF/CRLF, whitespace, line comments, nested block comments, ordinary/byte/C strings, raw strings with matching hash delimiters, character/byte-character literals, lifetime/label identifiers, and balanced parentheses, brackets and braces. Strings remain single tokens; comments and inter-token whitespace disappear from the shape. Delimiter and unterminated lexical input errors name the source path/line. This is not a full Rust expression/type checker.

Protected declarations are named brace structs and the named IndicatorHost trait. QuantickApp and ChartState are non-generic. IndicatorSlots carries its current single `'a` lifetime. Visibility tokens (including restricted `pub(...)`) are normalized. Named field types use balanced token groups and generic angle brackets, with trailing commas; attributed/conditional protected fields and attributed protected declarations are unsupported diagnostics. Host members are ordinary semicolon-terminated method signatures, without attributes/default bodies. A missing, duplicate, tuple/unit or unsupported target is an error, never an empty contract.

Protected implementation items accept canonical root names or qualified paths ending in them, with optional leading `::`, for inherent implementations or non-generic path-named trait implementations. Whitespace and line breaks in these headers are immaterial. Generics, where clauses, references/associated self types, unsafe/default modifiers, attributes and other protected headers outside that subset receive an unsupported-scope diagnostic. `impl Trait` whose nearest enclosing delimiter is a parenthesis/bracket, or immediately follows a return arrow, is not an implementation item. An intervening brace block restores item context: explicit root implementations inside a parenthesized const expression or an array's const length are counted. Ordinary implementation braces are scanned, including nested explicit items, and the per-root union avoids double counting.

Direct root-renaming `use` aliases (including grouped imports) and type aliases involving a root are rejected. Current callback aliases remain supported as `fn(...) -> ...` and `Box<dyn Fn(...) -> ... + ...>`; they name callable values rather than the root. There is no recursive alias graph or type-resolution claim.

Macro token trees containing a protected identifier receive an unsupported protected-scope diagnostic, rather than being counted as if already expanded. Unrelated declaration macros and protected names inside literal data remain allowed. Production `include!` and explicit path-attributed modules receive unsupported-source-layout diagnostics because the canonical directory walk cannot certify the imported scope. Explicit out-of-line production `mod tests;` and `mod target;` declarations (including raw identifiers and visibility modifiers) also receive a diagnostic because they may import a skipped directory. Top-level `#[cfg(test)]` declarations retain the existing exclusion; inline modules remain scanned, including root implementations inside them. Source symlinks are unsupported. This is intentionally a canonical source-layout scanner, not a macro expander or filesystem module resolver.

Unreadable directories/files, invalid UTF-8, missing/duplicate baseline records, malformed counts and unsupported source forms are findings. The budget parser and tightening arithmetic reuse `ratchet::Policy`; zero per-root slack requires shrinking caps to follow the measured source. `--tighten` can only lower counts, never authorize growth. The new GUARDS entry drives whole checks, app-source/baseline `check_file`, CLI checks, reports/tightening and ordinary workspace tests. Existing size, cycle, context, graph, language and other guard policies/baselines are unchanged. The advisory blast-radius parser/report remains advisory and unchanged.

## Known limits

This guard is not complete semantic coupling analysis. It does not expand macros or procedural derives, resolve synthesized protected names, follow indirect helper authority, count root-carrying free functions, or reject net-neutral rewrites inside a root's existing line budget. A helper called from a tiny root method can grow outside the root total. These are documented limits, not negative tests advertised as rejected cases. Same-leaf-name protected declarations are rejected as ambiguous duplicates rather than semantically resolved.

The baseline is a reviewed source contract, not tamper-proof policy. Explicit contract/cap amendments remain visible review changes. No automatic SE2=5 claim follows from adding a scanner. Reassessment must use the unchanged quantick-score rubric at the actual integrated SHA together with realistic native and foreign-workspace evidence; the coordinator owns that reassessment and final integration proof.

## Fixtures and validation record

Compiled behavior fixtures:

- `independent_native_host_observes_exact_source_and_full_cleanup` drives the actual operation for `native.ema` and `native.nonesuch`, independently expects native source/empty values, explicit full target, native saved kind, human ownership and complete removal while retaining a colliding-slot survivor.
- `native_toolbar_operations_mirror_two_panes_and_save_once` drives the actual toolbar for both IDs, settles workers, checks both panes and saved layouts, and removes from the non-focused pane with one save intent.
- Existing native catalog, unknown native/saved unknown native, F1 script, script-validation/permission and foreign-workspace fixtures remain required at the final SHA.

Independent scratch/CLI fixtures in `crates/guards/tests/extension_boundary.rs` specify their own source/contract and literal root caps. They cover field/effect/visibility broadening, equal-count authority-type replacement, below-file-ceiling root deposits, qualified/multiline/trait implementations, sibling moves/deposits, cross-root budget transfer, malformed/missing/unsupported inputs and realistic native-owned extension growth in existing/new files. The equivalent root deposition must fail. Lexer/type-position/classifier unit fixtures live beside the scanner.

Completed early checks: `001` guard build and `002` app all-target arming passed before any edit; `003`-`006` guard edit batches passed; `008` callback repair guards passed; `010` scanner build and `011`/`012` inventories passed; `013` scanner library guards passed; `014` independent origin crosscheck passed. These do not substitute for final ordered checks, native fixtures, reviews or CI.

Cumulative early failure record is preserved in `counters.json`: origin scan `007` rejected an existing boxed callback (bounded grammar repair); `009` confused an `impl Into` parameter with a root item (type-position repair); an inline independent span comparison asserted on different path sort orders (explicit full-key comparison repair, no scanner/budget change). Original `007`/`009` raw logs remain. The inline comparison's original output was captured in the tool transcript but was not redirected to a separate raw file; both original input inventories remain. No reconstructed log is presented as original raw output. A fourth hygiene finding came from nine trailing spaces on empty blockquote lines in the mission archive. The archive alone was corrected, with its decoded quoted body checked byte-for-byte against the retained issue body; original archive bytes and staged diff are retained as `mission-archive-before-whitespace.md` and `pre-hygiene-staged.patch`. Git's CRLF warning on the two new baselines was handled by LF normalization without changing their data.

The complete registered suite passed in `016-registered-guards.log`: 155 library tests (six new scanner grammar tests), ten extension-boundary integration/scratch tests including CLI assertions, 19 ordinary registered/report tests and five CLI tests. Actual compiled operation tests passed in `017-indicator-operations.log` (seven), native catalog/default/error/restore tests in `018-native-catalog-and-errors.log` (nine), and foreign-workspace roundtrip/refusal and surrounding bundle tests in `019-foreign-workspace.log` (15). Each has a timestamped command/exit receipt. These are observed passes before the later skipped-directory diagnostic/fixture addition; that addition requires fresh validation.

The first frozen staged tree `cbabf5729d659f5e2e7b15ca65a978968632e04d` passed registered guards, fmt, clippy and build (`021` through `024`). Its workspace test `025` failed the existing observer capture timing gate: the selected batch median was 261 microseconds against 250. The original failed output, cumulative counters and frozen patch `cbab-before-scope-fix.patch` remain retained. No full-suite pass or commit resulted. A subsequent source review identified explicit out-of-line modules that could enter the skipped directories. The dedicated scratch/CLI fixture plants a hidden root implementation in each directory and checks rejection, test-only exclusion and actual accounting inside inline modules, without changing either root cap or the observer gate.

The scope correction subsequently passed the complete registered suite in `027-scope-registered-guards.log`, including the new scratch/CLI fixture (155 library, 11 extension integration/scratch, 19 registered/report and five CLI tests). The single authorized existing per-scope observer diagnostic passed in `028-observer-existing-diagnostic.log`; its largest scope median was `session.paper` at 31 microseconds. It is a measurement-only diagnostic and does not prove the cause of the earlier full-suite timing failure. Occurrence one and diagnostic attempt one remain recorded without resetting the cumulative failure history.

The mandatory ordered commit gate uses the retained `run-check.py` runner, which captures process output bytes and writes command/cwd/time/exit receipts. The coordinator receives the frozen tree plus every ordered fmt/clippy/build/workspace-test receipt before authorizing a commit. Only actual exit-zero receipts discharge those gates. Final-head independent reproduction, architecture/AI/source-first delivery reviews, exact-head CI and integration/score reassessment remain coordinator obligations before merge. No pending gate is waived by this dossier.

## First AI review correction

At candidate `44822b94b16d1f1cbf98c48ff71fea8b69936e46`, independent AI review found that any enclosing parenthesis or bracket suppressed explicit implementation accounting, even when an inner brace block contained a real item. The compiler accepted both compact and rustfmt-formatted parenthesized const and array const-length deposits, while the guard incorrectly accepted all four against unchanged independent caps. Matched controls passed and ordinary deposits failed. The actual report and resolvable finding are [PR #346's AI report](https://github.com/milocaetano/quantick/pull/346#issuecomment-5579173638) and [AI-Q6-1](https://github.com/milocaetano/quantick/pull/346#discussion_r3954301478). Original compiler, formatter and exact-binary guard receipts remain in `Q6-review-44822b94b16d/ai-diagnostic-fixtures/coordinator-observations/`; they are retained failures, not new validation passes.

Repair attempt one changes only the nearest-delimiter context decision, retains the existing parameter `impl Trait` and callback support, and adds independent literal-cap registered/CLI specimens for all four forms plus unchanged controls. No root cap, declaration contract, application runtime or observer timing gate changes. `Q6-validation/ai-nested-impl-repair/` records this repair's source identity, command results and frozen proof separately from the original commit's validation. Fresh checks, independent finding closure and exact-head CI remain required; this section records the correction's design, not a claim that those pending gates passed.

The corrected scanner passed the complete registered guard suite (`035`) and all original independently prepared reviewer specimens (`036`): both controls remained accepted, ordinary deposits remained rejected, and all four previously escaping nested deposits were charged and rejected. Raw variants reported seven lines against four; formatted variants reported eleven against six. The frozen tree `da6875d945556e47a5d95b344c5ad52ede2efe07` then passed ordered fmt/clippy/build (`037`-`039`), but default-harness workspace test `040` failed the existing `gateway_a_client_that_never_reads_does_not_stall_another` Success assertion, with 1,909 app tests passing, one failing and four ignored. Its actual non-Success outcome was not printed by that existing assertion, so no cause is claimed. Original output SHA-256 is `cffe0e0966d11a5b70a5d7b0768e35922044e4e7610c13325c38c83aab50eb8e`.

The test submits eight UI snapshot requests before running a frame, then makes a worker-side describe request. Gateway dispatch reserves one of eight per-gateway response permits before worker dispatch; UI responses release theirs after completion or timeout. Same-test permit contention is therefore a hypothesis, not the recorded outcome. The observer's separate budget measures elapsed capture time and can also see scheduler load. [Checkpoint 5579347919](https://github.com/milocaetano/quantick/issues/330#issuecomment-5579347919) authorizes exactly one subsequent full workspace validation with child-only `RUST_TEST_THREADS=1`, following fresh guards and ordered fmt/clippy/build. This reduces concurrent libtest harness load while retaining every ordinary test, all internal client/worker concurrency, assertions and timing thresholds. It cannot establish that a within-test scheduling race has been fixed. Results are recorded externally with the environment; unchanged default CI must still pass at the final SHA. The original failure, prior observer failure and all cumulative retry counts remain retained.
