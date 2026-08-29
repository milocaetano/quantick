# Mission

Land the per-open-chart layout on `main`: every chart pane holds its own
active layout, chosen from the workspace's one shared list of layouts, so a
rename or an indicator edit shows on every pane that uses that layout while
switching one pane's layout leaves the other panes where they are.

Branch: `fix/layout-per-chart-lands`, worktree
`../quantick-worktrees/fix-layout-per-chart-lands`.

## Why the merged PR did nothing

PR #250 (`feat/layout-tabs` -> `main`) merged at 2026-08-29 03:33:16.
PR #251 (`feat/layout-per-pane`) was stacked, so its base was
`feat/layout-tabs`, not `main` — and it merged 28 seconds later, at
03:33:44, into a branch that had already been merged and abandoned. Its
merge commit `283d74b` lives only on `origin/feat/layout-tabs`;
`git merge-base --is-ancestor 283d74b main` is false. The trader merged a
green PR and the app never changed, because the code never reached `main`.
The work is recovered by merging that orphaned tip into a branch cut from
today's `main` (their merge base is exactly `46a1cbb`, so the delta applied
is precisely PR #251), then re-proving it against three PRs of drift
(#252, #253, #254).

## Acceptance criteria

### Mission-specific

1. **The lost work is on the branch.** `git merge-base --is-ancestor 283d74b
   HEAD` succeeds; the PR body states the merge-order diagnosis above so the
   same stacked-PR mistake is recognisable next time.
2. **A layout per open chart.** `ChartPane` carries the `LayoutId` it shows.
   The footer strip, `Alt+1..9`, the View menu and the control plane's
   `layout.tab.switch` change the *focused* pane only; a new pane opens on
   the focused pane's layout (or the book's default). Test: two panes on two
   layouts show two different indicator sets at once.
3. **One shared list of layouts.** Create, rename and delete act on the one
   `LayoutBook`: renaming `Layout 1` renames it in the strip every pane
   reads. An indicator edit on a pane mirrors onto every pane showing the
   *same* layout, in every market tab, and onto no other pane. Tests prove
   the rename is single-sourced and that an edit on layout 1 reaches a second
   pane on layout 1 and not a pane on layout 2.
4. **Drawings follow the pane's own layout.** Put-away/bring-out use the
   pane's layout, so switching one pane's layout swaps only that pane's
   drawings; the other pane keeps what is on it.
5. **Persistence.** The workspace file records each pane's layout and
   restores it on relaunch; a workspace file written before this change opens
   with every pane on the book's active layout. Tests: round trip, old-file
   default.
6. **Proven in the running app, not only in tests.** The app is launched
   through the `ui-harness` hook (`QUANTICK_PANE_LAYOUTS=<name,name,...>`)
   and a capture shows two panes on two different layouts at once; a rename
   made in one pane's strip shows in the other's. Screenshot paths in the PR
   body — a green test suite is what shipped last time.

### Standard gates

7. Every artifact in English (arch-review dimension 8; `language_guard.rs`).
8. Four checks green after rebasing on latest `main`: `cargo fmt --all --
   --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace`.
9. Performance impact declared by rate: the per-pane layout lookup and the
   drawing put-away sit on the per-frame path and must stay allocation-free
   on a quiet frame; the indicator fan-out and a layout switch are rare-path;
   the per-trade path is untouched. `APP_HEALTH_SUMMARY` fps/frame_avg under
   a dense tape against a `main` control run, numbers in the PR body.
10. `ui-harness`: the pane-layout hook is present and documented in the
    skill.
11. `visual-qa` PASS on the affected surfaces (layout strip lighting the
    focused pane, two panes on two layouts, context pane headers naming their
    layout); `trader-ux-review` with no unresolved Blocker.
12. `arch-review` run over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred in the PR body; PR opened. Merging is not
    part of the mission.
