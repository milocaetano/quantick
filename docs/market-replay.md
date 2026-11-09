# Market Replay

Replay a recorded session and watch the chart build it print by print, at up to
50× speed. Open it from **File → Market Replay…** (`Ctrl+R`), pick the folder
holding your sessions, choose one and press **Play session**. A transport bar
appears at the bottom of the window while a recording plays — and only then:

```
⏮ ⏸  REPLAY  WINJ26 · 2026-03-16  13:27:48   1× 2× 3× 5× 10× 20× 50×   57 650 / 86 313 prints  ✖
▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬●────────────
```

`Space` toggles play/pause. Dragging or clicking the track seeks; the chart is
rebuilt from the new position, because bars that already closed cannot be
reopened. Idle stretches longer than two seconds of market time are skipped —
lunch lulls and halts do not have to be watched in real time. Playback drives
the chart through the same feed channel a live venue uses, so bars, navigation
and metrics behave exactly as they do live, and nothing is ever labelled "live"
while a recording is playing.

Replay streams trades only. Depth is not in the file, so the L2 heatmap and
aggression bubbles are unavailable during a replay and disable themselves.

## Session file format

One folder per instrument, one CSV per session day:

```text
<replay folder>/
    WINJ26/
        20260316.csv
        20260317.csv
```

Files named `SYMBOL-YYYYMMDD.csv` directly in the folder are read too. Each file
holds one executed trade per line, oldest first, preceded by `#` metadata:

```text
# quantick-replay 1
# symbol=WINJ26
# timezone=-03:00
# side_source=bid_ask
Date,Time,Price,Bid,Ask,Volume,Side
2026-03-16,10:01:08.000,182035,182030,182035,12,B
2026-03-16,10:01:09.250,182025,182025,182030,4,S
```

- **Required columns:** `Date`, `Time`, `Price`, `Volume`, `Side`.
- **Optional columns:** `Bid`, `Ask`, `Id`. Any other column is ignored and
  reported, so a file carrying extra data still loads.
- Columns are matched **by name**, so their order is free.
- `Date` is `YYYY-MM-DD`; `Time` is `HH:MM:SS` or `HH:MM:SS.mmm`, in the
  timezone declared by `# timezone=` (UTC when the line is absent).
- `Side` is the **aggressor**: `B` when the buyer lifted the offer, `S` when the
  seller hit the bid. `A`/`ASK`/`BID` are rejected by name — those spellings mean
  opposite sides at different vendors, and guessing would mirror the order flow.

The layout follows what replay-capable platforms already do (an instrument/day
folder tree, as in NinjaTrader's `db/replay/`), with the row format every
tick-data vendor ships. A folder that does not load says exactly why, per file,
with the fix and the whole format one click away in the browser.

## Importing recordings

MT5 order-flow recordings (NDJSON with `tick` and `book_update` events) convert
to sessions with:

```bash
cargo run -p quantick-replay --example import_mt5_ndjson -- \
    --in runtime/mt5_orderflow_20260316.ndjson \
    --out /path/to/replay
```

| Option | Default | Behavior |
| --- | --- | --- |
| `--in <file>` | — | Recording to read; repeat for several |
| `--out <folder>` | — | Replay folder to write into |
| `--symbol <name>` | all | Keep only this instrument |
| `--side-source <policy>` | `bid_ask` | `bid_ask` (position against the recorded quote, tick rule as fallback), `flags` (the broker's BUY/SELL tick flags) or `tick_rule` |
| `--spread-within-second` | off | Even out prints inside each recorded second |
| `--timezone <offset>` | from the data | Offset the rows are written in |

The run reports how often each rule decided the aggressor side — including how
often it disagreed with the broker's flags — and the policy is written into the
session's `# side_source=` line. Recordings that stamp trades to the second are
labelled `# time_resolution=1s`; `--spread-within-second` distributes prints
evenly across each second for smoother playback and marks the session
`1s_interpolated`, because those sub-second times are inferred.

## Replay environment variables

| Variable | Default | Behavior |
| --- | --- | --- |
| `QUANTICK_REPLAY_DIR` | the folder you last chose, else `Documents/Quantick/replay` | Folder the browser opens on, **for this run only** — what you pick in the app is stored in the workspace and is not overwritten by a run under this variable |
| `QUANTICK_REPLAY_AUTOSTART` | unset | Set to `1` to load and play the first session in that folder on startup (same code path as clicking **Play session**). Automation hook: read only by a `--features harness` build; a default build logs it as `UNKNOWN_HOOK` |
| `QUANTICK_REPLAY_SPEED` | `1` | Speed the autostarted session opens at (`--features harness` only, like the autostart) |
