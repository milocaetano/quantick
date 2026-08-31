# Control plane capability inventory

**Status:** PR 0 migration baseline

**Measured at commit:** `fcd2ac400ed5e6362595246120b93dfb08fdc7ab`

**Date:** 2026-08-19

This inventory starts from the control surfaces that already exist instead of
guessing what Quantick can do. It is not the future runtime registry and must
not become a hand-maintained second copy of one.

## 1. Measurement

`crates/app/src` contains 88 distinct `QUANTICK_*` string literals:

- 86 production startup, configuration, validation, or action surfaces;
- 2 test-only store variables.

The earlier development-plan draft said 89. A source scan found 88, and the
two test-only names explain why even that number is not a count of production
capabilities. The corrected numbers are used below.

The baseline can be reproduced with a literal scan over Rust source followed
by unique sorting. Comments are not used to invent additional names; every
inventory row has a quoted string literal in `crates/app/src`.

## 2. Migration labels

| Label | Meaning |
| --- | --- |
| `retain` | Startup or path override remains; effective state becomes readable |
| `route` | Existing hook and UI must call one registered runtime handler |
| `fixture` | Deterministic demo remains for tests but is composed from registered primitives |
| `replace` | Structured snapshots or actions supersede the hook after harness migration |
| `test-only` | Excluded from the public capability registry |

Candidate IDs below follow the [control contract](control-contract.md). They
name the intended owner and prevent two modules from implementing the same
operation. PRs may refine an ID before it is released, but must update this
inventory and the reviewed schema together.

## 3. Bootstrap, configuration, and stores (22)

Primary owners: `config.rs`, `main.rs`, store modules, feed configuration, and
preset modules.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_CONFIG` | Select the startup feed configuration file | `system.config` snapshot scope | n/a | `retain` |
| `QUANTICK_DEFAULT_FEED` | Override the initial feed | `workspace.market.select` | `cockpit` | `route` |
| `QUANTICK_DEFAULT_SYMBOL` | Override the initial symbol | `workspace.market.select` | `cockpit` | `route` |
| `QUANTICK_WINDOW_SIZE` | Set the opening window dimensions | `window.size.set` | `cockpit` | `route` |
| `QUANTICK_LOG_FORMAT` | Select human or JSON startup logging | `system.logging` snapshot scope | n/a | `retain` |
| `QUANTICK_BACKFILL` | Configure initial history depth | `feed.history.policy` snapshot scope | n/a | `retain` |
| `QUANTICK_BOOK_DEPTH` | Configure Binance depth subscription size | `feed.orderbook.policy` snapshot scope | n/a | `retain` |
| `QUANTICK_UI_STATE` | Relocate the persisted workspace state | `workspace.store` snapshot scope | n/a | `retain` |
| `QUANTICK_REPLAY_DIR` | Select a replay folder for this run | `replay.library.folder.select` | `cockpit` | `route` |
| `QUANTICK_TRADES_DIR` | Select the paper journal folder | `paper.journal.folder.select` | `cockpit` | `route` |
| `QUANTICK_PAPER_STATE` | Relocate the paper settings sidecar | `paper.store` snapshot scope | n/a | `retain` |
| `QUANTICK_INDICATORS_DIR` | Relocate the Pine script library | `indicator.library` snapshot scope | n/a | `retain` |
| `QUANTICK_INDICATORS_STATE` | Relocate persisted active indicators | `indicator.store` snapshot scope | n/a | `retain` |
| `QUANTICK_INDICATOR_PRESETS` | Relocate indicator presets | `indicator.preset_store` snapshot scope | n/a | `retain` |
| `QUANTICK_DRAWING_PRESETS` | Relocate drawing presets | `drawing.preset_store` snapshot scope | n/a | `retain` |
| `QUANTICK_FOOTPRINT` | Override the tracked footprint configuration | `orderflow.footprint.config` snapshot scope | n/a | `retain` |
| `QUANTICK_FOOTPRINT_SETTINGS` | Relocate persisted footprint edits | `orderflow.footprint.store` snapshot scope | n/a | `retain` |
| `QUANTICK_FOOTPRINT_PRESETS` | Relocate footprint presets | `orderflow.footprint.preset_store` snapshot scope | n/a | `retain` |
| `QUANTICK_BUBBLES` | Override the bubble preset file | `orderflow.bubbles.config` snapshot scope | n/a | `retain` |
| `QUANTICK_CHART_LAYERS` | Relocate persisted chart-layer visibility | `chart.layer_store` snapshot scope | n/a | `retain` |
| `QUANTICK_STRATEGY_PRESETS` | Relocate the strategy preset bank | `strategy.preset_store` snapshot scope | n/a | `retain` |
| `QUANTICK_SYMBOLS` | Relocate the user-added symbol catalog | `feed.symbol_store` snapshot scope | n/a | `retain` |

Paths in these scopes are redacted by default. Retaining an environment
override does not make arbitrary environment reads a capability.

## 4. Workspace, chrome, and navigation (11)

Primary owner: `app.rs`, with state persisted through `ui_state.rs` and
`workspace_bundle.rs`.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_DOCK_TAB` | Activate a dock tab | `workspace.dock_tab.activate` | `cockpit` | `route` |
| `QUANTICK_MENU` | Open the Workspace menu for validation | `ui.menu.open` | `cockpit` | `replace` |
| `QUANTICK_LAYOUT` | Select flow, time, or split layout | `workspace.layout.set` | `cockpit` | `route` |
| `QUANTICK_STYLE_PANEL` | Open chart appearance settings | `ui.dialog.open` with `chart.appearance` | `cockpit` | `replace` |
| `QUANTICK_SOURCE_PICKER` | Open the market dialog | `ui.dialog.open` with `workspace.market` | `cockpit` | `replace` |
| `QUANTICK_INDICATOR_PREVIEW` | Stage the unapplied-draft watermark | `ui.overlay.set` with `chart.indicator_preview` | `cockpit` | `replace` |
| `QUANTICK_TOOL_FAVORITES` | Set drawing-tool favorites | `drawing.tool_favorites.set` | `cockpit` | `route` |
| `QUANTICK_TOOLBOX_DOCK` | Move the drawing rail | `drawing.tool_rail.dock` | `cockpit` | `route` |
| `QUANTICK_TOOLBAR_SCROLL` | Move the drawing-tool band | `drawing.tool_rail.scroll` | `cockpit` | `route` |
| `QUANTICK_TOOLBOX_FLYOUT` | Open one tool family | `drawing.tool_family.open` | `cockpit` | `replace` |
| `QUANTICK_WORKSPACE_SAVE` | Save the current workspace | `workspace.save` | `cockpit` | `route` |
| `QUANTICK_WORKSPACE_EXPORT` | Export a workspace bundle to a supplied path | `workspace.export` | `cockpit` | `route` |
| `QUANTICK_WORKSPACE_IMPORT` | Replace the cockpit from a bundle | `workspace.import` | `cockpit` | `route` |

