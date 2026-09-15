# MS1 validation inputs and chronology

This evidence describes synchronization issue #491. It is not a score, benchmark, or inherited approval.

- Root source-first PASS: https://github.com/milocaetano/quantick/issues/491#issuecomment-5671488448
- Starting campaign commit/tree: `432632239f924883c07e76904551be0faf0f37eb` / `fdb46a97219a096e8686c17f4628c15c374b61f3`.
- Exact incoming main commit: `1429941feca0efecadb305959f4b3293c84ab323`; merge base `eb7bb039434667bb150be9cdf5237e172c4198fe`.
- Validated staged tree and merge-commit tree: `bee98ce07dc2c440f59f0c8272e48170623cd390`.
- Cargo.lock SHA-256: `8826f73f0589b91f318c40316c18c1925b926bd7a03c0ddb393ca0a84ec795fa`.
- Toolchain: Cargo `1.98.0 (797e8a9bc 2026-08-05)`; rustc `1.98.0 (88d9e12ae 2026-08-18)`.
- Process-local bubbles fixture: `crates/app/config/bubbles.toml`, SHA-256 `5db6b44a4564f7e0cd26482a3f1badd17eb41788e6959453330f2738bb37cd4d`.
- Full-loop cache: released R1 target `C:/src/quantick-worktrees/fix-mutation-retry-truth/target`, exclusively leased to MS1 during the loop, with `CARGO_BUILD_JOBS=1`; no clean was run and durations are execution provenance, not calibration.
- Initial pre-mission guard build passed. The first relevant pre-edit check started `2026-09-14T21:58:51.0560873Z` and exited zero after Cargo-reported `48.07s`; its original output exists only as a tool receipt because no raw file was captured. It is not reconstructed here.
- A persisted rerun after mission creation but before product edits ran from `2026-09-14T22:04:54.2349144Z` through `22:04:55.3180858Z`, with guard/check exit zero. It is explicitly later evidence, not relabelled as the original pre-edit run.
- The history-preserving merge ran `--no-commit` from `22:10:55.5826545Z` to `22:10:55.9541977Z`, exit zero, with no conflict. Root then requested one 28-line public-boundary regression; the earlier fmt result is retained as historical and the full ordered loop restarted after that edit.
- Targeted boundary test passed at `22:15:29.9449740Z`.
- Final ordered loop at tree `bee98ce07dc2c440f59f0c8272e48170623cd390`: fmt exit zero `22:16:03.2372592Z`; clippy exit zero `22:17:51.0369890Z`; build exit zero `22:26:57.0930564Z`; workspace test exit zero `22:43:14.6010302Z`.
- The clippy transcript retains a non-fatal PowerShell `SHA256.HashData` metadata error and an empty optional status hash; the command's head/index tree and terminal exit are intact. Later receipts use compatible `SHA256.Create().ComputeHash()`.
- The workspace-test stream exceeded the tool's output cap in two bursts. The retained log includes the host's explicit truncation markers and does not infer missing lines or counts. Its exact start/input, terminal `EXIT_CODE=0`, and finish remain present.
- Tracked transcripts normalize line endings and remove three single-space blank context lines so Git's whitespace check passes. Exact private receipts remain in the worktree git-dir; no substantive output was rewritten.
- The hook suite ran at the merge commit from `22:44:55.5594322Z` to `22:54:53.1323131Z`: `277 passed, 0 failed`, exit zero.
- Initial campaign API operation counter: `0`. Initial review repair counter: `0`. Local shell quoting/metadata diagnostics are retained but are not GitHub operation or review-repair attempts.
- Final evidence validation repair 1: `git diff --cached --check` found three single-space blank lines in captured output; the guard executable itself passed. Those three spaces were removed, the failure receipt was retained privately, and both checks were rerun. No product line or evidence meaning changed.
