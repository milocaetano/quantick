# Evidence — pin the build environment

Everything the mission's criteria said would be written down. Every command was
run in the branch worktree, each on its own: a `&&` or a trailing `|| echo` has
reported success in this repository while one of the four had failed.

The branch was cut from `origin/main` at `0901993` and `main` has not moved
since, so the four checks below were run against the latest `main` with no
rebase needed (`git rev-list --count HEAD..origin/main` = 0).

## G1 — the four checks

| check | exit | result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets` | 0 | no warnings — **and no `-D warnings` on the command line** |
| `cargo build --workspace` | 0 | `Finished dev profile in 2m 45s` |
| `cargo test --workspace` | 0 | **3,316 passed, 0 failed** across 90 suites — see below |

Beyond the four, and because this branch touches what CI runs:

| check | exit | result |
| --- | --- | --- |
| `cargo test -p quantick-guards` | 0 | 65 passed — includes the size and context ratchets |
| `sh .claude/hooks/guardrails_test.sh` | 0 | 96 passed, 0 failed |
| `cargo deny check bans licenses` | 0 | `bans ok, licenses ok` |
| `cargo deny check advisories` | 0 | `advisories ok` |

The Python steps were not run: this branch touches nothing under `tools/mt5/`
or `bridge/mt5/`.

### One test needed fixing, and it is the interesting one

`cargo test --workspace` failed on the first run, and it caught a real
consequence of the dependency move that neither clippy nor the build could see:

```
---- control::evidence::tests::the_reported_graphics_backend_is_the_one_the_manifest_selects ----
panicked at crates\app\src\control\evidence.rs:2095:9:
the bundle reports `glow` but the manifest selects: { workspace = true }
```

The test reads a manifest to confirm the renderer an evidence bundle *names* is
the one the build actually links — a rule the compiler cannot see, so the repo
checks it against its own manifest. It read `crates/app/Cargo.toml`, where the
`eframe` feature list used to live. After the move that file says only
`eframe = { workspace = true }`, and the feature list is in the root.

Repointed at the workspace manifest. The failure mode is the good one: aimed at
the wrong manifest the test does not quietly pass, because both `expect`s still
find something and the assertion fails naming what it actually read — which is
exactly how the move was caught. Only `cargo test` could catch it at all;
`cargo clippy --all-targets` compiles tests but does not run them, which is why
the first three checks were green while this was broken.

## A1 — the toolchain is pinned, and the pin is what gets used

`rust-toolchain.toml` pins `channel = "1.98.0"` with `components = ["clippy",
"rustfmt"]`. That is the version CI ran, read from the most recent green run on
`main` rather than assumed — run `33701431733` logs:

```
stable-x86_64-unknown-linux-gnu unchanged - rustc 1.98.0 (88d9e12ae 2026-08-18)
```

The file is load-bearing, not decorative: the first cargo command run in this
worktree after it was added made rustup act on it.

```
$ cargo check -p quantick-guards
info: syncing channel updates for 1.98.0-x86_64-pc-windows-msvc
info: latest update on 2026-08-20 for version 1.98.0 (88d9e12ae 2026-08-18)
info: downloading 5 components
```

CI no longer installs a floating `stable` beside it: the
`dtolnay/rust-toolchain@stable` step is replaced by `rustup show`, which acts on
the file, and the step echoes `rustc`, `cargo clippy` and `cargo fmt` versions
so the log records which toolchain a given run used.

## A4 — the dependency move changed no resolution

Two measurements, both taken across the `[workspace.dependencies]` migration
and before the unrelated `webbrowser` bump below.

**`Cargo.lock` is byte-identical.** No package changed version, and none was
added or removed.

**The resolved feature set per package is identical.** 385 `package|features`
rows, no difference:

```
$ cargo tree --workspace -f "{p}|{f}" | sed 's/^[^a-zA-Z]*//' | sort -u   # before and after
385 rows each, diff empty
```

`cargo tree -e features` *does* differ, by 297 lines, and that difference is the
point rather than a problem: it records which crate *requests* a feature. A feed
now asks for the union entry rather than its own narrower one. What each package
ends up compiled with is unchanged, because cargo's resolver already unified
these across the workspace graph.

## A5 — the lints table fails on exactly what CI fails on

A deliberate rustc warning, with no flag on the command line:

```
$ cargo check -p quantick-guards
error: function `deliberately_unused_for_the_lint_proof` is never used
  = note: `-D dead-code` implied by `-D warnings`
