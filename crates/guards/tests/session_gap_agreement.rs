//! The bridge and the app must agree about where a trading session begins.
//!
//! Two pieces of software read the same tape and both have to decide the same
//! thing from it. `bridge/mt5/quantick_bridge.py` walks back from the newest
//! print to find where the session started, so it knows what the opening block
//! is; `crates/app/src/history_reach.rs` walks back from the chart's oldest bar
//! for the same reason, so it knows when one press of *load older* has arrived.
//! If those two ever disagree, a chart opens on a block whose left edge the
//! campaign does not recognise as a session edge — and the trader sees a
//! *load older* press that either refuses to move or silently drags in half of
//! the day before.
//!
//! Nothing about the two languages lets them share the constant, so this test
//! is the joint. It reads both sources and compares the numbers, which is
//! deliberately cruder than linking one of them and deliberately impossible to
//! forget: an edit to either side that does not touch the other turns this red
//! with a message naming both.
//!
//! **What it does not cover, and should say so.** The chart reads its gap from
//! `[history] session_gap_minutes` at runtime, not from the constant — the
//! constant is only that setting's default. So these tests pin the constants
//! and the shipped default to each other, and a trader who overrides the
//! setting in their own config still moves the chart's definition of a session
//! without moving the bridge's. Closing that needs the app to pass its gap to
//! the bridge it launches; recording it here is the honest interim, because a
//! guard believed to cover more than it does is worse than one whose limits
//! are written down.
//!
//! Why a test rather than a comment on each: the repository has been through
//! this exact failure. `QuantickBridge.mq5` sends 30 minutes of opening history
//! and `quantick_bridge.py` sent 720, and the two drifted apart quietly enough
//! that four separate branches worked on the symptom. A comment saying "keep
//! these in step" is what those two constants already had.

use std::path::{Path, PathBuf};

/// One constant that has to hold the same value on both sides of the wire.
struct Agreement {
    /// The Python name, as assigned at the bridge's module level.
    python: &'static str,
    /// The Rust name, as declared in `history_reach.rs`.
    rust: &'static str,
    /// Why they have to agree, printed when they do not.
    because: &'static str,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .to_path_buf()
}

/// The value assigned to `name` at the bridge's module level.
///
/// The bridge writes these as arithmetic (`60 * 60 * 1000`) because that is how
/// a reader checks them, so the right-hand side is evaluated rather than
/// parsed: a product of integer literals, which is all these assignments are
/// and all this needs to understand. Anything else is reported as unreadable
/// rather than guessed at, because a guess here would defeat the whole test.
fn python_constant(source: &str, name: &str) -> i64 {
    let prefix = format!("{name} = ");
    let line = source
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| {
            panic!(
                "`{name}` is not assigned at the module level of \
                 bridge/mt5/quantick_bridge.py. If it was renamed, rename it here too — \
                 that is what this test is for."
            )
        });
    let expression = line[prefix.len()..]
        .split('#')
        .next()
        .expect("a split always yields a first part")
        .trim();
    product_of_literals(expression, name, "bridge/mt5/quantick_bridge.py")
}

/// The value assigned to `pub const NAME` in a Rust source.
///
/// `crates/app` builds as a binary, so an integration test cannot `use` its
/// constants — there is no library target to link. Reading the source is what
/// is left, and it turns out to be the more honest half of the pair anyway:
/// both sides are now checked the same way, and neither can satisfy this test
/// by being the one that gets imported.
fn rust_constant(source: &str, name: &str) -> i64 {
    // The visibility and the integer type are both incidental: what is being
    // read is a number two sides have to agree on, and one of them is a
    // private `usize` in another crate.
    let needle = format!("const {name}: ");
    let line = source
        .lines()
        .find(|line| {
            let line = line.trim_start();
            line.starts_with(&needle) || line.starts_with(&format!("pub {needle}"))
        })
        .unwrap_or_else(|| {
            panic!(
                "`{name}` is not declared as a `pub const … : i64` in \
                 crates/app/src/history_reach.rs. If it was renamed or retyped, say so here \
                 too — that is what this test is for."
            )
        });
    let expression = line
        .split_once('=')
        .expect("a const declaration has a value")
        .1
        .split(';')
        .next()
        .expect("a split always yields a first part")
        .trim();
    product_of_literals(expression, name, "crates/app/src/history_reach.rs")
}

