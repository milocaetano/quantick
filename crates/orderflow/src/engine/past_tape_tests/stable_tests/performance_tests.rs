//! Manual timing separates retained-history cost from source construction.

use super::*;
use std::hint::black_box;

#[test]
#[ignore = "manual measurement; run serially with no concurrent compilation"]
fn stable_held_cache_cost_after_sixty_four_visits() {
    const WINDOW: i64 = 3_000;
    const ITERATIONS: u32 = 1_000;
    let trades = tape();
    let mut visited = PastTapeMemory::default();
    let first_end = 6_000;
    // Visiting adjacent blocks fills the offscreen budget while leaving the
    // currently visible blocks outside that budget. Setup is not timed.
    for step in 0..66 {
        let end = first_end + step * WINDOW;
        let source = past_at_window(&trades, end, WINDOW);
        black_box(stable_frame(&mut visited, &source, view(end, &source)));
    }
    let end = first_end + 65 * WINDOW;
    let source = past_at_window(&trades, end, WINDOW);
    let current = view(end, &source);
    let held = stable_frame(&mut visited, &source, current);
    let visited_buckets = visited.cached_block_count();
    assert!(visited_buckets >= 64, "the fixture must fill the cache");

    let mut fresh = PastTapeMemory::default();
    let cold = stable_frame(&mut fresh, &source, current);
    let fresh_buckets = fresh.cached_block_count();
    assert!(fresh_buckets <= 2);
    // The window and geometry never changed, so each fetched block was
    // settled under identical inputs in both histories.
    assert_eq!(held.marks, cold.marks);
    assert_eq!(held.max_radius, cold.max_radius);
    assert_eq!(held.full_quantity, cold.full_quantity);
    let quantity: Decimal = held.marks.iter().map(|mark| mark.quantity).sum();
    let buy: Decimal = held.marks.iter().map(|mark| mark.buy_quantity).sum();
    assert!(quantity > Decimal::ZERO);

    // All repetitions reuse the same immutable source and view. Alternating
    // the two memories reduces systematic order bias in this manual sample.
    let mut visited_elapsed = std::time::Duration::ZERO;
    let mut fresh_elapsed = std::time::Duration::ZERO;
    for step in 0..ITERATIONS {
        let measure = |memory: &mut PastTapeMemory, elapsed: &mut std::time::Duration| {
            let started = Instant::now();
            black_box(stable_frame(memory, &source, current));
            *elapsed += started.elapsed();
        };
        if step % 2 == 0 {
            measure(&mut visited, &mut visited_elapsed);
            measure(&mut fresh, &mut fresh_elapsed);
        } else {
            measure(&mut fresh, &mut fresh_elapsed);
            measure(&mut visited, &mut visited_elapsed);
        }
    }
    assert_eq!(
        stable_frame(&mut visited, &source, current).marks,
        held.marks
    );
    assert_eq!(stable_frame(&mut fresh, &source, current).marks, cold.marks);
    println!(
        "stable_held_cache profile={} fixture_rows={} iterations={} visible_groups={} quantity={} buy={} visited_buckets={} fresh_buckets={} visited_total_us={} fresh_total_us={} visited_per_frame_us={:.3} fresh_per_frame_us={:.3}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        trades.len(),
        ITERATIONS,
        held.marks.len(),
        quantity,
        buy,
        visited_buckets,
        fresh_buckets,
        visited_elapsed.as_micros(),
        fresh_elapsed.as_micros(),
        visited_elapsed.as_secs_f64() * 1_000_000.0 / f64::from(ITERATIONS),
        fresh_elapsed.as_secs_f64() * 1_000_000.0 / f64::from(ITERATIONS),
    );
}
