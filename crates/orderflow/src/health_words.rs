//! The health lines the order-flow settings panel prints at the foot of its
//! L2 and Bubbles sections: plain formatting of what the engine published,
//! kept beside it rather than in the UI crate.

use quantick_orderbook::BookLevel;
use rust_decimal::Decimal;

use crate::engine::{BookPublished, OrderflowHealth};

/// What the "book now" line reads from.
pub const BOOK_NOW_HOVER: &str =
    "Best resting bid and ask of the live book, read from the published ladder.";

/// Why some contracts are not drawn.
pub const FLOORED_HOVER: &str = concat!(
    "the minimum-quantity setting under bubble visuals. It is the ",
    "only thing left that keeps contracts off the canvas, so it says ",
    "how many - in contracts, not in dots, because what matters is ",
    "the size of what is missing. Set it to zero to draw everything",
);

/// What a merged mark stands for.
pub const FOLDED_HOVER: &str = concat!(
    "the frame draws a bounded number of bubbles, split between ",
    "the candles and the tape so neither can crowd the other out. ",
    "Over that budget the marks merge - the candles fold their ",
    "smallest together, the tape folds its oldest - and a merged ",
    "bubble carries the exact summed quantity and says how many ",
    "marks it stands for. A fold never crosses a side, a pane or a ",
    "bar, so a frame with more of those than it has budget draws ",
    "the extra marks instead. Nothing is discarded",
);

/// The L2 section's health lines, top to bottom: the capture and its
/// levels, the live book and its ladder while one is published, what the
/// history holds and the range the map is drawn at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookHealthWords {
    pub levels: String,
    pub book_now: Option<String>,
    pub ladder: Option<String>,
    pub retained: String,
    pub range: String,
}

impl BookHealthWords {
    #[must_use]
    pub fn of(published: &BookPublished) -> Self {
        let health = &published.health;
        let side = |level: Option<BookLevel>| match level {
            Some(level) => format!("{} × {}", level.price(), level.quantity()),
            None => "—".to_owned(),
        };
        let ladder = published.ladder.as_deref();
        let book_now = ladder.map(|ladder| {
            let spread = match (ladder.best_bid, ladder.best_ask) {
                (Some(bid), Some(ask)) => (ask.price() - bid.price()).to_string(),
                _ => "—".to_owned(),
            };
            let (bid, ask) = (side(ladder.best_bid), side(ladder.best_ask));
            format!("book now: bid {bid} · ask {ask} · spread {spread}")
        });
        Self {
            levels: format!(
                "{} · {} bid / {} ask levels",
                published.status.label(),
                health.bid_levels,
                health.ask_levels
            ),
            book_now,
            ladder: ladder.map(|ladder| {
                let (bids, asks) = (ladder.bids.len(), ladder.asks.len());
                format!("ladder holds {bids} bids / {asks} asks in view")
            }),
            retained: format!(
                "{} runs · {:.1} MiB retained · projection {:.1} ms",
                health.archived_runs + health.active_levels,
                health.history_bytes as f64 / (1024.0 * 1024.0),
                health.projection_ms
            ),
            range: format!(
                "effective range {} · {}× base",
                health.effective_grouping, health.effective_grouping_multiple
            ),
        }
    }
}

/// The Bubbles section's health lines: what the projection drew, and what
/// it left out - each of those only when there is any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BubbleHealthWords {
    pub projected: String,
    pub floored: Option<String>,
    pub folded: Option<String>,
}

impl BubbleHealthWords {
    #[must_use]
    pub fn of(health: &OrderflowHealth, show_aggressions: bool) -> Self {
        // The projection carries clusters whether or not the bubble layer
        // draws them - the live strip reads the same ones. Calling them
        // "bubbles" while none is on screen would report a layer that is off.
        let noun = if show_aggressions {
            "bubbles"
        } else {
            "clusters (bubble layer off)"
        };
        Self {
            projected: format!(
                "{} {noun} projected · {} aggressions retained",
                health.projection_aggressions, health.aggression_count
            ),
            floored: (health.floored_quantity > Decimal::ZERO).then(|| {
                let floored = health.floored_quantity;
                format!("{floored} contracts below your display floor are not drawn")
            }),
            folded: (health.folded_aggressions > 0).then(|| {
                let folded = health.folded_aggressions;
                format!("{folded} marks merged into a neighbour to fit the frame")
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::engine::BookLadder;

    fn level(price: i64, quantity: i64) -> BookLevel {
        BookLevel::new(Decimal::new(price, 0), Decimal::new(quantity, 0)).unwrap()
    }

    #[test]
    fn book_health_names_the_ladder_only_while_one_is_published() {
        let mut published = BookPublished::initial();
        let words = BookHealthWords::of(&published);
        assert_eq!((words.book_now, words.ladder), (None, None));
        assert!(words.levels.ends_with("· 0 bid / 0 ask levels"));
        assert_eq!(
            words.retained,
            "0 runs · 0.0 MiB retained · projection 0.0 ms"
        );
        published.ladder = Some(Arc::new(BookLadder {
            best_bid: Some(level(100, 3)),
            best_ask: None,
            bids: vec![level(100, 3)],
            asks: Vec::new(),
        }));
        let words = BookHealthWords::of(&published);
        let now = words.book_now.unwrap();
        assert_eq!(now, "book now: bid 100 × 3 · ask — · spread —");
        assert_eq!(
            words.ladder.unwrap(),
            "ladder holds 1 bids / 0 asks in view"
        );
    }

    #[test]
    fn bubble_health_says_clusters_while_the_layer_is_off() {
        let mut health = BookPublished::initial().health;
        let off = BubbleHealthWords::of(&health, false);
        assert!(
            off.projected
                .contains("clusters (bubble layer off) projected")
        );
        assert_eq!((off.floored, off.folded), (None, None));
        health.floored_quantity = Decimal::new(25, 1);
        health.folded_aggressions = 4;
        let on = BubbleHealthWords::of(&health, true);
        assert!(on.projected.starts_with("0 bubbles projected"));
        assert_eq!(
            on.floored.unwrap(),
            "2.5 contracts below your display floor are not drawn"
        );
        assert_eq!(
            on.folded.unwrap(),
            "4 marks merged into a neighbour to fit the frame"
        );
    }
}
