# Ordered verification after MVU-P1/MVU-P2

Input manifest: candidate-inputs-v4.json,87 files, SHA2561825E86A366C894FD37CBC7AA532A1F2190280EAC12D8615FEDF19932C46A626.
Worktree feat-sync-main-chart-layout, precommit HEAD9ff57501/MERGE_HEADa6644bc6.
Child CARGO_TARGET_DIR=feat-evidence-resources/target; only inherited QUANTICK_BUBBLES removed for fixture isolation.

Commands executed in order, stop-on-failure shell, session64485, final exit0:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
Finished dev profile in17.32s
cargo build --workspace
Finished dev profile in1m40s
cargo test --workspace
Finished test profile in2m39s
```

Verbatim suite-summary excerpts from the captured output (successful individual test names omitted; first output chunk was tool-truncated, not presented as complete log):

```text
test result: ok. 2075 passed; 0 failed; 11 ignored; 0 measured; 0 filtered out; finished in 23.13s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_backtest-5d4325db6bc7945a.exe)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\main.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_backtest-2b6063c506b721e6.exe)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\determinism_guard.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\determinism_guard-c424afccf5982733.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\harness.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\harness-e7cec12d2ca253b5.exe)
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\paper_account.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\paper_account-b16db5a7ab1862c5.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_chart_interaction-46a037ba99f651ec.exe)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_civil-bcb95fcbf0c35160.exe)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_control-bfeed44d0c5983fa.exe)
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests\canonical_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\canonical_contract-8b15bc9ad4aac759.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\codec_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\codec_contract-e6f9ba8ae4d139ef.exe)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\fake_host_client.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\fake_host_client-dc9a67843e2d6ff7.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
     Running tests\handshake_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\handshake_contract-1a5daa908c0e5be1.exe)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\identifiers.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\identifiers-b1f2f8d5373ecf11.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\pagination_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\pagination_contract-29ddeaf4f6ee62e7.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\published_schema_compatibility.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\published_schema_compatibility-a9937cea964f9ce6.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests\registry_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\registry_contract-0b95e1567acc6363.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\schema_compatibility.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\schema_compatibility-ca00b56cca05897d.exe)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\schema_snapshots.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\schema_snapshots-7917d159c37dd6b2.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_control_host-102f8186a197f868.exe)
test result: ok. 47 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests\admission_contract.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\admission_contract-ff0b6f2615bbf76d.exe)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests\evidence_resource.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\evidence_resource-bd67cbd58a192fa9.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_control_local-40f7b69c72ee1d28.exe)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.64s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_engine-135aea9964f6a10e.exe)
test result: ok. 132 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.69s
     Running tests\bar_spec.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\bar_spec-ef86b0dab0e3cb73.exe)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\fixture_roundtrip.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\fixture_roundtrip-5d00ed4424dff9c1.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\forming_run.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\forming_run-7c4bea802b1736d3.exe)
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.66s
     Running tests\golden_dollar.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_dollar-10c6bc1c038de8b8.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_harness.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_harness-bcd81d2a04fef598.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_imbalance.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_imbalance-4c053f210993a2a9.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_tick.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_tick-60c40aab39ed1c78.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_time.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_time-b0dac9f7980563a7.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_volume.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_volume-d691aa54a7183e81.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\profile_fold_parity.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\profile_fold_parity-492688b761e90376.exe)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
     Running tests\trade_tape.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\trade_tape-d92475e7f6ab0f92.exe)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.65s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_feed-5d6bfffbe1d54b40.exe)
test result: ok. 140 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 5.20s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_feed_binance-4aadaa213de451ae.exe)
test result: ok. 48 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.09s
     Running tests\anomaly_tracing.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\anomaly_tracing-5279742f016f9283.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\depth_tracing.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\depth_tracing-9ac5c5966c64ed2c.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\depth_wire_mapping.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\depth_wire_mapping-d1d66d10fe33227e.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\wire_mapping.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\wire_mapping-36382a393e2f5ed1.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_feed_hyperliquid-b588c51068e790a8.exe)
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
     Running tests\live_market_smoke.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\live_market_smoke-8f1a74649fd1d0cf.exe)
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\wire_mapping.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\wire_mapping-35634aaf7762fb95.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_feed_mt5-73b95129cdb5be3b.exe)
test result: ok. 89 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\bridge_paging.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\bridge_paging-33d57fda59800e8d.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.99s
     Running tests\bridge_server.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\bridge_server-2058ba5a24957e10.exe)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.01s
     Running tests\fixture_replay.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\fixture_replay-2d987b206cb3d95e.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\quote_fixture_replay.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quote_fixture_replay-7ac981175a3c373a.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_guards-9d693ac897417a20.exe)