Import and export also declare `filesystem_read` or `filesystem_write` risk and
are not part of the observer profile.

## 5. Chart, viewport, and history (8)

Primary owners: `app.rs`, `tab.rs`, `pane.rs`, `viewport.rs`, and feed history
services.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_CANDLE_WIDTH` | Set pixels per bar through the zoom path | `chart.viewport.bar_width.set` | `cockpit` | `route` |
| `QUANTICK_PAN_PX` | Pan through the same gesture path as the chart | `chart.viewport.pan` | `cockpit` | `route` |
| `QUANTICK_INVERTED` | Invert the price axis on chart panes | `chart.price_axis.inverted.set` | `cockpit` | `route` |
| `QUANTICK_LIVE_STRIP_AUTOSTART` | Enable the live order-flow strip | `chart.layer.visibility.set` | `cockpit` | `route` |
| `QUANTICK_LOAD_OLDER` | Request older feed history by page | `feed.history.load_older` | `cockpit` | `route` |
| `QUANTICK_PROGRESSIVE_HISTORY` | Set progressive history delivery | `feed.history.progressive.set` | `cockpit` | `route` |
| `QUANTICK_VENUE_HISTORY_DEMO` | Inject deterministic complete or partial venue history | `feed.history.fixture.publish` | n/a | `fixture` |
| `QUANTICK_CONTEXT_MENU` | Open chart, tape, or axis context menus | `ui.context_menu.open` | `cockpit` | `replace` |

The observer equivalents are `chart.window.read`, viewport fields in the chart
snapshot, feed capability state, and the semantic scene. Read operations never
call the setters above.

## 6. Order flow and L2 (8)

Primary owners: `app.rs`, `footprint_config.rs`, `footprint_render.rs`,
`orderflow`, and the chart-layer registry.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_BOOK_AUTOSTART` | Enable the chart heatmap | `chart.layer.visibility.set` | `cockpit` | `route` |
| `QUANTICK_BUBBLES_AUTOSTART` | Enable aggression bubbles and live flow | `chart.layer.visibility.set` | `cockpit` | `route` |
| `QUANTICK_FOOTPRINT_AUTOSTART` | Enable candle footprints | `chart.layer.visibility.set` | `cockpit` | `route` |
| `QUANTICK_FOOTPRINT_DEBUG` | Append footprint decision inputs to the legend | `orderflow.footprint.diagnostics` snapshot scope | `observe` | `replace` |
| `QUANTICK_FOOTPRINT_PANEL` | Open footprint settings | `ui.dialog.open` with `orderflow.footprint` | `cockpit` | `replace` |
| `QUANTICK_TAPE` | Show or hide the tape pane | `orderflow.tape.visibility.set` | `cockpit` | `route` |
| `QUANTICK_TAPE_LAYERS` | Set tape heatmap and bubble visibility | `orderflow.tape.layers.set` | `cockpit` | `route` |
| `QUANTICK_TAPE_WINDOW` | Set the tape's visible market-time window | `orderflow.tape.window.set` | `cockpit` | `route` |

