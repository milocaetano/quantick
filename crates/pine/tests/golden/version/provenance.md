# Version fixture provenance

These manually calculated expectations follow TradingView's Pine v6 migration guide:
https://www.tradingview.com/pine-script-docs/migration-guides/to-pine-version-6/
Retrieved 2026-11-08. The fixture sources are original Quantick tests.
The v5 lazy-call column deliberately preserves Quantick's eager stateful advancement.
Integer payloads remain f64; no exact i64 parity is claimed. PineTS 0.13.1 simple-int
numeric results differ from the primary reference; these primary-reference expectations
do not rewrite or replace the existing oracle artifacts.

The explicit bool/na join column follows the v6 type-system statement that
bool-returning expressions normalize unavailable values to false:
https://www.tradingview.com/pine-script-docs/language/type-system/#bool
This is a primary-document interpretation, not a verified TradingView compiler
result. The existing v5 missing-bool result is retained in the paired fixture.
