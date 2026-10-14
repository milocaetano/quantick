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
active = "default"        # the look an undeclared asset opens on ("" = none)

[[presets]]
name = "default"
cluster_ms = 200          # merge compatible prints inside this window
candle_summary = false    # fold each bar into one two-sided pie bubble per price
                          # range — the forming one included, as a running total

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

### WIN tick-chart spheres

`B3 WIN tick spheres` is an opt-in, chart-only preset for the configured WIN
family. Choose it in the tick sphere picker, or apply it by name. It keeps the
current tape look, grouping, scale, window and book settings. The old
`mini index regions` preset and the WIN default remain unchanged. Saving uses
the existing per-asset save switch; no other asset is selected or restyled.

Its `tick_only = true` scope applies only `[presets.tick_bubbles]`. Quantities
are contracts. Native 100 ms price cells enter the existing causal tape merger
in market-time coordinates, then each group is placed at its exact quantity-weighted
candle position. The declared grouping geometry is 72 seconds over 420 logical
points, 0.8 price units per point (a 5-point WIN tick spans 6.25 logical points),
and an 18-point grouping radius. These settings model the observed WIN tape
geometry; they never change the actual tape. The 20,000-contract fixed
reference is close to the measured median volume of a 2,000-tick WIN
candle (19,063 contracts in the calibration recording). The 18-point reference
radius gives 100, 1,000 and 5,850 contracts radii of about 1.3, 4.0 and 9.7
points. Small quantities have no inflated minimum radius.
The optional `volume_opacity_floor = 0.25` makes small WIN groups quieter:
fill opacity rises linearly from 0.225 to 0.9 at 5,000 contracts (reference/4),
then stays at 0.9. This fixed quantity cue leaves radii, buy/sell sectors and
every member intact; it never depends on zoom or the largest visible group.
Previously saved styles and BTC omit this field and keep uniform opacity.
Below the existing 3.541-point detail threshold, WIN spheres retain both
quantity sectors as flat discs without the unreadable inner lighting ring.
Contract labels between 1K and 10K use one decimal, still subject to text-fit
and collision checks. Exact quantities remain available in structured readback.
The candle's existing body/wick contours receive a thin canvas-colored backing
only with this causal tick style. It adds at most 0.5 points per side and
shrinks for small bodies; it never fills an entire candle column, changes
direction colors or enables a hidden contour. This is visual occlusion of
underlying spheres, not an exclusion from volume accounting.
These settings do not change the bar rule. As in BTC, circle area represents gross quantity and
the two sectors preserve buy/sell quantities. Volumes above the reference earn
larger radii; entering or leaving the viewport never rescales other groups.
Grouping uses independent UTC-aligned 72-second blocks. A burst crossing a
block boundary remains two groups. A forming 100 ms cell is a preview; it
does not freeze partial quantities into retained groups. Incomplete source
admission also remains provisional. Late executions update their group's facts
without splitting unrelated retained groups. A cold reconstruction of out-of-order
source can group differently from its prior arrival history; the source ordinals
and quantities remain exact. This does not claim to recover the look-dependent
epoch of a previously running tape. Pan, candle zoom and window
resize preserve complete groups and apply only factual coordinate transforms
and clipping. Whole blocks are admitted on the worker before the viewport clip;
source progress and all clipped categories remain readable. Raw execution
membership is retained. The native tape has its own unchanged rules and memory.

Agents discover the choices in `orderflow.bubbles.tick_presets` and apply
`orderflow.bubbles.tick.set` with stable tab/pane IDs and the exact name.
`orderflow.bubbles.preset.apply` also respects this preset's chart-only scope.
Read back `tick_bubbles.quantity_unit`, `region_window_ms`, `tape_bursts` and
`volume_opacity_floor`. FLOW's optional `tape_bursts` readback groups its
calibration, admitted time interval and native-cell watermark in one object.
FLOW also reports the admitted temporal context and fixed scale basis. Layer visibility
remains a separate choice. Source side provenance is unchanged: a feed using
tick-rule inference remains inferred, and recordings without L2 supply no book.

Tick sphere calibrations declare `tick_asset_keys` on the preset, matching the
resolved asset key from the feed configuration (for example `WIN*`). Both tick
pickers and chart-only preset application use this list. Add a configured family
and a preset entry to support another instrument; quantity denomination only
formats labels. An omitted list retains both original Binance BTC asset keys for
older preset files; an explicit empty list disables tick discovery. Saving a
sphere look records its currently bound asset key. Whole looks retain their
existing cross-asset application behavior.