## 7. Indicators and analytical drawings (7)

Primary owners: `app.rs`, `app::indicators`, `IndicatorHost`, and drawing tools
that compute AVWAP or fixed-range volume profiles.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_INDICATORS_AUTOSTART` | Attach deterministic built-in indicators | `indicator.attach` | `annotate` | `route` |
| `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART` | Attach named Pine library scripts | `indicator.script.attach` | `annotate` | `route` |
| `QUANTICK_INDICATOR_SETTINGS` | Open an indicator inputs or style dialog | `ui.dialog.open` with `indicator.settings` | `cockpit` | `replace` |
| `QUANTICK_LEGEND_COLLAPSED` | Collapse the focused pane legend | `indicator.legend.collapsed.set` | `cockpit` | `route` |
| `QUANTICK_AVWAP_DEMO` | Place a deterministic anchored VWAP | `drawing.create` with `anchored-vwap` | n/a | `fixture` |
| `QUANTICK_FRVP_DEMO` | Place deterministic fixed-range volume profiles | `drawing.create` with `fixed-range-volume-profile` | n/a | `fixture` |
| `QUANTICK_FRVP_DEMO_SELECT` | Select the fixed-range profile fixture | `drawing.selection.set` | n/a | `fixture` |

`indicator.script.compile` returns Pine diagnostics without attaching anything
and has `observe` effect. `indicator.script.attach` is additive, reversible,
and belongs to `annotate`.

## 8. Drawings and attention surfaces (14)

Primary owners: `app.rs`, `drawings`, `toolrail`, and `pane`.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_DRAWING_TOOL` | Arm a registered drawing tool | `drawing.tool.arm` | `cockpit` | `route` |
| `QUANTICK_DRAWING_MAGNET` | Enable OHLC snapping | `drawing.magnet.set` | `cockpit` | `route` |
| `QUANTICK_DRAWING_DRAFT` | Stage a partially placed drawing | `drawing.draft.begin` | n/a | `fixture` |
| `QUANTICK_DRAWING_CONSTRAIN` | Hold the draft's constraint modifier | `drawing.draft.constraint.set` | n/a | `fixture` |
| `QUANTICK_DRAWINGS_DEMO` | Place one fixture for every registered tool | `drawing.fixture.populate` | n/a | `fixture` |
| `QUANTICK_DRAWINGS_DEMO_SHARED` | Make demo drawings visible across charts | `drawing.visibility_scope.set` | n/a | `fixture` |
| `QUANTICK_DRAWINGS_DEMO_RECUT` | Re-cut bars under drawing fixtures | `chart.bar_spec.set` plus fixture data | n/a | `fixture` |
| `QUANTICK_DRAWINGS_DEMO_SELECT` | Select and center a fixture by tool ID | `drawing.selection.set` | n/a | `fixture` |
| `QUANTICK_DRAWINGS_MANAGER` | Open the object manager | `ui.dialog.open` with `drawing.manager` | `cockpit` | `replace` |
| `QUANTICK_DRAWING_INSPECTOR` | Open the selected drawing's properties | `ui.dialog.open` with `drawing.inspector` | `cockpit` | `replace` |
| `QUANTICK_DRAWING_INSPECTOR_TAB` | Select an inspector tab | `drawing.inspector.tab.set` | `cockpit` | `replace` |
| `QUANTICK_DRAWING_INSPECTOR_POS` | Park the inspector window | `drawing.inspector.position.set` | `cockpit` | `route` |
| `QUANTICK_CONTEXT_BAR_POS` | Park the selected object's context bar | `drawing.context_bar.position.set` | `cockpit` | `route` |
| `QUANTICK_TEXT_NOTE` | Place a text drawing and enter inline edit | `drawing.create` with `text` | `annotate` | `route` |

The new attention capabilities do not come from existing hooks:
`attention.cursor`, `attention.selection`, and human-created mark events are
read scopes. Remote `attention.mark.create` is a separate annotate action.

