# Dimension 3 — nothing hardcoded

Read before writing a hardcoded-value finding: the three tiers decide what the fix is, and naming the wrong tier is worse than not filing.

Every literal that *configures behaviour* — a number, a threshold, a path, an
endpoint — lives in a named constant or in config, never inline at the point of
use. That it is "obviously" 2.0, or used exactly once, is not a defence: the
magic numbers that survive review are the ones nobody looked at twice.

**Scope this before hunting.** The rule is about values that tune what the code
*does*, not about every literal in the diff. Message text is not a
configuration value: `log::info!("…")`, `anyhow!("…")` and assertion strings
stay where they are read. Filing those turns a review into a wall of noise and
costs it the precision *Verify before reporting* demands.

Three tiers. Every finding names which one the value belongs in, because
"extract a constant" is the wrong fix for a value the trader was supposed to
edit:

- **A config file — anything a *user* tunes.** Feeds and symbols in
  `crates/app/config/feeds.toml`, bubble looks in `config/bubbles.toml`,
  footprint styling in `config/footprint.toml`, the layers a fresh chart
  opens with in `config/chart-layers.toml`, strategy presets in
  `quantick-strategies.toml`, each overridable by env var. Symbols, endpoints,
  tick sizes, colours and user-facing thresholds are never literals in code. A
  Rust `const` may hold the *default*, but the knob itself lives in the file: a
  `const` still costs a rebuild, and a rebuild is the one thing the trader
  cannot do. This is dimension 7's *what the trader authors is data* rule seen
  from the constants side.
- **A shared module — a value two or more places must agree on.** A bridge
  port, a protocol magic, a frame bound, a file-format version, a directory
  name written by one crate and scanned by another. Duplicating it at both ends
  is a bug with a delay fuse: it ships green and breaks the day one side
  changes. One owner, imported by the rest — and the finding is the *second*
  copy, wherever it sits. Where the value crosses a language boundary the repo
  cannot type-check (the MQL5 bridge, the Python exporter), it cannot be
  imported, so the finding is instead the missing test or doc comment pinning
  the two sides together.
- **Module top — a value one module owns.** `SCREAMING_SNAKE_CASE`, unit in
  the name (`_MS`, `_PX`, `_TICKS`, `_BYTES`), and a doc comment saying *why
  this number* rather than restating it. `const` and `static` cost nothing at
  runtime, so there is never a performance argument for leaving the literal
  inline. This is the tier the repo already uses well — compare against the
  constant blocks at the top of `crates/app/src/app.rs` before proposing
  anything else.

**Opening state is a config value too**, and its tier is always the first
one. Which layers, panels and surfaces a fresh launch draws is a product
decision someone may want different, so it belongs in the shipped TOML —
`config/chart-layers.toml` is the worked example, compiled in with
`include_str!` the way `feeds.toml` and `bubbles.toml` are. A `Default` impl
deciding what the first frame shows, or a `set_*(false)` at startup, is a
finding: it puts a product decision where nobody can change it without a
build, and it splits the answer across a struct and a file the moment a state
file exists. The test here is not "is it a number" — it is "would a human ever
want this different".

Also:

- A magic number in a renderer, a threshold buried in a condition, a sleep or
  timeout duration, a retry count, a hardcoded `C:\...` or `/tmp` path, a bare
  URL or port — each is a finding every time.
- A capacity or buffer size is a finding when the number *means* something a
  human would tune — a queue bound, a frame limit, a page size. An arbitrary
  `with_capacity` hint that only avoids a realloc is not; say so and move on.
- Exempt, and say so rather than filing them: `0`, `1` and `-1` as identity or
  step; indices into a shape the code itself fixes (`rgba[3]`); message and
  assertion text; and a literal a doc comment right there derives from a named
  constant.
- Config round-trips must survive a save: a writer that drops comments or
  re-emits `0.78` as `0.7799999713897705` destroys the reason the file is
  tracked in git. Check the write path, not just the read path.

