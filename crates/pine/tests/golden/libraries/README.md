# Library fixture provenance

These independently authored MIT fixtures use hand-calculated values, not
PineTS output. The local resolver receives `local/Stats/1` as an exact key.
Each closed bar adds its close to the persistent Point and increments count.
Repeated previews roll back both fields. Expected totals: 2, 5, 10.

Primary semantics references:
https://www.tradingview.com/pine-script-docs/language/objects/
https://www.tradingview.com/pine-script-docs/language/methods/
https://www.tradingview.com/pine-script-docs/concepts/libraries/

C61 also rejects missing, cyclic and remote imports deterministically, and v6
field-history expressions while accepting object history and temporary series.
C90-C93 combine offline goldens, deterministic bytes, app snapshots and report
contracts after the retained feature children integrate. Oracle tolerances and
explicit divergences remain those committed by their respective owners.
