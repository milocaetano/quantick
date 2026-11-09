# Optional L2 liquidity map

The chart can capture Binance Spot, Hyperliquid perpetual and MetaTrader DOM
depth and render a Bookmap-inspired liquidity map. Capture starts with any feed that can stream
depth; the map itself is **hidden by default** and shown from the chart
controls. The **⚙ L2** button docks a settings panel to
the left of the chart — the chart keeps the remaining width instead of being
covered — including a deterministic preview of every visual state, so themes
and grouping can be tuned without waiting for a rare live-book event.

The order flow splits into two independent layers, each with its own toolbar
switch and its own docked panel:

- **book heatmap** (**⚙ L2**) — resting liquidity and liquidity-response
  markers, built from the synchronized L2 depth pipeline.
- **bubbles** (**⚙ bubbles**) — confirmed aggressive executions, built from the
  aggregate-trade stream the chart already consumes.

Neither switch touches the other. Bubbles render with the map hidden (and for
providers without a depth pipeline at all), and hiding the book leaves the
bubbles running.

Both switches are display-only. Recording and drawing are separate concerns:
the depth recorder runs from the moment a depth-capable feed starts, so hiding
the map never punches a hole in the recorded book — reopen it and the whole
retained window is there, without the gap that stopping capture would leave.
While the map is hidden no projection is built and no depth geometry is drawn,
so the only cost it carries is the recording itself, bounded by the same
retention budget documented below. Capture stops only when the market changes:
a feed or symbol switch, or a replay taking the chart over.

The visualization follows a few data-honesty rules:

- History begins at the first successfully synchronized live snapshot/update sequence. Neither Binance nor Hyperliquid provides historical L2 backfill through these feeds, so candles before that point are marked as unavailable instead of being reconstructed. That stretch is marked by a single dashed line where the book begins, plus a label: it can span most of the chart, and tinting it would bury candles and bubbles that are perfectly real there.
- Depth update IDs are checked continuously. A disconnect, sequence gap or resynchronization closes the current liquidity runs, marks the affected interval with a faint fill between dashed vertical boundaries, and starts again from a fresh snapshot. Stale book state is never stretched across a gap. Interior gaps keep their fill precisely because they are narrow — an untinted sliver would read as "no resting liquidity" rather than "no data".
- Heatmap quantities are resting bid/ask amounts from the snapshot plus absolute depth updates, limited to the source's declared coverage. Hyperliquid publishes complete visible images of at most 20 levels per side; Binance's requested depth is configurable. Liquidity outside either coverage is unknown.
- A displayed band records the bucket total observed when its run opened; changes smaller than ~10% merge into the open run instead of cutting a new one. This churn-merge tolerance is a disclosed granularity choice — larger moves, appearances and full removals always cut a new run at their exact value.
- Trade bubbles are confirmed market aggression (`aggTrade` on Binance and venue-side `trades` on Hyperliquid). Their area is quantity-proportional and nearby prints can be clustered without changing total volume.
- A dark **bite** and impact ring mean an aggression and an L2 reduction were compatible in passive side, displayed price range, synchronized generation and time. This is an association, not a claim that one event caused the other.
- A violet dashed tail means displayed liquidity decreased without compatible aggression. It is deliberately called an **unattributed L2 reduction**, not a cancellation: a depth reduction can be an execution, cancellation, replacement or a combination of those events.
- Window clipping, retention boundaries, snapshots and synchronization gaps never create fake reduction markers.
- Captured history is bounded and kept in memory only. Restarting the application starts a new capture.

The chart exposes these settings:

| Setting | Default | Range / behavior |
| --- | ---: | --- |
| L2 heatmap | Hidden | Display-only; the recorder runs whenever the feed can stream depth, so reopening the map shows the whole retained window |
| Retention | 30 minutes | 1–1,440 minutes |
| Display range | Auto / 128 rows | Native, `2×`, `5×`, `10×`, `25×`, `50×`, custom multiple or adaptive-to-zoom; changing it reprojects immediately without resetting history |
| Base capture bucket | auto from price | Sized to ~`price / 65000` (1/2/5·10^k) on the first snapshot; any positive value can be set by hand. Changing the base resolution requires a fresh snapshot and resets retained L2 history |
| Theme | Bookmap | Bookmap, High contrast or Color blind |
| Brightness | `0.9` | `0.05`–`1.0` |
| Quiet liquidity curve | `1.8` | `0.25`–`2.0`; above one sinks quiet liquidity into the dark canvas so only real walls glow |
| Intensity scale | Visible P99 | Automatic visible-window P99 or a fixed full-intensity quantity |
| Aggression bubbles | Off | Own toolbar switch and own panel; records and projects without L2 depth capture |
| Bubble clustering | 200 ms | Raw, 50, 100, 200 or 500 ms |
| Bubble opacity | `0.78` | `0.05`–`1.0` |
| Largest bubble | `15 px` | `4`–`48 px`; area stays quantity-proportional |
| Liquidity response | On / 250 ms | Bite or withdrawal-tail markers with a configurable 25–1,000 ms evidence window |
| Legend | On | Explains liquidity brightness, buy/sell aggression, aligned depletion, unattributed reduction and L2 gaps |

The in-memory safety budgets are 500,000 liquidity runs (approximately 64 MiB), 100,000 aggression records, 12,000 projected visible cells/events and 700 projected bubbles. Old history is pruned and excess render primitives are dropped within those limits; the associated counters are emitted in diagnostic logs. All book state lives on a dedicated thread: the UI forwards depth events and latest-wins projection requests through a channel and only draws the newest published frame, so a dense-book projection can never block a frame. Projections rebuild at the ~220 ms depth cadence, are regrouped in a deterministic sweep and submitted in batched meshes. The paint order is `gap → heatmap → liquidity response → candle → aggression → legend`.

## L2 and logging environment variables

| Variable | Default | Behavior |
| --- | --- | --- |
| `QUANTICK_BOOK_DEPTH` | `1000` | Binance snapshot depth per side. Numeric values are clamped to `1`–`5000`; a missing or invalid value uses the default. Hyperliquid's public L2 coverage is fixed by the venue at up to 20 levels per side. |
| `QUANTICK_BOOK_AUTOSTART` | unset | Set to `1` to show the L2 map on startup without clicking the chart toggle (development/ops convenience; same code path as the toggle; read only by a `--features harness` build). Capture itself needs no flag — it runs for every depth-capable feed. |
| `QUANTICK_LOG_FORMAT` | `text` | Set to `json` for newline-delimited JSON diagnostic logs on stderr. |
| `RUST_LOG` | `quantick=info` | Standard tracing filter; for example, use `quantick=debug` for deeper diagnostics. |

JSON logs include stable fields such as `schema_version`, `event_code`, symbol, connection generation, update IDs, recovery action and health counters so synchronization and coverage gaps can be investigated without inferring state from prose.