## 9. Replay (5)

Primary owners: `app.rs`, `replay_view.rs`, and the replay feed adapter.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_REPLAY_AUTOSTART` | Start the selected recorded session | `replay.start` | `cockpit` | `route` |
| `QUANTICK_REPLAY_SPEED` | Set playback speed at startup | `replay.speed.set` | `cockpit` | `route` |
| `QUANTICK_REPLAY_BROWSER` | Open the replay browser | `ui.dialog.open` with `replay.browser` | `cockpit` | `replace` |
| `QUANTICK_REPLAY_GET_DATA` | Open and seed the Get data flow | `replay.data_request.open` | `cockpit` | `route` |
| `QUANTICK_REPLAY_RESTART_AFTER` | Restart after a deterministic trade count | `replay.restart` | n/a | `fixture` |

Non-observe agent actions during replay follow the durable control-trace rule in
the [control contract](control-contract.md#11-replay-determinism-decision).

## 10. Paper trading, reports, and strategies (11)

Primary owners: `paper_trading.rs`, `app.rs`, `sim`, and `strategy`.

| Existing surface | Current purpose | Candidate registry target | Remote effect | Migration |
| --- | --- | --- | --- | --- |
| `QUANTICK_PAPER_DEMO` | Run deterministic simulated trades | `paper.fixture.run` | n/a | `fixture` |
| `QUANTICK_CMD_PREVIEW` | Paint a buy or sell command preview without placing | `paper.command.preview` | `annotate` | `route` |
| `QUANTICK_PAPER_ORDER_HOVER` | Force resting-order tags into hover detail | `paper.order.scene` snapshot scope | n/a | `fixture` |
| `QUANTICK_PAPER_ORDERS` | Rest deterministic orders around the mark | `paper.order.place` | `paper` | `fixture` |
| `QUANTICK_PAPER_REPORT_AUTOSTART` | Open simulated performance | `ui.dialog.open` with `paper.performance` | `cockpit` | `replace` |
| `QUANTICK_PAPER_CALENDAR` | Open and select report calendar periods | `paper.report.period.set` | `cockpit` | `route` |
| `QUANTICK_PAPER_REPORT_LIST` | Collapse or expand the report trade list | `paper.report.trade_list.set` | `cockpit` | `route` |
| `QUANTICK_LEDGER_SCOPE` | Select chart, all, or one symbol in the ledger | `paper.ledger.scope.set` | `cockpit` | `route` |
| `QUANTICK_LEDGER_PAGES` | Reveal older ledger pages | `paper.ledger.load_older` | `cockpit` | `route` |
| `QUANTICK_LEDGER_FOLD` | Fold every civil day in the ledger | `paper.ledger.days.collapse` | `cockpit` | `route` |
| `QUANTICK_STRATEGY_DEMO` | Place a region and stage or arm a strategy | `strategy.arm` | `paper` | `fixture` |

Fixtures that place orders or arm strategies are never exposed to an observer.
They must reuse the same simulator and registry handlers as the corresponding
human and future agent actions.

## 11. Test-only variables (2)

| Existing surface | Source | Disposition |
| --- | --- | --- |
| `QUANTICK_FAKE_STORE` | `workspace_bundle.rs` test fixture | `test-only`; never register |
| `QUANTICK_TEST_STORE_HOME_ENV` | `store_home.rs` test fixture | `test-only`; never register |

## 12. Capabilities missing from the current hooks

The environment surfaces are mostly startup writes. They do not cover the
observation side of the control plane. The initial registry must add at least:

- system, build, instance, and effective-permission snapshots;
- workspace, feed, chart, indicator, drawing, order-flow, replay, paper, and
  health snapshots;
- visible chart-window pagination;
- semantic scene and stable control IDs;
- cursor, selection, and human mark observation;
- cursor-based semantic events and parked waits;
- evidence capture and chunked resources;
- capability search, availability, and structured errors;
- compile-only Pine diagnostics;
- connection and revocation state.

## 13. Migration rule

An existing production hook is not removed merely because a capability exists.
Migration is complete only when:

1. the UI and hook call the same named handler;
2. the handler has a registry descriptor and structured result;
3. the resulting state is readable through a snapshot or event;
4. actor and authority rules are enforced;
5. existing deterministic harness coverage passes through the handler;
6. at least one harness scenario uses the live control plane;
7. removal no longer reduces startup-only coverage.

The inventory is retired when the runtime registry can generate an equivalent
report and every retained hook is explicitly marked startup-only.

## References

- [UI harness catalog](../../.claude/skills/ui-harness/SKILL.md)
- [Development plan](../mcp-control-plane-development-plan.md)
- [Control contract](control-contract.md)
