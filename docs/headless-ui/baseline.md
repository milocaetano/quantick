# Headless UI baseline and closing contract

The initial fetched `main` is pinned to
`ddb56167420f4a07ee96ab1321fd8b80099986b5`. The adjacent
[baseline.json](baseline.json) is the complete machine-readable inventory.
The measurement reads Git objects, including when invoked from a dirty checkout.
It does not build the app and is not a guard.

```sh
python tools/headless_ui/measure.py --ref ddb56167420f4a07ee96ab1321fd8b80099986b5 --check-baseline docs/headless-ui/baseline.json
python -m unittest discover -s tools/headless_ui -v
python tools/headless_ui/measure.py --ref HEAD --compare docs/headless-ui/baseline.json
python tools/headless_ui/measure.py --ref HEAD --check-chrome
python tools/headless_ui/measure.py --ref HEAD --check-recipe
python tools/headless_ui/measure.py --ref HEAD --check-final
```

`--output PATH` writes the full JSON instead of stdout. Baseline checks compare
the entire inventory, not just totals. C1 runs two independent Git snapshots
and requires identical inventories and the pinned JSON. Check modes exit
nonzero on unmet targets; chrome, recipe and final checks intentionally fail
on the unmigrated baseline. `--compare` reports raw, production and test deltas
and enforces the field ceiling and prohibition on new wiring paths.

| Baseline measure | Lines or fields |
| --- | ---: |
| All app Rust source, raw | 211,902 |
| App production | 90,468 |
| All app-owned tests | 121,434 |
| Raw `crates/app/tests/` | 0 |
| Test files beneath `crates/app/src/` | 103,944 |
| Inline test-only items | 17,490 |
| `QuantickApp` fields | 14 |
| Pane-header test file | 3,213 |

Counts include blank and comment lines. Production classification is a literal
port of `guards::size::production_flags`: column-zero `#[cfg(test)]` excludes
the annotated item, then counting resumes. Indented attributes are retained,
as in the guard. Files in `tests/` directories and external modules reached
through `#[cfg(test)]` are entirely tests. The module walk follows explicit
`#[path]` attributes and their children, including helpers without "test" in
their names. `test_only_modules` makes those ownership decisions auditable.
Inline totals include gated test imports/helpers and external test-module
declarations, not only test function bodies. The pane-header source has four
such lines in addition to its 3,213-line test file. Raw equals production plus
tests, and the three test categories are disjoint. Areas group each source
directory and each root source file; `files` retains every exact path.
`guard_production` additionally reports the unadjusted size-guard total of
93,119: its path filter excludes `tests/` but includes external modules such as
`demo/tests.rs`, `fold_tests.rs`, and the test-only `schema_catalog.rs`. The
classified production/test totals correctly place those modules with tests.

The twelve inventory entries are `agent_popup`, `bar_switch`, `chart_settings`,
`drawing_chrome`, `home`, `indicator_picker`, `indicator_preview`,
`market_palette`, `remove_chart`, `script_editor`, `strategy_popup`, and `toast`.
Each records owned paths, production/test totals, and matching test-file paths.
Tests shared across surfaces remain in the global inventory; filename matching
is a navigation aid, not proof of exclusive test ownership. The separate
`widget_surfaces/mod.rs` collection is an existing adapter, not a thirteenth
surface. Its five widget registrations retain their existing domain owners.

The historical 58-field estimate does not describe this commit. Fourteen root
fields hide further app-owned state: `ChromeState` has 15 fields,
`WorkspaceRuntime` 11, `WindowPresentation` 3, `ControlState` 2 and `Surfaces` 11.
`app_owner_structs` inventories named multiline structs and their field types
throughout app production source, including conditional declarations. Review
the owner inventories as well as root fields: wrapping state, renaming a type,
or moving code between app modules is not extraction. An app view or registry
owner may remain; model decisions and their tests must actually leave app.

Fixed closing targets:

- App production must be below 90,468 lines; all app-owned tests below 121,434.
  The unadjusted guard production total must also fall below 93,119.
- `QuantickApp` has at most 30 fields. No new `*_wiring.rs` path is allowed.
- Chrome retires `capsule_wiring.rs` and `toolbar_wiring.rs`. `layout_wiring.rs`
  also owns session layout effects, indicator fan-out and drawing persistence;
  its chrome callers migrate, while domain adapter ownership is preserved.
- `ui-model` exists and contains no egui, eframe or wall-clock sampling.
  Duration values supplied by callers are allowed. The final command checks
  source/dependency spelling; the full dependency graph and headless-only model
  test execution are separate required delivery evidence.
- A new surface docks as a model file, a view file and one catalog line, with
  zero `QuantickApp` edits. A throwaway docking diff must prove this at closing;
  measurement alone cannot prove behavior or architectural ownership.

The recipe lives at
`.claude/skills/new-extension/references/headless-ui.md`. Its first fenced JSON
block must contain string keys `model`, `view`, `catalog`, and `registration`.
Paths name shipped repository files; `model` is inside `crates/ui-model/`,
`view` inside `crates/app/`. `registration` is the exact single catalog line,
including indentation, and names the model and view file stems. The recipe
check validates those files and that line against the selected Git revision.
An optional path after `--check-recipe` permits explicit recipe placement.
The final check includes the default recipe check. This contract is deliberately
independent of the future catalog's Rust macro or registration syntax.
