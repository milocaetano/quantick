# Read-cost ledger fixture

Two invented rows in the shape `docs/quality/read-cost/ledger.md` uses, so the
reuse of `tools/read_cost/ledger.py`'s parser is tested without depending on
the real ledger, which grows a row on every merge.

| PR | Date | Branch | Base | Read cost | Changed | Top referenced |
| ---: | --- | --- | --- | ---: | ---: | --- |
| 9001 | 2026-01-01 | feat/fixture-alpha | main | 17251 | 14 | `crates/app/src/state.rs` (663) |
| 9002 | 2026-01-02 | feat/fixture-beta | main | 0 | 0 | — |
