use super::{ExclusionClass, Mapping, Source, UiBehaviour};
const PENDING_HISTORY: Mapping = Mapping::Excluded {
    class: ExclusionClass::PendingCapability,
    reason: "no capability fetches older venue candles; trade history loads through \
             feed.history.load. Tracked in issue 401",
};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (5, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "history.candles.load_older",
        title: "Fetch another span of older venue candles",
        reach: "toolbar history caret",
        keys: &[(Source::ToolbarAction, "LoadOlderCandles")],
        mapping: PENDING_HISTORY,
    },
    UiBehaviour {
        id: "history.progressive.toggle",
        title: "Build venue history backwards a week at a time, or in one request",
        reach: "window menu",
        keys: &[(Source::MenuEntry, "Progressive venue history")],
        mapping: capability!("settings.set"),
    },
    UiBehaviour {
        id: "history.trades.cancel",
        title: "Stop loading history and keep what arrived",
        reach: "the loading History button, its menu's Cancel loading, or an Esc no draft, selection or armed tool wants",
        keys: &[(Source::ToolbarAction, "CancelHistory")],
        mapping: capability!("feed.history.cancel"),
    },
    UiBehaviour {
        id: "history.trades.load",
        title: "Load the chart back by hours of trading or to a previous session's open",
        reach: "toolbar History button (repeats the last target) and its menu's targets",
        keys: &[(Source::ToolbarAction, "LoadHistory")],
        mapping: capability!("feed.history.load"),
    },
    UiBehaviour {
        id: "history.venue_lead_in.toggle",
        title: "Put venue minute candles in front of bars cut by trades",
        reach: "window menu",
        keys: &[(Source::MenuEntry, "Venue candles on charts cut by trades")],
        mapping: capability!("settings.set"),
    },
];
