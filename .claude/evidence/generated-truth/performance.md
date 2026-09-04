# Performance impact, declared (G3)

Every path this branch touches, classified by how often it runs. The rate is
the judgement, not the line count: a cheap thing per frame is expensive and an
expensive thing once is not.

| Path | Rate | Cost | Verdict |
| --- | --- | --- | --- |
| `hooks::log_unknown_hooks` | **once, at startup** | one pass over the process environment (tens of entries), each checked against a 126-entry `BTreeSet` and a 3-entry slice | Immeasurable. Runs after the tracing subscriber and before the config read, on a thread that is not yet drawing. |
| `hooks::hook_registry_markdown`, `control::inventory::capability_inventory_markdown` | **offline only** | builds an `ObserverContract`, renders ~70 KB | Never on a launch path. Reached solely by `--dump-*`, which returns before `init_tracing`. |
| `crates/guards` `generated` check | **developer loop / CI** | reads ~330 files under `crates/app/src` plus three documents | Whole guards suite 0.31 s; the three timed runs in `guards-timing.log` are 725–791 ms end to end including cargo's own overhead. |
| `declare_hooks!` expansions | **compile time** | 126 `&'static str` in `.rodata` across 37 slices | No runtime cost. Nothing walks them except the startup pass above. |
| The two `crates/app` byte-comparison tests | **test only** | reads two committed files, renders both | Inside `#[cfg(test)]`; not in the shipped binary. |

**Nothing on a hot path.** No code in this branch executes per trade, per depth
update or per frame. The engine, the aggregators, the chart layers and the feed
loops are untouched — `git diff origin/main...HEAD --stat` names no file under
`crates/engine`, `crates/orderbook`, `crates/trading` or any `feed-*` crate.

That is why the hot-path evidence gate (`APP_HEALTH_SUMMARY` against a `main`
control run) is not injected: there is no path whose rate would make the
measurement mean anything. The classification above is the evidence that this
was established rather than assumed, which is what the gate actually asks for.

The one runtime addition, `log_unknown_hooks`, was placed deliberately: it must
run after the subscriber exists, or its warning has nowhere to go, and before
anything opens, so a mistyped hook is the first thing the run says instead of
something inferred later from a surface that never appeared.
