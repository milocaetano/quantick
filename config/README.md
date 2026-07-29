# config/

Project configuration that is **tracked in git**. quantick reads these from the
working directory it runs in (the repository root, for `cargo run`).

| File | What it is | Tracked? |
|---|---|---|
| `config/bubbles.toml` | Named looks for the **aggression bubbles** panel (bubble size, the consumption mark, the trail, colours, labels). Written by the panel's `save` button; safe to edit by hand. | yes — this is the point |
| `crates/app/config/feeds.toml` | Built-in feed/symbol list, compiled into the binary as the fallback. | yes |
| `crates/app/config/bubbles.toml` | Built-in bubble presets, compiled in as the fallback. | yes |
| `./quantick.toml` | **Local** feed/symbol override for one machine (which broker contracts your terminal really has). | no — gitignored |

Rule of thumb: anything that describes *how the project reads the market* is
tracked here, so it can be reviewed and rolled back like code. Anything that
describes *this machine* stays out of git (`quantick.toml`).

## Overriding a path

- `QUANTICK_CONFIG=/path/to/feeds.toml` — feeds and symbols.
- `QUANTICK_BUBBLES=/path/to/bubbles.toml` — bubble presets.

Both fall back to the file in the working directory, then to the copy compiled
into the binary. A missing file is fine; a malformed feed config is a hard error
(a bad config must never be guessed at), while a malformed presets file only
falls back to the built-in presets and reports the error in the panel — losing
the chart over a bad colour triple would be the worse failure.

## bubbles.toml

```toml
active = "default"        # the preset the panel opens on ("" = none)

[[presets]]
name = "default"
cluster_ms = 200          # merge compatible prints inside this window
candle_summary = false    # fold each closed bar into one two-sided pie bubble

[presets.bubbles]         # everything visual; every key is optional
max_radius = 15.0
render_mode = "sphere"    # "flat" (classic disc) or "sphere" (shaded 3D ball);
sphere_shading = 0.6      # spheres keep overlapping prints readable as separate
sphere_highlight = 0.4    # bubbles on a dense tape
side_offset = 3.5         # buys nudged up, sells down, so both sides are readable
front_width = 3.0         # the vertical consumption mark ("risco")
trail_length = 18.0       # the glow into the consumed side ("rastro")
buy_color = [46, 224, 150]  # omit to follow the chart theme

[presets.live_lane]       # the rolling tape pinned to the chart's right edge
width_share = 0.35        # share of the chart, up to 0.5; candle zoom never changes it
time_zoom = 1.0           # 1x = one typical bar of market time in the band
cluster_ms = 100          # omit to use the same window history uses
radius_scale = 1.7        # the lane has room to spare, so it can draw bigger
show_marks = true         # the lane boundary and the live-edge line
```

The live lane is where prints arrive in real time: a band pinned to the right
edge of the chart, showing a fixed window of market time that always ends at
now. A print enters at the right edge, slides left at a fixed pixels-per-ms
rate, and leaves through the left edge into the slot of the bar it happened in.
It belongs to the tape, not to the forming bar, so a bar closing empties nothing
and restarts nothing.

It is a **pane of its own**: the candles pan and zoom in the space left of the
divider, and nothing they do moves, shrinks or empties the tape — the most
recent prints are on screen whatever the rest of the chart is showing. Both of
its knobs are also on the chart itself: drag the divider to resize the band
(the pointer becomes a resize cursor over it), and drag or scroll the bottom
time strip *under the band* to zoom its window. Zooming out shows more market
time in the same width and scales `cluster_ms` with it, so a cluster keeps a
constant width on screen — the crowd gathers into fewer, bigger bubbles instead
of piling up.

Because history is compressed into one slot per bar and the lane is not, the two
regions can be tuned apart — and `candle_summary` is the setting that leans into
that, trading a closed bar's intra-bar detail for one readable mark per price
range.

A preset only describes how bubbles **look**. Turning the layer on stays a live
decision in the panel, so having this file can never start capture by itself.
Quantities (`min_quantity`, `size_reference_quantity`) are in the symbol's own
units — contracts on the mini index, coins on Binance — so a size-based preset
says which market it was built for.
