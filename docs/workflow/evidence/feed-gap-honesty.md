# Feed continuity evidence (F1, campaign #472)

Initial base: `eb7bb039`, `origin/campaign/outside-eight`. Child issue: #474.
Current publication base: `05bc95ecfa36339be75411b251edf67cffa79992`.

The Binance transport already retained its continuity tracker across automatic
reconnects. Its returned anomalies were discarded, leaving only tracing. The
host now retains the aggregate-ID watermark and publishes ordered continuity
events before subsequent live trades. MT5 likewise publishes its existing
sequence classification before mapping the affected tick, including quote-only
ticks. Counts mean source messages, not inferred executed trades.

The chart retains short and equal-timestamp confirmed gaps and cumulative
integrity counters. `feed.status.tape_gaps` supplies bounded market-time bounds;
`health.summary.tabs[].feed_integrity` supplies anomaly, known missing-message,
unknown-loss and non-monotonic counts. Counters survive bounded gap eviction.
The reset path clears both with their market timeline. Unknown MT5 reconnect
loss carries no guessed count because its synthetic sequence resets per session.

Per-source processing is constant work per tick/trade, with scalar state and
no new clock read. Anomaly events use the existing bounded ordered channels;
only anomaly arrival invokes bounded gap retention. Observer snapshots are
read-time work and extend the published contract additively. Existing notice
classification moved into the feed owner to pay for the UI-free projection.
No root cap, ratchet, guard, financial rule or scoring file changes.

Recording limitation: the existing replay download is a separate broker-history
export process (`feed/src/replay_download.rs`, `tools/mt5/export_session.py`),
not a recorder consuming the live FeedEvent stream. The replay-v1 tick file does
not preserve these live anomaly events; issue #226 tracks that existing format
limitation. This change does not claim recording completeness or alter replay
trade ordering, matching, timestamps, or the shared bar engine.

Preflight: coordinator source-first completeness PASS on map revision 1,
SHA256 `AAB084A01543D92B2BC1C2448BD49A75D2D869180A6B00BFA588AAA0616EAEBF`,
before production edits. Guards built and baseline feed-crate checks passed
before any edit. App pre-edit check started before app edits but compiled the
already-added FeedEvent variant and failed E0004 at its unfinished consumer.
Coordinator explicitly accepted this bounded chronology exception under the
user's delegated defaults; it is not a pristine app baseline PASS. After app
wiring, `cargo check -p quantick-app --all-targets` passed (45.73 seconds).

Targeted verification before the full ordered loop:

- Feed library: 139 passed, zero failed, one manual benchmark ignored.
  The Binance reconnect fixture uses the production host loop with loopback
  REST/WebSocket transport inputs, not a test recreation of host forwarding.
- MT5 bridge socket fixtures: 30 passed after adding truthful non-monotonic
  expectations to the pre-existing unsolicited-page fixture; both existing
  replay golden suites passed. Their trade/bar assertions were retained.
- App schema generation: one passed, zero failed (16m39s rebuild). Only the
  feed-status description and additive health-integrity schema changed.
- App confirmed-loss snapshot/reset fixture: one passed, zero failed (0.13s).
- Repository guards passed after moving existing notice classification and
  recovery wire names into their headless feed owner. No ceiling changed.
- `cargo deny check bans licenses`: exit zero, `bans ok, licenses ok`.
  Cargo.lock SHA256:
  `8826F73F0589B91F318C40316C18C1925B926BD7A03C0DDB393CA0A84EC795FA`.

Performance chronology, not acceptance evidence: the first shared-host Binance
sample with an extra asynchronous forwarding wrapper measured 918.13 ns baseline
versus 972.15 ns candidate. That wrapper was removed. A short synchronous sample
under concurrent builds still measured 806.74 ns versus 856.46 ns with widely
overlapping raw ranges. Pinning one core increased scheduling noise further.
All observations are retained; none is relabelled a performance PASS. The final
benchmark uses one million prints per sample and 20 alternating-order pairs,
retaining raw paired samples. The MT5
initial sample measured 1564.36 ns baseline versus 1564.61 ns candidate.

The enlarged Binance run, with the campaign build slot reserved, completed in
22.11 seconds: baseline median 528.5922 ns, candidate median 531.19605 ns,
approximately +0.49%. External Cargo processes remained present; no rustc was
observed during the process snapshot. This is not a claim of an exclusive
machine or an end-to-end exchange latency benchmark. It measures the affected
decode/tracker/host-channel path with the same inputs on both sides.