/// Evaluate `a * b * c`, which is all either side writes these as.
///
/// Both files spell their durations out as arithmetic (`60 * 60 * 1_000`)
/// because that is how a reader checks them against a clock. Anything richer is
/// reported as unreadable rather than guessed at: a guess here would let the
/// two drift while the test stayed green, which is the one failure it exists to
/// prevent.
fn product_of_literals(expression: &str, name: &str, file: &str) -> i64 {
    expression
        .split('*')
        .map(|factor| {
            let factor = factor.trim().replace('_', "");
            factor.parse::<i64>().unwrap_or_else(|_| {
                panic!(
                    "`{name}` in {file} is `{expression}`, which this test only knows how to \
                     read as a product of integer literals. Either write it that way or teach \
                     this test the new shape."
                )
            })
        })
        .product()
}

#[test]
fn the_bridge_and_the_app_measure_a_session_the_same_way() {
    let root = repo_root();
    let source = std::fs::read_to_string(root.join("bridge/mt5/quantick_bridge.py"))
        .expect("the MetaTrader bridge is part of this repository");
    let rust_source = std::fs::read_to_string(root.join("crates/app/src/history_reach.rs"))
        .expect("the chart's history reach is part of this repository");

    let agreements = [
        Agreement {
            python: "SESSION_GAP_MS",
            rust: "SESSION_GAP_MS",
            because: "the bridge stops the opening block at a gap this wide, and the app \
                      decides a load-older campaign reached a session edge at the same one. \
                      Different values mean the chart opens on a block whose edge the \
                      campaign does not recognise.",
        },
        Agreement {
            python: "SESSION_WALK_MAX_SPAN_MS",
            rust: "MAX_CAMPAIGN_SPAN_MS",
            because: "on a market that never closes there is no gap to stop either side, so \
                      both fall back to this span. Different values mean the opening block \
                      and one press of load older disagree about how far back is enough.",
        },
    ];

    let mut broken = Vec::new();
    for agreement in &agreements {
        let python_value = python_constant(&source, agreement.python);
        let rust_value = rust_constant(&rust_source, agreement.rust);
        if python_value != rust_value {
            broken.push(format!(
                "  bridge {} = {} but chart {} = {}\n    {}",
                agreement.python, python_value, agreement.rust, rust_value, agreement.because
            ));
        }
    }

    assert!(
        broken.is_empty(),
        "the MetaTrader bridge and the chart no longer measure a session the same way:\n{}\n\
         Change both, or neither.",
        broken.join("\n")
    );
}

#[test]
fn the_shipped_config_default_agrees_with_the_bridge_too() {
    // The constants agreeing is necessary and not sufficient. At runtime the
    // chart does not read `SESSION_GAP_MS` at all: `HistorySettings::
    // reach_bounds` builds the gap from `[history] session_gap_minutes`, whose
    // default is derived from that constant but whose *shipped* value lives in
    // `crates/app/config/feeds.toml`. A default edited there and nowhere else
    // would leave the two ends of the tape measuring a session differently
    // with the test above still green.
    //
    // What is still deliberately not covered, because nothing here can: a
    // trader who overrides `session_gap_minutes` in their own config moves the
    // chart's definition and not the bridge's. The honest fix is for the app
    // to pass its gap to the bridge it launches; until then this is a known
    // and recorded divergence rather than an assumed impossibility.
    let root = repo_root();
    let source = std::fs::read_to_string(root.join("bridge/mt5/quantick_bridge.py"))
        .expect("the MetaTrader bridge is part of this repository");
    let bridge_gap_ms = python_constant(&source, "SESSION_GAP_MS");

    let config = std::fs::read_to_string(root.join("crates/app/config/feeds.toml"))
        .expect("the shipped feed config is part of this repository");
    // The key ships commented out, as the whole `[history]` section does, so
    // the value read here is the documented default a trader would uncomment.
    let shipped_minutes = config
        .lines()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim()
                .strip_prefix("session_gap_minutes")
        })
        .filter_map(|rest| rest.trim().strip_prefix('='))
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .next()
        .expect("feeds.toml documents session_gap_minutes");

    assert_eq!(
        shipped_minutes * 60_000,
        bridge_gap_ms,
        "the config default the trader reads ({shipped_minutes} min) and the gap the \
         bridge walks by ({bridge_gap_ms} ms) disagree; change both, or neither"
    );
}

