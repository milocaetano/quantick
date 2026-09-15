# Current-code feature validation

Source: 1b4bad7ebb257d0c0a016db6994338592b367bda, clean runtime inputs.
Worktree: feat-sync-main-chart-layout. Exclusive build target:
C:/src/quantick-worktrees/feat-evidence-resources/target.

Read-only validator feature_validation executed:

- cargo build -p quantick-app --features quick-range-harness: exit 0, 1m08s.
- cargo test -p quantick-app --features quick-range-harness: exit 0;
  compile 2m16s; 2076 passed, 0 failed, 11 ignored, 15.40s test execution.

Test executable: debug/deps/quantick_app-4895c18869080ee5.exe.
SHA-256: DF11DF1B756A4EF6FC7315F2FE79109217256A43218BF00285D055A300C85147.
Size: 59564032 bytes; modified 2026-09-15T06:16:15.0591216Z.
This is a validator receipt, not a full verbatim stdout archive or CI report.
The ordinary-build ordered workspace receipt is ordered-validation-v4.md.

The first feature app executable did not embed a source SHA. Capture correctly
refused that provenance; it was not accepted as verified visual evidence.
Root then set process-local QUANTICK_GIT_COMMIT to the exact source above and
rebuilt the same feature app in session 16723: exit 0, 28.72s. Runtime source
did not change. The metadata-stamped visual executable SHA-256 is
6C3AFF359D548AB3AF35D972F99B59D374C03F0E74C2238A687E98D807242466.
Root also rebuilt quantick-mcp: exit 0, 9.65s. Build metadata alone is not
proof; visual receipts additionally associate the owned PID, executable hash,
live system information, bundle/chunk hashes and decoded screenshot.

No rubric, score skill, measurement script, trader settings or trader process
was changed. No score, final review approval or PR readiness is implied.
