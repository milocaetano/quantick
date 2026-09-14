# Feed continuity evidence (F1, campaign #472)

Base: `eb7bb039`, `origin/campaign/outside-eight`. Child issue: #474.

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
Base remains `eb7bb039434667bb150be9cdf5237e172c4198fe`. Final evidence and
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