#[test]
fn the_slice_cap_matches_what_the_feed_will_accept() {
    // The bridge slices the opening block; the feed trims any block past
    // `MAX_TRADES_PER_PAGE` and drops the surplus with only a warn line. So a
    // bridge whose slice cap drifts above the feed's turns a fixed defect back
    // on — a quiet cut of the trader's morning — and the bridge's own comment
    // says this test is what stops that. It was not, until it existed.
    let root = repo_root();
    let bridge = std::fs::read_to_string(root.join("bridge/mt5/quantick_bridge.py"))
        .expect("the MetaTrader bridge is part of this repository");
    let stream = std::fs::read_to_string(root.join("crates/feed-mt5/src/stream.rs"))
        .expect("the feed's stream is part of this repository");

    let bridge_cap = python_constant(&bridge, "MAX_SLICE_TICKS_THE_FEED_ACCEPTS");
    let feed_cap = rust_constant(&stream, "MAX_TRADES_PER_PAGE");

    assert_eq!(
        bridge_cap, feed_cap,
        "the bridge clamps its opening slices to {bridge_cap}, but the feed \
         accepts {feed_cap} in one block and silently trims the rest. Change \
         both, or neither."
    );
}

#[test]
fn the_fill_progress_is_on_the_wire_and_is_optional() {
    // The control plane is how an operator without a mouse tells "this chart
    // is still arriving" from "this is all there is". That answer travels as
    // `opening_slices_remaining` in the feed-status snapshot, so the shipped
    // schema has to carry it — and has to carry it as *optional*, because it
    // is absent whenever nothing is filling, which is the steady state.
    //
    // Asserted against the committed schema rather than the Rust type: the
    // schema is what a consumer validates against, and it is the artifact that
    // would silently stop matching if the field were renamed or made required.
    let schema = std::fs::read_to_string(
        repo_root().join("schemas/control/observer-feed-status-v1.schema.json"),
    )
    .expect("the observer feed-status schema is part of this repository");

    assert!(
        schema.contains("\"opening_slices_remaining\""),
        "the feed-status schema does not carry the opening fill's progress, so          an operator cannot read it back"
    );
    // The field must appear in *no* `required` array in the document. Checked
    // across all of them rather than the first: the schema carries one per
    // `$defs` entry, and an earlier version of this assertion read
    // `FeedCapabilitiesSnapshot`'s — not the one that governs this field — so
    // it would have stayed green if the field had been made required. A guard
    // that cannot fail for the reason it states is worse than none.
    let required_anywhere = schema
        .match_indices("\"required\"")
        .filter_map(|(at, _)| schema[at..].split(']').next())
        .any(|block| block.contains("opening_slices_remaining"));
    assert!(
        !required_anywhere,
        "the field is marked required in some definition, so a snapshot taken          while nothing is filling — the steady state — would fail its own schema"
    );
}

#[test]
fn the_walk_budget_is_derived_from_the_span_it_bounds() {
    let source = std::fs::read_to_string(repo_root().join("bridge/mt5/quantick_bridge.py"))
        .expect("the MetaTrader bridge is part of this repository");
    let span = python_constant(&source, "SESSION_WALK_MAX_SPAN_MS");
    let gap = python_constant(&source, "SESSION_GAP_MS");
    assert!(
        span % gap == 0,
        "the opening walk steps in gap-wide windows up to a span budget, so the span \
         ({span} ms) has to be a whole number of gaps ({gap} ms). A remainder is a final \
         window the walk can never spend."
    );
}