test result: ok. 200 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.77s
     Running unittests src\main.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_guards-1752b430820b65a2.exe)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\capability_documentation.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\capability_documentation-75cab13858967042.exe)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s
     Running tests\extension_boundary.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\extension_boundary-01e1eff3c1fc88ef.exe)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.89s
     Running tests\guards.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\guards-a02ea2a81509d639.exe)
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.30s
     Running tests\session_gap_agreement.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\session_gap_agreement-ea33140b49947fe7.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_indicators-79eac66e61a7befd.exe)
test result: ok. 86 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\bot_readiness.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\bot_readiness-620de691077552ee.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\fmath_guard.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\fmath_guard-d35329df69bdcc91.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_avwap.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_avwap-df6f7bf8d1a50381.exe)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_harness.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_harness-c03964d27b38dd56.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\golden_ta.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\golden_ta-0a0eea0c8e39a305.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\host.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\host-51f1cccf0f241c38.exe)
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\paint_golden.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\paint_golden-fe149aecb3edc4fc.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_mcp-0e3e195233151741.exe)
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running unittests src\main.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_mcp-3a4dada14c20176b.exe)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\fake_gateway.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\fake_gateway-8aa1bdbc87c278e4.exe)
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
     Running tests\stdio_smoke.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\stdio_smoke-a82344300ff0dd9e.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_orderbook-b5261f2fa997463b.exe)
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_orderflow-b82fb9a472a5c1e8.exe)
test result: ok. 186 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_paper-5c764cbe8fd76e00.exe)
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_pine-1bca0fc9ed24f05b.exe)
test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\barcolor_semantics.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\barcolor_semantics-5c24f8b3afd6573c.exe)
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\compile_passes.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\compile_passes-842f699273af0ec3.exe)
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\copilot_semantics.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\copilot_semantics-d4f4b7702dce6255.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\corpus.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\corpus-0f2a7fcb2280b71a.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\dialect_doc.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\dialect_doc-a2b06bf1cd793e97.exe)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\exhaustion_reversal_semantics.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\exhaustion_reversal_semantics-9fd9f60cdcd47176.exe)
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\force_bar_semantics.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\force_bar_semantics-834f572122ddf2aa.exe)
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\preview_cost.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\preview_cost-ff46ead13aa1d646.exe)
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\script_semantics.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\script_semantics-8510af2a86b1a105.exe)
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_replay-27d67c814b0a4cbe.exe)
test result: ok. 93 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
     Running tests\exported_session.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\exported_session-415c9c25428de1bd.exe)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_sim-49a07bf6084e8190.exe)
test result: ok. 67 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\exit_ladder.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\exit_ladder-aeb95907906c3219.exe)
test result: ok. 10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests\port_parity.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\port_parity-5293b31ef5de1b4e.exe)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_strategy-15e92c6b5b26e252.exe)
test result: ok. 73 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests\full_operation.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\full_operation-ba4bb5d45acabaa5.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (C:/src/quantick-worktrees/feat-evidence-resources/target\debug\deps\quantick_trading-cfcd550deb0128e0.exe)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests quantick_backtest
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests quantick_chart_interaction
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_civil
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_control
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_control_host
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_control_local
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_engine
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_feed
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_feed_binance
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_feed_hyperliquid
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_feed_mt5
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_guards
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_indicators
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_mcp
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_orderbook
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_orderflow
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_paper
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_pine
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_replay
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_sim
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_strategy
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests quantick_trading
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

All workspace tests and doctests passed, including app2075/0failed/11ignored, headless14, control25, guards200, guard integration26, extension boundaries13 and capability documentation14. Feature-enabled validation remains separate. Frozen seven working blobs rechecked unchanged. Prior guardrails277PASS and cargo-denyPASS are reused: corresponding scripts/fixtures/Cargo.lock/toolchain are unchanged since that verified input; only reviewed runtime/test additions differ. No final CI/review/score verdict is implied.
