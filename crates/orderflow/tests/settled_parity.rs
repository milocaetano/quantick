#[path = "support/fixtures.rs"]
mod fixtures;
#[path = "support/signature.rs"]
mod signature;

use quantick_orderflow::{
    EffectiveGrouping, GroupingWindow, project_live, project_settled, sweep_grouped_runs,
};

#[test]
fn frozen_workloads_preserve_complete_public_outputs() {
    for workload in 1..=3 {
        let fixture = fixtures::dense(workload);
        let history = &fixture.history;
        let before = format!(
            "{:?}{:?}{:?}{:?}",
            history.runs().collect::<Vec<_>>(),
            history.aggressions().collect::<Vec<_>>(),
            history.coverage_segments().collect::<Vec<_>>(),
            history.coverage_gaps().collect::<Vec<_>>()
        );
        let settled = project_settled(history, &fixture.timeline, fixture.prices);
        let live = project_live(history, &fixture.timeline, fixture.prices, &settled);
        let joined = settled.with_live(live.clone(), history.config());
        assert_eq!(joined, fixtures::whole(&fixture));
        let after = format!(
            "{:?}{:?}{:?}{:?}",
            history.runs().collect::<Vec<_>>(),
            history.aggressions().collect::<Vec<_>>(),
            history.coverage_segments().collect::<Vec<_>>(),
            history.coverage_gaps().collect::<Vec<_>>()
        );
        assert_eq!(before, after, "projection must not mutate retained inputs");
        assert_eq!(history.aggression_count(), 40_000);
        let signature = signature::exact(&settled, &live, &joined);
        let expected = [
            (1_152_657, 0xa6b78375ab1a6acd),
            (1_152_983, 0xa14b86292f435805),
            (2_729_248, 0x6e4ac1eab689b442),
        ][usize::from(workload - 1)];
        assert_eq!(
            (signature.len(), signature::sentinel(signature.as_bytes())),
            expected
        );
        assert_eq!(
            (settled.aggressions.len(), live.aggressions.len()),
            (398, 251)
        );
        assert_eq!(joined.folded_aggressions, 9_551);
        eprintln!(
            "SIGNATURE W{workload} bytes={} sentinel={:016x}",
            signature.len(),
            signature::sentinel(signature.as_bytes())
        );
        eprintln!(
            "COUNTS W{workload} runs={} active={} coverage={} gaps={} cells={} settled_marks={} live_marks={} settled_events={} live_events={} joined_events={} dropped_cells={} folded={} dropped_events={} reference={}",
            history.archived_run_count(),
            history.active_level_count(),
            history.coverage_segments().count(),
            history.coverage_gaps().count(),
            settled.cells.len(),
            settled.aggressions.len(),
            live.aggressions.len(),
            settled.liquidity_events.len(),
            live.liquidity_events.len(),
            joined.liquidity_events.len(),
            settled.dropped_cells,
            joined.folded_aggressions,
            joined.dropped_liquidity_events,
            settled.liquidity_reference
        );
        if workload == 3 {
            let (start_ms, end_ms) = fixture.timeline.timestamp_range().unwrap();
            let grouped = sweep_grouped_runs(
                history.runs_intersecting(start_ms, end_ms),
                history.coverage_segments(),
                EffectiveGrouping::resolve(
                    history.config().display_grouping,
                    history.config().price_grouping,
                    fixture.prices.high - fixture.prices.low,
                ),
                GroupingWindow {
                    start_ms,
                    end_ms,
                    open_run_end_ms: history.latest_book_ms().unwrap(),
                    price_low: fixture.prices.low,
                    price_high: fixture.prices.high,
                },
            );
            eprintln!(
                "DEPTH grouped_runs={} transitions={} unallocated_live_events={}",
                grouped.runs.len(),
                grouped.transitions.len(),
                settled.live_events.len()
            );
            assert_eq!(
                (history.archived_run_count(), history.active_level_count()),
                (239_760, 192)
            );
            assert_eq!(
                (grouped.runs.len(), grouped.transitions.len()),
                (39_537, 40_033)
            );
            assert_eq!(
                (settled.cells.len(), settled.dropped_cells),
                (1_024, 12_430)
            );
            assert_eq!(
                (settled.liquidity_events.len(), live.liquidity_events.len()),
                (1_024, 102)
            );
            assert_eq!(settled.live_events.len(), 102);
            assert_eq!(joined.dropped_liquidity_events, 19_052);
            assert_eq!(settled.gaps.len(), 2);
        }
        // Optional generated evidence path is explicit; normal tests write no
        // files. Baseline/candidate use different directories, then compare
        // these complete byte streams independently of sentinel assertions.
        if let Some(directory) = std::env::var_os("QUANTICK_PARITY_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("w{workload}.exact.txt")), &signature).unwrap();
            std::fs::write(
                directory.join(format!("w{workload}.config.txt")),
                format!("{:#?}", history.config()),
            )
            .unwrap();
        }
    }
}
