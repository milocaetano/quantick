# What left the tree, and the proof nothing needed it

Measured on `chore/public-surface` against `origin/main` at `aaf74d3`.
Criteria **A2**–**A7**.

## A3 — nothing reads the three root state files from the repository root

The ledger's own grep, re-run on the branch before the files were removed:

```
$ git grep -nE '\.\./\.\./(layouts|paper-state|quantick-symbols)\.toml' -- crates
$ echo $?
1
```

Empty. The wider grep — every mention of the three names anywhere in `crates`,
`tools`, `bridge`, `.claude`, `.github` and `docs` — returns only:

- the constants that name the file relative to the **working directory**:
  `crates/app/src/layouts.rs:50`, `paper_state.rs:25`, `symbols_file.rs:27`,
  `config.rs:968` and `:999`;
- tests that join the name onto a **temp root** they created:
  `paper_home.rs:651`, `:707`, `workspace_bundle.rs:710`, `:723`, `:729`,
  `workspace_store.rs:638`;
- the bundle's own manifest of filenames, `workspace_bundle.rs:420`–`437`;
- prose: `feeds.toml`, `app.rs:1477`, `paper_home.rs:14`, `ui_state.rs:665`,
  `hook-registry.md:75`, `hook-prose.md:89`, two `GOAL-archive-*.md`;
- two harness capture scripts that point `QUANTICK_LAYOUTS` and
  `QUANTICK_PAPER_STATE` at their **own scratch stores**
  (`.claude/evidence/mt5-session-history/capture.ps1:19`, `:27`,
  `mt5_open.ps1:11`, `:19`).

Not one of them resolves against the repository root. The build that proves it:

```
$ cargo test -p quantick-app
test result: ok. 1894 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out
```

## A2 — the three root state files

```
$ git ls-tree --name-only HEAD | grep -E '^(paper-state|layouts|quantick-symbols)\.toml$'
$ echo $?
1
```

`.gitignore` gains three lines in the existing "written by the app at runtime"
style, one comment each, beside `/chart-layers.toml`:

```
/layouts.toml
/paper-state.toml
/quantick-symbols.toml
```

`paper-state.toml`'s two lines — including
`trades_dir = 'C:\Users\Camillo\OneDrive\Documents\BTCUSDT'` — stay in history
at `d407444`. Untracking is the fix; rewriting history is out of scope.

## A4, A5 — the evidence screenshots

| | before | after |
| --- | --- | --- |
| PNGs under `.claude/evidence/` | 31 | 0 |
| bytes those PNGs occupied | 10,227,830 | 0 |
| non-PNG files under `.claude/evidence/` | 97 | 97 |

```
$ git ls-tree -r HEAD -- .claude/evidence | grep -c '\.png$'
0
```

Every `.md`, `.txt`, `.log`, `.ps1` and `.py` survives, including the two
capture scripts that *produce* the screenshots — they are the recipe, and the
recipe is what a reader can re-run.

`.gitignore` gains `.claude/evidence/**/*.png` with the reason written beside
it, in the same voice as the `!.claude/evidence/**/*.log` rule three lines
above that it deliberately does not contradict: the log is the measurement, the
pixels are the trader's local proof.

**Dead links repaired.** Two Markdown links pointed at a PNG and now name it in
plain text — `mt5-session-history/in-the-app.md:9` and `second-operator.md:79`.
The five documents that name a PNG at all each carry a note under their title
saying the screenshots were *captured locally, not tracked*:
`harness-hook-owner/visual-qa.md`, `mt5-session-history/in-the-app.md` and
`second-operator.md`, `report-out-of-the-ticket/visual-qa/README.md`,
`workspace-persistence-owner/visual-qa.md`.

`.claude/GOAL-archive-mt5-session-history.md` also cites two of the deleted
PNGs and is **left verbatim**. An archive is the record of what a shipped
branch claimed at the time it shipped; editing one to match a later tree change
makes it a worse record, not a better one, and the brief puts those 78 files
out of scope.

## A6, A7 — the design lab

`heatmap-design-ref/` (8 files, 518,193 bytes) is gone from the root:

| file | where it went |
| --- | --- |
| `capture_window.ps1` | `tools/capture_window.ps1` — the PrintWindow capture the harness runs |
| `heatmap_lab.py` | `tools/heatmap-lab/heatmap_lab.py` — still a working tool; it reproduces `orderflow_render.rs`'s colour pipeline outside the egui app |
| `heatmap_study.html`, 4 PNGs, `maximize.ps1` | deleted, 487,933 bytes — the lab's output and one script nothing in the repository names |

Both moved scripts were translated on the move: `capture_window.ps1`'s header
and its `NOT_FOUND` message, and two ramp labels in `heatmap_lab.py`. Its usage
line also named a `-TitleMatch` parameter the script has never had; it now names
`-ProcessName`, which it does. `python -m py_compile` on the moved lab passes.
`__pycache__/` was already ignored.

No live reference to the folder survives. The bare grep is **not** empty, and
saying it was would be the dishonest version of this line: an honest record of
a removal contains the name of what it removed, so this branch's own two
records — the mission archive and this file — match it. Scoped past them:

```
$ git grep -n 'heatmap-design-ref'     -- ':!.claude/GOAL-archive-public-surface.md'        ':!.claude/evidence/public-surface/tree.md'
$ echo $?
1
```

Nothing under `crates/`, `tools/`, `.github/`, `docs/`, `.claude/hooks/` or
`.claude/skills/` names it — no script, no skill line, no config path.

**The context budget (A7, R9).** `ui-harness/SKILL.md:71` now names
`tools/capture_window.ps1` — 24 bytes against the 37 it replaces, so the file
shrank. `arch-review/references/language.md:16` loses `heatmap-design-ref/`
from the pre-existing-debt exemption list, because the debt it named was
translated rather than relocated. `cargo test -p quantick-guards`: 138 + 16 + 5
passed, 0 failed.

## What did not shrink

`git count-objects -vH` reports `size-pack: 29.69 MiB` and will keep reporting
it. Deleting a blob from the tree shrinks a **checkout**, not a **clone**:
history still holds every PNG. The checkout is about 10.7 MB lighter
(10,227,830 + 487,933 = 10,715,763 bytes); the clone is not, and the PR body
says so rather than claiming a number it cannot back.
