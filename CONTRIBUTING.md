# Contributing to quantick

Thanks for your interest! The whole point of this project is to open up tooling that has historically been private, so ideas, questions and code are all welcome.

- **Ideas, questions, design discussion** → [Discussions](https://github.com/milocaetano/quantick/discussions)
- **Actionable work** (bugs, features with a defined scope) → [Issues](https://github.com/milocaetano/quantick/issues)

## Getting started

You need [rustup](https://rustup.rs/). You do not need to pick a Rust version:
`rust-toolchain.toml` at the repository root pins the exact one, and rustup
installs and selects it — with the `clippy` and `rustfmt` components — the
first time you run a cargo command in this directory. CI reads the same file,
so a toolchain release can never turn your pull request red on its own.

```sh
git clone https://github.com/milocaetano/quantick.git
cd quantick
cargo build --workspace
cargo test --workspace
```

## Workflow

Every change follows the same loop — including changes by the maintainer:

1. **Start from an issue.** Pick one from the current milestone (issues labeled `good first issue` are a great entry point), or open a new one first. Comment on the issue so work isn't duplicated.
2. **Branch** off `main`: `feat/<desc>`, `fix/<desc>` or `docs/<desc>`.
3. **Engine code is test-first.** Write the fixture trades and expected bars before the implementation, then implement until green.
4. **Run the verification loop** (below) locally.
5. **Open a PR** that references the issue (`Closes #N`). CI runs the same checks; a PR with red CI is never merged.

## Verification loop (mandatory)

All four must pass before every commit — no exceptions:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo build --workspace
cargo test --workspace
```

There is no `-D warnings` on that clippy line, and its absence is deliberate.
The lint levels live in `[workspace.lints]` in the root `Cargo.toml`, which
every crate inherits, so a warning is an error for every cargo command —
`cargo check -p <crate>` included — and `cargo clippy -p <crate>` on your
machine fails on exactly what CI fails on. While the flag lived on the command
line, a clean local run and a red CI could come out of the same code.

A lint that starts firing gets fixed, never allow-ed. If one genuinely cannot
be fixed where it surfaced, it is recorded in that lints table with a reason
and a link to the follow-up that removes it.

## Commit style

Conventional style, imperative mood, English: `feat: ...`, `fix: ...`, `docs: ...`, `test: ...`.

## Design rules

These are non-negotiable; PRs that break them won't be merged (see `CLAUDE.md` for the full list):

- **Determinism.** Same trades in → same bars out, always. No wall-clock time, randomness or iteration-order-dependent output inside the engine.
- **One engine, three consumers.** Chart, backtest and bot consume the same aggregator code path — never fork bar-building logic per consumer.
- **Data honesty.** Inferred or incomplete data is labeled as such, never silently patched.
- **Small and focused.** This is not a trading platform. Build bars, show bars, expose bars to code.