The executable SHA256 was
`7F2A5C72BAB05748DF46FFE52BA26BDC8A478E3BC2631D106F00617807A961BA`.
Its benchmark source SHA256 was
`FBA471A293B35E8BE0307602308518D36D2D693A4D396E540409773521FB8804`.
Subsequent changes only fixed the HTTP test fixture's bounded header read;
the benchmark and production hot path stayed unchanged.

Raw paired samples, nanoseconds per print, in execution-round order:

```text
baseline = [522.6528,510.8889,532.9741,522.045,511.9286,535.0205,522.9055,529.87,499.0443,493.4616,527.3144,535.9689,537.1158,500.5969,493.8693,640.7981,560.7129,606.8747,844.5063,719.7851]
candidate = [530.8326,509.4417,536.3966,502.9586,520.4104,531.5595,511.7368,498.3565,492.6398,547.6426,537.1168,539.5972,550.802,491.5936,495.1215,547.1512,530.6025,581.7492,883.0691,625.2629]
```

Initial full-loop clippy stopped on `unused_io_amount` in the new loopback HTTP
fixture. The repair reads a complete bounded header and asserts against EOF;
the failed output is retained, not called PASS. The restarted validation binds
`QUANTICK_BUBBLES` process-locally to `crates/app/config/bubbles.toml` because the
host inherits a trader-specific bubbles configuration. No user file is changed.
Tracked fixture SHA256:
`5DB6B44A4564F7E0CD26482A3F1BADD17EB41788E6959453330F2738BB37CD4D`.

The first full test run stopped at the catalog consistency check: standalone
schemas had been regenerated but the embedded catalog had not. Its existing
generator updated only the same feed-gap description and additive health
schema. The subsequent full ordered loop passed at source tree
`606946fc4f33a4e5afc0399454e92fa27e2a66c7` on 2026-09-14:

```text
cargo fmt --all -- --check             exit 0
cargo clippy --workspace --all-targets exit 0 (1.10s warmed)
cargo build --workspace                exit 0 (0.98s warmed)
cargo test --workspace                 exit 0
app: 2068 passed; 0 failed; 11 ignored (18.41s)
all subsequent workspace and doc-test suites passed
ordered loop: 16:31:56 to 16:32:53 America/Sao_Paulo
```

Toolchain: rustc 1.98.0 (`88d9e12ae`, x86_64-pc-windows-msvc), LLVM 22.1.8.
At that initial run, base was `eb7bb039434667bb150be9cdf5237e172c4198fe`. Final evidence and
mission archive edits reuse this runtime proof: their entire delta is prose,
not runtime, schema, fixture, dependency or build input. Guards and diff hygiene
are checked again for that delta. Original failed logs and successful raw
outputs remain in the worktree's private `evidence` directory.

Production UI executable SHA256:
`8322C178258A0FE1A72B59B393BF78DC8266DDD3A6B321704B5F5AA91F42135C`.
Its successful initial full build took 2m14s; the subsequent catalog-only change
did not change this executable. A copy outside the target cache is retained for
the coordinator's isolated visual review.

Independent visual observation, final reviews and exact-head CI remain pending
at draft publication. This record makes no score claim, and a headless snapshot
test is not a pixel-level visual verdict.

## Prepublication visual repair

After the initial local archive commit, but before push or PR publication, the
coordinator's independent capture found a 100 ms gap captioned as `0 s gap` and
partly hidden by the top-center book-sync overlay. Structured evidence in the
same captured state confirmed a 100 ms interval and three missing messages.
The initial successful loop above remains historical proof, not validation of
the subsequent repair.

The repair gives `FeedGap` an exact duration label, including `0 ms` and `100 ms`,
without changing coarse stall/status formatting. Captions move into the lower
chart band above the footer/backfill labels, away from the top loading overlay.
The original dashed seam and its market-time position are unchanged.

Targeted checks passed: `gap_captions_preserve_exact_millisecond_bounds`, and
`short_gap_captions_stay_exact_and_clear_of_loading_and_footer_chrome` (0.22s).
The latter inspects actual app paint shapes at 1400x900 and 900x560 with the
production book-sync overlay: equal-time and 100 ms captions must remain in
their chart, above the footer/time axis, and disjoint from the loading label.
It does not replace the coordinator's repeat pixel observations.

