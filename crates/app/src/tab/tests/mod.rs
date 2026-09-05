//! What a tab owes its panes: routing, ordering, collapsing and aiming.
//!
//! The four suites that were inline in `tab.rs`, kept under their own names.

use super::canvas::trading_pane;
use super::*;
use crate::pane::SharedInteraction;

#[cfg(test)]
mod shared_routing_tests {
    use super::*;
    use crate::state::BarSpec;
    use quantick_feed as feed;
    use tokio::sync::mpsc;

    /// Hands each test tab its own trades directory. A tab opens a
    /// paper-trading ledger, and two tabs pointed at one folder read each
    /// other's trades — which shows up as unrelated ledger tests failing.
    static NEXT_TEST_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// A tab with `context` context panes stacked beside its flow pane.
    fn tab_with_context_panes(context: usize) -> Tab {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let mut tab = Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            // Its own directory, never the shared temp root: a tab opens a
            // paper-trading ledger, and pointing every test tab at one folder
            // makes them read each other's trades.
            crate::scratch::thread_dir("tab-test").join(
                NEXT_TEST_DIR
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string(),
            ),
        );
        for slot in 0..context {
            tab.time_panes.push(ChartPane::time(
                100 + slot as u64,
                crate::time_header::DEFAULT_INTERVAL_MS,
            ));
        }
        tab
    }

    /// Collapsing must not spend the width it collapses.
    ///
    /// `split_fraction` is the trader's own sizing and the only thing that can
    /// restore the column. A collapse that overwrote it — with a rail's width,
    /// or with a default — would hand back a different chart from the one that
    /// was put away.
    #[test]
    fn collapsing_a_column_keeps_the_width_it_springs_back_to() {
        let mut tab = tab_with_context_panes(1);
        tab.split_fraction = 0.42;

        tab.context_collapsed = true;
        assert_eq!(
            tab.split_fraction, 0.42,
            "the collapse spent the width it was supposed to remember"
        );

        // And again: a second collapse must not overwrite it either.
        tab.context_collapsed = true;
        assert_eq!(tab.split_fraction, 0.42);

        tab.context_collapsed = false;
        assert_eq!(
            tab.split_fraction, 0.42,
            "the column came back at a width the trader never chose"
        );
    }

    /// The flow pane is address `0` and the context stack follows it, whatever
    /// order they are drawn in. A reader who took this for a left-to-right
    /// order would mirror every edit, so it is pinned.
    #[test]
    fn panes_are_addressed_flow_first_then_the_context_stack() {
        let tab = tab_with_context_panes(2);
        assert_eq!(tab.pane_count(), 3);
        assert_eq!(
            tab.pane_at(0).map(|pane| pane.id),
            Some(tab.flow_pane.id),
            "address 0 is the flow pane"
        );
        assert_eq!(tab.pane_at(1).map(|pane| pane.id), Some(100));
        assert_eq!(tab.pane_at(2).map(|pane| pane.id), Some(101));
        assert!(tab.pane_at(3).is_none(), "there is no fourth pane");
    }

    /// A shared mark belongs to the pane whose store holds it, and an edit
    /// made on a mirror has to land *there* — not on "the other pane".
    ///
    /// With two panes those two phrases mean the same thing, which is why the
    /// routing this replaced (`side.other()`) was correct and why nothing
    /// caught it losing that meaning. With a stack beside the flow pane there
    /// is more than one other pane. The owner named here is address 2, which
    /// is neither the actor nor the actor's single counterpart, so this fails
    /// against the arithmetic it replaced rather than merely passing beside
    /// it.
    #[test]
    fn a_shared_gesture_opens_on_the_pane_the_interaction_names() {
        let mut tab = tab_with_context_panes(2);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: Some(2),
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);

        assert!(
            tab.pane_at(2)
                .expect("the named pane exists")
                .drawings
                .in_gesture(),
            "the gesture must open on the pane the interaction named"
        );
        for bystander in [0usize, 1] {
            assert!(
                !tab.pane_at(bystander)
                    .expect("pane exists")
                    .drawings
                    .in_gesture(),
                "pane {bystander} took a gesture it was never named for"
            );
        }
    }

    /// An interaction with no owner is refused rather than guessed. Landing it
    /// on a neighbour chosen by arithmetic would move an object the trader
    /// drew onto a chart they were not working on.
    #[test]
    fn an_unowned_interaction_lands_nowhere() {
        let mut tab = tab_with_context_panes(2);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: None,
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);
        for pane in 0..tab.pane_count() {
            assert!(
                !tab.pane_at(pane)
                    .expect("pane exists")
                    .drawings
                    .in_gesture(),
                "pane {pane} opened a gesture for an interaction that named no owner"
            );
        }
    }

    /// An owner address past the end of the stack is ignored, not panicked on:
    /// a saved workspace or a control-plane call can name a pane that has
    /// since gone.
    #[test]
    fn an_owner_that_no_longer_exists_is_ignored() {
        let mut tab = tab_with_context_panes(1);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: Some(7),
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);
        for pane in 0..tab.pane_count() {
            assert!(
                !tab.pane_at(pane)
                    .expect("pane exists")
                    .drawings
                    .in_gesture()
            );
        }
    }
}

