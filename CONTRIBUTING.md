# Contributing to quantick

Thanks for your interest! The whole point of this project is to open up tooling that has historically been private, so ideas, questions and code are all welcome.

- **Ideas, questions, design discussion** → [Discussions](https://github.com/milocaetano/quantick/discussions)
- **Actionable work** (bugs, features with a defined scope) → [Issues](https://github.com/milocaetano/quantick/issues)

## Getting started

You need a stable Rust toolchain ([rustup](https://rustup.rs/)).

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
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

## Commit style

Conventional style, imperative mood, English: `feat: ...`, `fix: ...`, `docs: ...`, `test: ...`.

## Design rules

These are non-negotiable; PRs that break them won't be merged (see `CLAUDE.md` for the full list):

- **Determinism.** Same trades in → same bars out, always. No wall-clock time, randomness or iteration-order-dependent output inside the engine.
- **One engine, three consumers.** Chart, backtest and bot consume the same aggregator code path — never fork bar-building logic per consumer.
- **Data honesty.** Inferred or incomplete data is labeled as such, never silently patched.
- **Small and focused.** This is not a trading platform. Build bars, show bars, expose bars to code.