The caption repair's full ordered loop passed at tree
`73f4e4f6e58b9e41dfab8093a7c909ad32597c48`, 16:59:09 to 17:02:13 on
2026-09-14 (America/Sao_Paulo): fmt, clippy, build and workspace tests all exited
zero. App: 2069 passed, zero failed, 11 ignored. Fresh production executable
SHA256: `8CD1D369A9A855B24AF5E7335505D916F4AE80766BB64E92364502C6B7560EBC`.

The coordinator then integrated sibling R1 at campaign tip
`05bc95ecfa36339be75411b251edf67cffa79992` and requested a clean rebase before
publication. The clean rebase produced head
`4c394f63dc8fba5e68e221f3d680feb2e20bb8b3`, tree
`a8cebfcffb26dc8a384bcc840895c8b2854c17e2`. Its full ordered loop passed
17:04:09 to 17:08:48: all four commands exited zero, 3910 tests passed,
zero failed, 21 ignored (including two new manual benchmarks). App: 2071
passed, zero failed, 11 ignored. The production executable SHA256 was
`8D9A1A30299B269CBCD4E9E7A7EF24A5D92AD6518A94A77E2A536C70DAEEC3FC`.

An independent source review of that full declared-base diff found one
Should-fix, F1-AR1: the caption/footer clearance needed a documented module-top
tuning constant under the architecture hardcoded-values rule. The bounded
repair names `GAP_CAPTION_BOTTOM_CLEARANCE_PX`, preserving the identical three
text rows and midpoint clamp. Attempt 1 is one compatible source-review repair
batch; no other runtime change was requested. The successful pre-repair output
remains retained, not relabelled as the next tree's validation.

The constant-repair ordered loop passed at tree
`5dd1f20b76e3d5076375b49ab98776b4496994ac`, 17:27:16 to 17:29:30 on
2026-09-14 (America/Sao_Paulo). All four commands exited zero: 3910 passed,
zero failed, 21 ignored; app 2071 passed, zero failed, 11 ignored. The fresh
production executable SHA256 is
`ED0D9E3B08FDD4C7871F8F7E5738E7481AB76D5D256F7685DFC3408498772F63`.

The final publication record reuses that runtime proof. The full delta from
the verified tree modifies only this evidence file and the archived mission;
both are prose records, with no tested input change. Base, toolchain, lock hash
and tracked bubbles fixture remain the identities recorded above. No relevant
untracked/generated input changed. Guards, diff hygiene and the entire record
delta are checked again. Raw command logs, status timestamps and the exact
current-tree reuse receipt remain in the worktree's private evidence directory.

## Independent affected-flow review

The coordinator's [visual and trader-flow report](../../../.claude/evidence/feed-gap-honesty/visual/review.md)
records PASS for the affected warning flow and closes F1-V1/F1-V2. Its complete
normal/narrow/short/equal/reconnect/popup/no-gap matrix and actual campaign-base
popup comparison retain both structured evidence and selected PNGs. The
[preliminary findings](../../../.claude/evidence/feed-gap-honesty/visual/preliminary.md)
preserve failed and incomplete attempts; they are not relabelled successful.
Report bytes match the independent original SHA256
`B0499359B083FEE608A9559DBEE54AC6DED1B50C672B68CB13B03534D948CAA8`.

The successful pixels were captured from tree
`a8cebfcffb26dc8a384bcc840895c8b2854c17e2`, before the nonsemantic constant
repair. The [independent source follow-up](../../../.claude/evidence/feed-gap-honesty/source/followup-staged-5dd1f20b.md)
closes F1-AR1 in the inspected staged runtime and confirms identical geometry.
This explicitly supports pixel reuse; an optional final-binary smoke was
refused by the input guard before launch and is not evidence. The follow-up's
historical pending-test statement is preserved unchanged; later test completion
is documented above, not retroactively attributed to that reviewer.

[The hash manifest](../../../.claude/evidence/feed-gap-honesty/manifest.json)
identifies every copied report, PNG, state and describe transcript. Only
report-referenced captures and their observer describes are included; there are
no descriptors, credentials, executable binaries, user stores or raw app logs.
The initial DPI-invalid crop and incomplete caption attempt are included only
as the preliminary report's explicitly failed/incomplete history.

This additional publication delta is evidence only and reuses the same verified
runtime under the delivery contract: no executable, fixture, schema, dependency,
configuration or build-consumed input changed. Guards, hash equality, NDJSON
parsing, artifact paths, changed relative links and diff hygiene are checked.
Final clean-head canonical architecture/AI/delivery reports and exact-head CI
remain required. No score or mission completion is asserted here.
