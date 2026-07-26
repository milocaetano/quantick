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

[presets.bubbles]         # everything visual; every key is optional
max_radius = 15.0
render_mode = "sphere"    # "flat" (classic disc) or "sphere" (shaded 3D ball);
sphere_shading = 0.6      # spheres keep overlapping prints readable as separate
sphere_highlight = 0.4    # bubbles on a dense tape
side_offset = 3.5         # buys nudged up, sells down, so both sides are readable
front_width = 3.0         # the vertical consumption mark ("risco")
trail_length = 18.0       # the glow into the consumed side ("rastro")
buy_color = [46, 224, 150]  # omit to follow the chart theme
```

A preset only describes how bubbles **look**. Turning the layer on stays a live
decision in the panel, so having this file can never start capture by itself.
Quantities (`min_quantity`, `size_reference_quantity`) are in the symbol's own
units — contracts on the mini index, coins on Binance — so a size-based preset
says which market it was built for.
