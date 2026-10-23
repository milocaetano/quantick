//! Comparable workload retained unchanged across the ownership extraction.
use super::*;

#[test]
#[ignore = "manual before/after workspace lifetime workload"]
fn workspace_lifetime_frame_baseline() {
    const HISTORY: u64 = 2_000;
    const FRAMES: u64 = 120;
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    let (mut app, first, _commands, _book) = test_app();
    let second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    for events in [&first, &second.events] {
        events
            .try_send(FeedEvent::Backfilled((0..HISTORY).map(trade).collect()))
            .unwrap();
    }
    settled_frame(&mut app, &ctx);
    let feed_commands: Vec<_> = app
        .tab_reads()
        .tabs()
        .iter()
        .map(|tab| tab.commands.clone())
        .collect();
    let ids: Vec<_> = app
        .tab_reads()
        .tabs()
        .iter_with_ids()
        .map(|(id, _)| id)
        .collect();
    let pane_ids: Vec<_> = app
        .tab_reads()
        .tabs()
        .iter()
        .flat_map(|tab| tab.panes().map(|(pane, _)| pane.id))
        .collect();
    let workers = |app: &QuantickApp| {
        app.tab_reads()
            .tabs()
            .iter()
            .flat_map(|tab| tab.panes())
            .flat_map(|(pane, _)| {
                std::iter::once(pane.indicator_worker.progress().instance).chain(
                    pane.orderflow
                        .as_ref()
                        .map(|view| view.worker_progress().instance),
                )
            })
            .collect::<Vec<_>>()
    };
    let worker_instances = workers(&app);
    assert!(worker_instances.iter().all(Option::is_some));
    for (round, paint) in [true, false].into_iter().enumerate() {
        let mut elapsed = Vec::with_capacity(FRAMES as usize);
        let before = crate::work_meter::tally();
        let live_before: u64 = app
            .tab_reads()
            .tabs()
            .iter()
            .map(|tab| tab.live_trades)
            .sum();
        for frame in 0..FRAMES {
            let index = HISTORY + round as u64 * FRAMES + frame;
            first.try_send(FeedEvent::Live(trade(index))).unwrap();
            second
                .events
                .try_send(FeedEvent::Live(trade(index)))
                .unwrap();
            let start = Instant::now();
            if paint {
                run_frame(&mut app, &ctx);
            } else {
                // A minimized-workload proxy: the production intake adapter
                // runs without egui painting; native scheduling is not timed.
                app.drain_tabs();
            }
            elapsed.push(start.elapsed().as_nanos());
        }
        let heap = crate::work_meter::tally().since(before);
        elapsed.sort_unstable();
        let retained_trades: usize = app
            .tab_reads()
            .tabs()
            .iter()
            .flat_map(|tab| tab.panes())
            .map(|(pane, _)| pane.state.trades().len())
            .sum();
        let retained_tape_bytes: usize = app
            .tab_reads()
            .tabs()
            .iter()
            .flat_map(|tab| tab.panes())
            .map(|(pane, _)| {
                pane.state.trades().capacity() * std::mem::size_of::<quantick_engine::Trade>()
            })
            .sum();
        assert_eq!(
            app.tab_reads()
                .tabs()
                .iter()
                .map(|tab| tab.live_trades)
                .sum::<u64>()
                - live_before,
            2 * FRAMES,
            "visible and background sessions each ingest once without painting"
        );
        assert_eq!(
            app.tab_reads()
                .tabs()
                .iter_with_ids()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(
            app.tab_reads()
                .tabs()
                .iter()
                .flat_map(|tab| tab.panes().map(|(pane, _)| pane.id))
                .collect::<Vec<_>>(),
            pane_ids
        );
        assert_eq!(
            workers(&app),
            worker_instances,
            "actual worker instances survive each workload"
        );
        for (index, tab) in app.tab_reads().tabs().iter().enumerate() {
            assert!(feed_commands[index].same_channel(&tab.commands));
        }
        println!(
            "workspace_lifetime paint={paint} frames={FRAMES} median_ns={} allocs={} bytes={} reallocs={} copy_bytes={} retained_trades={retained_trades} retained_tape_bytes={retained_tape_bytes} feed_handles={} worker_instances={:?}",
            elapsed[elapsed.len() / 2],
            heap.allocs,
            heap.alloc_bytes,
            heap.reallocs,
            heap.realloc_copy_bytes,
            feed_commands.len(),
            worker_instances
        );
    }
}
