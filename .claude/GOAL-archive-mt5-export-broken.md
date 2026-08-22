# Mission: the MetaTrader export died on a typo — bring the download back

- **Branch**: `fix/mt5-export-download`
- **Worktree**: `C:\src\quantick-worktrees\fix-mt5-export-download` (every write happens here, never in the main checkout)
- **Done** = PR open with green CI and the evidence in its body. Merge is NEVER part of the mission — only Camilo merges.

## Objective (user's request)

Downloading WINV26 tapes from MetaTrader 5 stopped working. A month of data
used to come down; now nothing does, so there is nothing new to replay for the
mini index. Fix the feature.

## Root cause (found 2026-08-14, before any code was written)

`tools/mt5/export_session.py:484` reads

```python
venue_flagged=0 if one_sided_flags else flagged,
```

Neither `one_sided_flags` nor `flagged` exists in `export_tape` — `flagged` is
a local inside `flags_are_one_sided`, and `one_sided_flags` was never written.
Python resolves names at execution, so the file imports and parses fine and
then raises `NameError` the moment the line runs, which is **after** the whole
tape has been read and formatted and **immediately before** `write_atomically`.
Every tape export therefore dies without writing a byte; the app sees a
non-zero exit and reports "MetaTrader 5 stopped with code 1".

Introduced by `0976ec3d` (2026-08-13 11:33, already on `main`), the commit that
added the one-sided-flags policy. The user's own disk dates it exactly: the last
tape that landed is `C:\Users\Camillo\quantick-data\WINV26\2026-08-12.csv`,
written **2026-08-13 01:29** — nine hours before that commit.

Nothing in the repo could have caught it: `--probe` never reaches the line (so
the calendar still fills in, which is why the feature *looks* alive), the four
checks are cargo-only, and `tools/mt5/test_export_session.py` covers the pure
side-inference helpers but never calls `export_tape`.

## Acceptance criteria (evidence required for each)

1. **Root cause declared** — the above, in the conversation and in the PR body.
2. **The emit fixed**, and `python -m ruff check --select F tools/mt5/ bridge/mt5/` prints 0 errors.
3. **`python tools/mt5/test_export_session.py` passes**, including a NEW test that drives `export_tape` end to end against a fake MetaTrader5 module and that fails on the `NameError` before the fix.
4. **Permanent guard against the class of bug** — ruff (F rules) and the exporter's tests run in CI; `.github/workflows/ci.yml` edited and the hunk shown.
5. **Probe/calendar and WINV26 replay verified**, or the limitation declared with the exact command for Camilo to run on his terminal.
6. **Four checks exit 0**: `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo build --workspace` · `cargo test --workspace`.
7. **arch-review** over `git diff main...HEAD`, no Blocker or Should-fix left open (or deferred in the PR body).
8. **All work in this worktree**, never in the main checkout.
9. **PR URL printed.**
10. **This file archived** as `.claude/GOAL-archive-mt5-export-broken.md`.

## Performance impact (declared as part of the plan)

Every path touched is **rare**, not hot: `export_tape` runs once per download in
a separate one-shot Python process, and the CI job is build-time. No per-trade,
per-depth or per-frame code is edited, so no fps evidence is owed.

## Notes / boundaries

- The Rust side of the download (`replay_download.rs`, `replay_get_data.rs`) is
  not implicated: it faithfully reported the exporter's non-zero exit. Do not
  refactor it.
- The tapes already on disk carry `side_source=venue_flags` with every print
  flagged `B` — the very lie `0976ec3d` set out to fix. Re-downloading those
  days after the fix is the user's call, not part of this mission.