#[cfg(test)]
mod move_pane_tests {
    use super::*;
    use crate::state::BarSpec;
    use quantick_feed as feed;
    use tokio::sync::mpsc;

    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

    /// One trade at `ms`, cheap enough to build a block from.
    fn older_trade(ms: i64) -> quantick_engine::Trade {
        quantick_engine::Trade {
            agg_id: ms as u64,
            timestamp_ms: ms,
            price: rust_decimal::Decimal::new(1000, 1),
            quantity: rust_decimal::Decimal::ONE,
            side: quantick_engine::Side::Buy,
        }
    }

    /// A tab whose feed channel is still held, so a test can post events onto
    /// it and drain them the way a frame does.
    fn tab_with_feed() -> (Tab, mpsc::Sender<FeedEvent>) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let tab = Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            crate::scratch::thread_dir("opening-test").join(
                NEXT_DIR
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string(),
            ),
        );
        (tab, evt_tx)
    }

    #[test]
    fn an_opening_slice_draws_without_answering_the_traders_press() {
        // The defect the trader-ux review caught, and the reason the opening
        // block has an event of its own rather than reusing the reply.
        //
        // The bridge fills the session in behind the chart in thirty-odd
        // slices. A trader who presses `+ older` while that is happening has
        // raised a loading indicator and started a campaign; if a slice
        // arrived as `HistoryPrepended` it would stop that indicator on
        // history the trader did not ask for, and hand the campaign a page it
        // did not fetch — so a run could spend its budget, or declare itself
        // finished, on tape it never pulled.
        let (mut tab, feed_tx) = tab_with_feed();
        // The tab already has its own opening request in flight, so the count
        // is asserted as a delta rather than against zero: what matters is
        // that an opening slice answers nothing, whatever else is outstanding.
        tab.loading.begin(LoadingTask::History);
        let before = tab.loading.count(LoadingTask::History);
        assert!(before > 0, "the press raised an indicator to begin with");

        let slice: Vec<_> = (0..8).map(|i| older_trade(1_000 + i)).collect();
        feed_tx
            .try_send(FeedEvent::OpeningPrepended {
                trades: slice.clone(),
                remaining: Some(4),
            })
            .expect("the test channel has room");
        tab.drain_feed();

        assert_eq!(
            tab.loading.count(LoadingTask::History),
            before,
            "an opening slice must leave the trader's own loading indicator up"
        );
        assert_eq!(
            tab.history_trades,
            slice.len(),
            "and it is still charted and counted — it is the trader's morning"
        );
        assert_eq!(
            tab.opening_slices_remaining(),
            Some(4),
            "and how much of the session is still arriving is readable, not              only loggable: an operator has to be able to tell a chart that is              still filling from one that has all it will get"
        );
    }

    #[test]
    fn a_page_the_trader_asked_for_still_answers_the_press() {
        // The other half, so the test above cannot pass by the reply path
        // having quietly stopped working.
        let (mut tab, feed_tx) = tab_with_feed();
        tab.loading.begin(LoadingTask::History);
        let before = tab.loading.count(LoadingTask::History);
        feed_tx
            .try_send(FeedEvent::HistoryPrepended(vec![older_trade(1_000)]))
            .expect("the test channel has room");
        tab.drain_feed();
        assert_eq!(
            tab.loading.count(LoadingTask::History),
            before - 1,
            "a reply settles exactly the one press that asked for it"
        );
    }

    fn tab_with(context: usize) -> Tab {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let mut tab = Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            crate::scratch::thread_dir("move-test").join(
                NEXT_DIR
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string(),
            ),
        );
        for slot in 0..context {
            tab.time_panes.push(ChartPane::time(
                200 + slot as u64,
                crate::time_header::DEFAULT_INTERVAL_MS,
            ));
        }
        tab
    }

    /// The order the stack draws in is the order it holds, so moving a chart
    /// moves the pane rather than swapping what is inside two of them: the
    /// drawings, indicators and bars travel with the chart the trader moved.
    #[test]
    fn moving_a_chart_carries_the_pane_rather_than_its_contents() {
        let mut tab = tab_with(3);
        let ids: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(ids, vec![200, 201, 202]);

        assert!(
            tab.move_context_pane(3, 1),
            "the bottom chart moves to the top"
        );
        let after: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(
            after,
            vec![202, 200, 201],
            "the pane moved and the others closed up behind it"
        );
    }

    #[test]
    fn moving_a_chart_one_slot_swaps_it_with_its_neighbour() {
        let mut tab = tab_with(2);
        assert!(tab.move_context_pane(1, 2));
        let after: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(after, vec![201, 200]);
    }

    /// The flow pane is address `0` and does not move: its column is the one
    /// thing every preset agrees on, and a caller that asked to move the
    /// heatmap meant something this cannot do.
    #[test]
    fn the_flow_pane_refuses_to_move() {
        let mut tab = tab_with(2);
        let before: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert!(!tab.move_context_pane(0, 1), "address 0 is the flow pane");
        assert!(!tab.move_context_pane(1, 0), "and it is not a destination");
        assert_eq!(
            tab.time_panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            before,
            "a refused move must leave the stack exactly as it was"
        );
    }

    /// An address past the end is refused rather than clamped. A control-plane
    /// call or a stale menu naming a chart that has gone means something this
    /// cannot do, and moving a different chart would be worse than saying no.
    #[test]
    fn an_address_the_stack_does_not_have_is_refused() {
        let mut tab = tab_with(2);
        let before: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        for (from, to) in [(1_usize, 9_usize), (9, 1), (7, 8)] {
            assert!(
                !tab.move_context_pane(from, to),
                "moving {from} to {to} names a chart that is not there"
            );
        }
        assert_eq!(
            tab.time_panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            before
        );
    }

    /// Moving a chart onto itself changes nothing, and says so. A caller that
    /// retried a dropped call needs "nothing happened" to be distinguishable
    /// from "it worked".
    #[test]
    fn moving_a_chart_onto_itself_reports_no_change() {
        let mut tab = tab_with(2);
        assert!(!tab.move_context_pane(1, 1));
        assert!(!tab.move_context_pane(2, 2));
    }

    /// A single context chart has nowhere to go.
    #[test]
    fn a_lone_context_chart_cannot_be_reordered() {
        let mut tab = tab_with(1);
        assert!(!tab.move_context_pane(1, 1));
        assert!(!tab.move_context_pane(1, 2));
        assert_eq!(tab.time_panes.len(), 1);
    }
}

