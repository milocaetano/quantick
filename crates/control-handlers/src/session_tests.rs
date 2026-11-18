//! The session scopes walk the window's open tabs, each answering for
//! itself: rows come in screen order under the tabs' ids.
use super::*;
use crate::test_support::{PaperDesk, TabsWindow};

/// One tab: its market and its paper desk, playing no recording.
struct Tab {
    symbol: &'static str,
    desk: PaperDesk,
}

impl SessionTab for Tab {
    fn symbol(&self) -> &str {
        self.symbol
    }
    fn desk(&self) -> &dyn TradeDesk {
        &self.desk
    }
    fn replay(&self) -> Option<Box<dyn ReplayRead + '_>> {
        None
    }
}

fn tab(symbol: &'static str) -> Tab {
    let mut account = quantick_paper::PaperAccount::with_trades_dir(std::path::PathBuf::from(
        "no-such-trades-folder",
    ));
    account.set_symbol(symbol);
    Tab {
        symbol,
        desk: PaperDesk::new(account),
    }
}

#[test]
fn the_paper_and_replay_scopes_read_each_open_tab_in_screen_order() {
    let window = TabsWindow::new(vec![(7, tab("WINZ26")), (2, tab("WDOZ26"))]);
    let context = CaptureContext {
        captured_at_unix_ms: 0,
    };

    let paper = project_paper(&window, context);
    let rows: Vec<(u64, &str, bool)> = paper
        .tabs
        .iter()
        .map(|row| (row.tab_id.get(), row.symbol.as_str(), row.flat))
        .collect();
    assert_eq!(rows, [(7, "WINZ26", true), (2, "WDOZ26", true)]);

    let replay = project_replay(&window, context);
    let rows: Vec<(u64, bool)> = replay
        .tabs
        .iter()
        .map(|row| (row.tab_id.get(), row.replaying || row.session.is_some()))
        .collect();
    assert_eq!(rows, [(7, false), (2, false)]);

    let keys = revision(&window);
    assert_eq!(
        keys.iter().map(|key| key.tab_id).collect::<Vec<_>>(),
        [7, 2]
    );
}
