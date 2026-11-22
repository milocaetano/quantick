use super::{ExclusionClass, Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (10, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "feed.reconnect",
        title: "Respawn the transport and keep the timeline",
        reach: "the feed notice popup; the connection overlay's Reconnect; the status bar's \
                Reconnect while the chart holds bars",
        keys: &[(Source::NoticeAction, "Reconnect")],
        mapping: capability!("feed.reconnect"),
    },
    UiBehaviour {
        id: "feed.reload",
        title: "Throw the timeline away and rebuild the chart",
        reach: "the feed notice popup",
        keys: &[(Source::NoticeAction, "Reload")],
        mapping: capability!("feed.reload"),
    },
    UiBehaviour {
        id: "feed.connection_state",
        title: "Read what the chart says about its feed, book, trades and current job",
        reach: "the flow header's chip and slot, the connection overlay, the status word",
        keys: &[(
            Source::Authored,
            "a readout painted from one state, with no control of its own",
        )],
        mapping: excluded!(
            UiOnlyByDecision,
            "nothing to perform: the state is readable as `connection` on the `feed.status` \
             scope, and the recovery it offers is `feed.reconnect` and `feed.reload`"
        ),
    },
    UiBehaviour {
        id: "feed.deal_recording.set",
        title: "Record the venue's deal counter, stop, load a recorded day, or show the pane as \
                trades bars",
        reach: "the REC control beside the symbol and its popover; the Settings window's \
                `Record deals by default` row",
        // The call's `record_by_default` is the same choice as the Settings row.
        keys: &[
            (Source::ToolbarAction, "DealRecording"),
            (Source::MenuEntry, "Record deals by default"),
        ],
        // `ShowAsTrades` is the popover's shortcut to the `trades` rule, the
        // same recut `layout.pane.set_bar_spec` drives; `OpenFolder` reveals
        // the recording directory in the OS file browser, a hand-only
        // convenience whose path `feed.status` reports as `file`.
        mapping: capability!("feed.deal_recording.set", "layout.pane.set_bar_spec"),
    },
];
