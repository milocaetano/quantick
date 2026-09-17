#[path = "support/fixtures.rs"]
mod fixtures;

/// W3 supplements the two unchanged unit benchmarks. This executable never
/// links the allocation counter; fixture construction and output checks are
/// outside the timed loop. Output destruction is inside, as in W1 and W2.
#[test]
#[ignore]
fn bench_settled_over_dense_depth_and_prints() {
    let fixture = fixtures::dense(3);
    let project =
        || quantick_orderflow::project_settled(&fixture.history, &fixture.timeline, fixture.prices);
    for _ in 0..3 {
        std::hint::black_box(project());
    }
    let runs = 30;
    let started = std::time::Instant::now();
    for _ in 0..runs {
        std::hint::black_box(project());
    }
    let per_run = started.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    let output = project();
    assert!(!output.cells.is_empty());
    assert!(!output.liquidity_events.is_empty());
    assert!(output.dropped_cells > 0);
    assert_eq!(output.gaps.len(), 2);
    assert!(!fixtures::whole(&fixture).aggressions.is_empty());
    eprintln!(
        "BENCH settled_depth_ms_per_frame={per_run:.6} cells={} marks={} events={}",
        output.cells.len(),
        output.aggressions.len(),
        output.liquidity_events.len()
    );
}