```

And a clippy-specific lint, which is the half that matters for R4 — CI's old
`-D warnings` covered these, and a plain local `cargo clippy -p <crate>` did
not:

```
$ cargo clippy -p quantick-guards
error: length comparison to zero
  = note: `-D clippy::len-zero` implied by `-D warnings`

$ cargo check -p quantick-guards
    Finished `dev` profile      # rustc does not know this lint, and is not asked to
```

Both probes were reverted; `git status` on `crates/guards/src/lib.rs` is clean.

## A10 — the guard fails when the property is broken

Two new tests in `crates/pine/tests/workspace_deps.rs`, each demonstrated
against a deliberate violation and then restored.

Restating a version outside the root:

```
$ sed -i 's/^rust_decimal = { workspace = true }$/rust_decimal = "1"/' crates/orderbook/Cargo.toml
$ cargo test -p quantick-pine --test workspace_deps third_party_versions
panicked: these lines state a third-party version outside the root manifest: [
    "orderbook: rust_decimal = \"1\"",
]
```

A crate not inheriting the lints:

```
$ # remove [lints] from crates/sim/Cargo.toml
$ cargo test -p quantick-pine --test workspace_deps every_crate_inherits
panicked: these crates do not inherit `[workspace.lints]`: ["sim"]
```

Restored, the suite is green: 6 passed.

Both tests walk `crates/` rather than a list, so a crate added later is covered
without anyone remembering to register it — the lesson the neighbouring
`every_crate_is_covered_by_a_dependency_rule` was written to record.

## A7 / A9 — what the supply-chain checks found

`cargo deny check advisories` was red on arrival, which is the best available
argument for the trader's decision to keep it off the pull-request path: both
findings predate this branch and neither has anything to do with it.

**`RUSTSEC-2026-0257`, a vulnerability — fixed.** `webbrowser 1.2.1` allowed
browser argument injection through the Unix `BROWSER` template. It reaches the
tree as `quantick-app → eframe → egui-winit → webbrowser`. `cargo update -p
webbrowser` took it to 1.2.4, in a commit of its own so the lockfile change is
attributable to the security fix rather than to the dependency migration. It
also dropped a duplicate `core-foundation 0.10.1`.

**`RUSTSEC-2026-0192`, unmaintained — recorded, with an expiry.** `ttf-parser`
arrives as `eframe → egui → epaint → ab_glyph → owned_ttf_parser → ttf-parser`.
No `eframe 0.29` exists without it, so the only fix is a major egui upgrade,
which is not the business of the branch that added the check. It is recorded in
`deny.toml` with the reason and with
[issue #283](https://github.com/milocaetano/quantick/issues/283), which deletes
the entry. No lint or advisory was silenced without one.

Licences: the allow-list is the set the tree actually contains, over every
target rather than just the CI runner's. Two are worth a reader's attention and
neither is quietly allowed — **MPL-2.0**, weak file-level copyleft, reaching the
tree twice (`option-ext` under `dirs`, and the `symphonia` family under `rodio`
for the Windows alarm clips), both consumed as unmodified libraries; and
**`LicenseRef-UFL-1.0`** with **OFL-1.1**, the Ubuntu and Open Font Licences on
the typefaces `eframe` ships.

`bans` keeps `wildcards = "deny"` with real teeth, which required declaring
`publish = false` — cargo-deny reads a version-less `path` dependency as a
wildcard and exempts one only for a crate that cannot be published. That is a
true statement about all seventeen crates. Duplicate versions are `warn`, not
`deny`: 49 crates in this tree resolve to more than one version, `windows-sys`
alone at four, and denying that would mean a 49-entry skip list that reddens a
pull request whenever something two levels down bumps.

## G2 — performance impact

No path is touched at any rate. The branch changes manifests, CI configuration,
documentation and one test's `include_str!` target. The only production Rust
line that moved is inside `#[cfg(test)]`.

The `webbrowser` bump is the one change reaching shipped code, and it is a
patch-level bump inside a dependency invoked when the user opens a link — a
rare path, not per-trade, per-depth or per-frame.