#[cfg(test)]
mod collapse_path_tests {
    use super::*;
    use crate::state::BarSpec;
    use quantick_feed as feed;
    use tokio::sync::mpsc;

    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2000);

    fn tab() -> Tab {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            crate::scratch::thread_dir("collapse-test").join(
                NEXT_DIR
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string(),
            ),
        )
    }

    /// Collapse reports whether it changed anything, so a caller that retried
    /// a dropped call can tell "nothing happened" from "it worked".
    #[test]
    fn collapsing_reports_change_and_is_idempotent() {
        let mut tab = tab();
        assert!(
            tab.set_context_collapsed(true),
            "the first collapse changes it"
        );
        assert!(
            !tab.set_context_collapsed(true),
            "the second is a no-op and says so"
        );
        assert!(tab.set_context_collapsed(false), "and it comes back");
        assert!(!tab.set_context_collapsed(false));
    }

    /// The width the column springs back to is never spent by putting it away
    /// — however many times it is put away.
    #[test]
    fn collapsing_never_spends_the_remembered_width() {
        let mut tab = tab();
        tab.split_fraction = 0.42;
        for _ in 0..3 {
            tab.set_context_collapsed(true);
            tab.set_context_collapsed(false);
        }
        assert_eq!(
            tab.split_fraction, 0.42,
            "the column came back at a width the trader never chose"
        );
    }

    /// A workspace saved with its charts put away reopens with them put away.
    ///
    /// The order inside `restore_canvas` is the whole test: `set_layout` opens
    /// the column it reveals, which is right for a menu click and wrong for a
    /// restore. Assigned before that call, the flag was overwritten every
    /// time — and the next `capture_arrangement` wrote the wrong answer back
    /// A three-pane workspace records one bar rule per context chart, and
    /// each chart opens on its own. Before `context_bars` existed the file
    /// kept one interval and both charts opened on it — the bottom chart lost
    /// its timeframe on every restart.
    #[test]
    fn a_restored_stack_opens_each_context_chart_on_its_own_interval() {
        let mut tab = tab();
        tab.restore_canvas(
            CanvasLayout::TimeTimeAndFlow,
            None,
            false,
            Some(PaneSide::Time(1)),
            &[60_000, 900_000],
            LegendFold::default(),
        );
        let config = crate::config::AppConfig {
            default_feed: "binance".to_owned(),
            default_symbol: "BTCUSDT".to_owned(),
            feeds: vec![],
            metatrader: Default::default(),
            paper: Default::default(),
            history: Default::default(),
        };
        let style = crate::style::ChartStyle::default();
        let mut ids = PaneIdAllocator::new();
        tab.apply_pending_layout(&config, &style, &mut ids);
        tab.apply_pending_layout(&config, &style, &mut ids);
        assert_eq!(tab.time_panes.len(), 2, "both charts were built");
        assert_eq!(
            tab.time_panes[0].spec.retained(crate::state::BarKind::Time),
            &crate::state::BarSpec::Time(60_000)
        );
        assert_eq!(
            tab.time_panes[1].spec.retained(crate::state::BarKind::Time),
            &crate::state::BarSpec::Time(900_000),
            "the bottom chart opened on the interval the file kept for it"
        );
        assert_eq!(
            tab.focused_side(),
            PaneSide::Time(1),
            "and the saved focus names the bottom chart"
        );
    }

    /// over the trader's file.
    #[test]
    fn a_restored_workspace_keeps_its_collapsed_column() {
        let mut tab = tab();
        tab.restore_canvas(
            CanvasLayout::TimeAndFlow,
            Some(0.42),
            true,
            Some(PaneSide::Flow),
            &[],
            LegendFold {
                flow: false,
                time: false,
            },
        );
        assert!(
            tab.context_collapsed,
            "the workspace recorded a collapsed column and it opened expanded"
        );
        assert_eq!(
            tab.split_fraction, 0.42,
            "and the width it springs back to is the saved one"
        );
    }

    /// Picking the arrangement that is already showing brings the column back.
    ///
    /// The picker lights a cell for the current layout; a trader who put the
    /// column away and then clicked that lit cell is asking for the charts the
    /// thumbnail draws. Answering with the 8 px rail is the chrome lying.
    #[test]
    fn re_picking_the_current_layout_brings_the_column_back() {
        let mut tab = tab();
        tab.set_layout(CanvasLayout::TimeAndFlow);
        assert!(tab.set_context_collapsed(true));

        tab.set_layout(CanvasLayout::TimeAndFlow);
        assert!(
            !tab.context_collapsed,
            "the lit cell promised two panes and handed back a rail"
        );
    }

    /// A layout with no flow pane has no column to put away, so the collapse
    /// flag must not decide its focus.
    ///
    /// Read the other way round, `Ctrl+0` on the Timeframe layout pointed
    /// focus at a pane nobody draws: `paper_hud_here` went false for the one
    /// chart on screen, so order entry, the ladder and the trade HUD all went
    /// dead — and with no split there is no rail to click to undo it.
    #[test]
    fn collapsing_never_takes_focus_off_the_only_chart_on_screen() {
        let mut tab = tab();
        tab.time_panes.push(ChartPane::time(
            300,
            crate::time_header::DEFAULT_INTERVAL_MS,
        ));
        tab.layout = CanvasLayout::Time;
        tab.context_collapsed = true;

        assert_eq!(
            tab.focused_side(),
            PaneSide::Time(0),
            "the only chart drawn has to be the one the chrome speaks for"
        );
    }
}

