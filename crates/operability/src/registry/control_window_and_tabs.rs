use super::{ExclusionClass, Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (0, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "layout.window.state.set",
        title: "Set native window presentation",
        reach: "title-strip minimize and maximize controls; double-click free title-strip space",
        keys: &[(
            Source::Authored,
            "native title-strip minimize/maximize buttons and free-space double-click",
        )],
        mapping: capability!("layout.window.state.set"),
    },
    UiBehaviour {
        id: "home.show",
        title: "Show the Home tab",
        reach: "first tab of the market title strip, Ctrl+1, Ctrl+Shift+H",
        keys: &[
            (Source::TabAction, "Home"),
            (Source::Hotkey, "HOME_SHORTCUT"),
            (Source::Hotkey, "HOME_ALT_SHORTCUT"),
        ],
        mapping: capability!("home.show"),
    },
    UiBehaviour {
        id: "home.leave",
        title: "Leave the Home tab",
        reach: "the active market's chip, Ctrl+Tab and Ctrl+Shift+Tab while Home shows",
        keys: &[(
            Source::Authored,
            "market chip click and tab cycling while Home shows",
        )],
        mapping: capability!("home.leave"),
    },
    UiBehaviour {
        id: "window.move",
        title: "Move or resize the native window",
        reach: "free title-strip drag space and native window edges; scene tab_strip.drag names the drag surface",
        keys: &[(
            Source::Authored,
            "native title-strip drag and window-edge resize gestures",
        )],
        mapping: capability!("layout.window.placement.set"),
    },
    UiBehaviour {
        id: "window.snap",
        title: "Snap the native window",
        reach: "Windows+Left/Right and the operating system's native window snapping",
        keys: &[(Source::Authored, "native Windows snapping")],
        mapping: excluded!(
            UiOnlyByDecision,
            "Windows snapping remains an OS gesture; layout.window.placement.set requests an explicit bounded rectangle and does not invoke shell snapping"
        ),
    },
    UiBehaviour {
        id: "app.exit",
        title: "Close the application",
        reach: "the window's own close button",
        keys: &[(Source::Authored, "native window close button")],
        mapping: excluded!(
            Authority,
            "`docs/control-plane/observer-threat-model.md` section 6 refuses starting or \
             stopping Quantick, a terminal, a broker or any other process. A capability that \
             closes the final window would end every other capability's session; layout.window.close instead preserves tabs in a sibling and refuses final-window exit"
        ),
    },
    UiBehaviour {
        id: "tab.market.activate",
        title: "Switch to another market tab",
        reach: "market title strip and its overflow menu; Ctrl+Tab and Ctrl+Shift+Tab",
        keys: &[
            (Source::TabAction, "Activate"),
            (Source::Hotkey, "NEXT_TAB_SHORTCUT"),
            (Source::Hotkey, "PREVIOUS_TAB_SHORTCUT"),
        ],
        mapping: capability!("layout.market_tab.select"),
    },
    UiBehaviour {
        id: "tab.market.transfer",
        title: "Reorder, detach or attach a market tab",
        reach: "drag a market chip within its strip, outside it, or onto another exposed window strip",
        keys: &[(
            Source::Authored,
            "native market-tab drag and insertion marker",
        )],
        mapping: capability!(
            "layout.market_tab.reorder",
            "layout.market_tab.detach",
            "layout.market_tab.attach"
        ),
    },
    UiBehaviour {
        id: "tab.market.drag.cancel",
        title: "Cancel a staged market-tab drag",
        reach: "Escape while dragging a market chip",
        keys: &[(
            Source::Authored,
            "native market-tab staged gesture and Escape",
        )],
        mapping: capability!(
            "layout.market_tab.drag.begin",
            "layout.market_tab.drag.cancel"
        ),
    },
    UiBehaviour {
        id: "window.close_preserving_tabs",
        title: "Close one native window and retain every market in a sibling",
        reach: "native window close while sibling windows exist",
        keys: &[(Source::Authored, "native close-to-survivor lifecycle")],
        mapping: capability!("layout.window.close"),
    },
    UiBehaviour {
        id: "tab.market.close",
        title: "Close the active market tab",
        reach: "tab strip chip, Ctrl+W",
        keys: &[
            (Source::TabAction, "Close"),
            (Source::Hotkey, "CLOSE_TAB_SHORTCUT"),
        ],
        mapping: excluded!(
            PendingCapability,
            "closing a tab discards its drawings and its paper position, and the control \
             contract admits no destructive effect yet — the same ceiling `layout.tab.delete` \
             sits under. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "tab.market.open",
        title: "Open the Open a market palette",
        reach: "tab strip +, Ctrl+T, Home's Open a market",
        keys: &[
            (Source::TabAction, "New"),
            (Source::Hotkey, "NEW_TAB_SHORTCUT"),
        ],
        mapping: capability!("market.palette.open"),
    },
    UiBehaviour {
        id: "market.palette.choose",
        title: "Open or focus the market chosen in the palette",
        reach: "Enter or Ctrl+Enter on a palette row, with its Bars and Workspace selectors",
        keys: &[(
            Source::Authored,
            "palette rows are drawn from the headless market list, not an action enum",
        )],
        mapping: capability!("market.open"),
    },
    UiBehaviour {
        id: "market.palette.add",
        title: "Add a symbol to a venue and open it",
        reach: "Enter on a palette no-match row",
        keys: &[(
            Source::Authored,
            "the no-match row is drawn from the headless market list",
        )],
        mapping: excluded!(
            PendingCapability,
            "no capability adds a symbol to a venue's list yet; `market.open` opens what the \
             config and the added-symbols file already offer. Tracked in issue 401"
        ),
    },
    UiBehaviour {
        id: "market.palette.remove",
        title: "Remove a market the trader added",
        reach: "Delete on a palette row the trader added",
        keys: &[(
            Source::Authored,
            "the row is drawn from the headless market list",
        )],
        mapping: capability!("catalog.remove"),
    },
    UiBehaviour {
        id: "tab.market.source.set",
        title: "Switch the active tab to another configured feed and symbol",
        reach: "the toolbar's market button, then Enter on a row of the palette in switch mode",
        keys: &[(Source::ToolbarAction, "SwitchMarket")],
        mapping: capability!("tab.market.set", "feed.source.set"),
    },
    UiBehaviour {
        id: "tab.market.roll",
        title: "Roll the active tab to its next configured contract",
        reach: "the toolbar's market button, then the roll row of the palette in switch mode; \
                the feed notice popup's Roll button on a contract the venue no longer lists, \
                and the same Roll beside the status word and on the empty chart",
        keys: &[
            (
                Source::Authored,
                "the roll row is drawn from the headless market list, not an action enum",
            ),
            (Source::NoticeAction, "Roll"),
        ],
        mapping: capability!("feed.contract.roll"),
    },
];
