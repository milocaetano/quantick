# Quantick score skill verification

## Structure

- Canonical workflow: `.claude/skills/quantick-score/SKILL.md`
- Versioned rubric: `docs/quality/quantick-score-rubric.md`
- Codex adapter: `.agents/skills/quantick-score/SKILL.md`
- Both skill directories pass the `skill-creator` `quick_validate.py` validator.
- The adapter resolves to the canonical workflow and the shared Codex compatibility mapping.
- Rubric v1.0 contains 20 five-point criteria: 10 sustainable-engineering, 5 agentic-development and 5 AI-operable-product criteria.
- The context-budget guard passes without raising its budget; detailed scoring rules live outside automatic agent context and are loaded only for an assessment.

## Workspace checks

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets`: exit 0.
- `cargo build --workspace`: exit 0.
- `RUST_TEST_THREADS=1 cargo test --workspace`: exit 0.

Two prior default-parallel test runs failed only the existing
`observer_core_capture_stays_within_the_ui_budget` timing test at 276 and 253
microseconds against its 250-microsecond median budget. The correctly filtered
standalone test passed at 94–97 microseconds, and the full single-thread run
passed. The PR CI remains the standard parallel-environment verification.

## Performance impact

The changed paths contain Markdown instructions loaded only when an assessment
is explicitly invoked. Their rate is rare. They add no per-trade, per-depth,
per-frame, transport or application-runtime work and allocate no runtime state.

## Scope

The assessment is read-only. It does not replace `ai-review`, mutate GitHub,
edit the assessed revision or award points for planned work.