/// Which pane the order-entry gesture lands on, per pointer and per drag.
#[cfg(test)]
mod trading_pane_tests {
    use super::*;

    /// Every visible pane is a trading surface: the aim follows the pointer,
    /// so holding the buy modifier over a pane that is *not* focused places
    /// there — no focusing click first. This is the whole of "trade on any
    /// chart that is on screen".
    #[test]
    fn order_entry_follows_the_pointer_onto_an_unfocused_pane() {
        let top = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 200.0));
        let bottom = egui::Rect::from_min_max(egui::pos2(0.0, 200.0), egui::pos2(800.0, 400.0));
        // Pane 1 is the context chart on top, pane 0 the flow chart below.
        let panes = [(1 as PaneIndex, top), (0 as PaneIndex, bottom)];
        let focused: PaneIndex = 0;

        assert_eq!(
            trading_pane(Some(top.center()), &panes, false, None, focused),
            1,
            "the pointer is on the unfocused context pane, so the aim is there"
        );
        assert_eq!(
            trading_pane(Some(bottom.center()), &panes, false, None, focused),
            0,
            "and on the flow pane it is there"
        );
    }

    /// With the pointer nowhere near a pane — over the dock, off the window
    /// — the focused pane answers. That is also the unsplit case: one pane,
    /// always the answer, nothing about today changes.
    #[test]
    fn order_entry_falls_back_to_focus_with_the_pointer_off_the_canvas() {
        let flow = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let panes = [(0 as PaneIndex, flow)];
        assert_eq!(
            trading_pane(Some(egui::pos2(900.0, 200.0)), &panes, false, None, 0),
            0,
            "a pointer over the dock leaves entry where focus is"
        );
        assert_eq!(
            trading_pane(None, &panes, false, None, 0),
            0,
            "and so does no pointer at all"
        );
    }

    /// A drag keeps the pane it started in. The grabbed line is read against
    /// that pane's price scale; handing the gesture to a neighbour halfway
    /// through would reprice the order to a different scale — a stop that
    /// jumps because the hand strayed across a divider.
    #[test]
    fn a_paper_drag_stays_with_the_pane_it_started_in() {
        let top = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 200.0));
        let bottom = egui::Rect::from_min_max(egui::pos2(0.0, 200.0), egui::pos2(800.0, 400.0));
        let panes = [(1 as PaneIndex, top), (0 as PaneIndex, bottom)];

        // The drag began on pane 1; the pointer has since crossed into 0.
        assert_eq!(
            trading_pane(Some(bottom.center()), &panes, true, Some(1), 0),
            1,
            "the gesture stays where it started"
        );
        // Released, the pointer decides again immediately.
        assert_eq!(
            trading_pane(Some(bottom.center()), &panes, false, Some(1), 0),
            0,
            "and the pin is spent the moment the drag ends"
        );
    }
}
