//! The shipped BTC tape looks over unchanged, attributed Binance recordings.
//! Native cells are admitted only after their 100 ms window closes, so every
//! measured group has the same causal history as an incrementally drawn tape.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr as _;
use std::time::Instant;

use quantick_engine::{Bar, Side, Trade};
use quantick_orderflow::projection::{
    TapeDotFrame, TapeDotGeometry, TapeDotMemory, TapeDotView, TapeHorizontalGeometry,
    project_live, project_settled,
};
use quantick_orderflow::{
    AggressionPrimitive, BarTimeline, BubbleSizeReference, DotSizing, HeatmapConfig,
    LiquidityHistory, LiveEdge, PriceWindow, VolumeDots,
};
use quantick_stores::bubble_presets;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

const WINDOW_MS: i64 = 60_000;
const CELL_MS: i64 = 100;

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn recordings() -> [(&'static str, &'static str); 4] {
    [
        ("spot quiet", include_str!("fixtures/btc/spot-quiet.csv")),
        ("spot busy", include_str!("fixtures/btc/spot-busy.csv")),
        (
            "perpetual quiet",
            include_str!("fixtures/btc/perpetual-quiet.csv"),
        ),
        (
            "perpetual busy",
            include_str!("fixtures/btc/perpetual-busy.csv"),
        ),
    ]
}

fn trades(csv: &str) -> Vec<Trade> {
    csv.lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            Trade {
                agg_id: fields[0].parse().unwrap(),
                timestamp_ms: fields[1].parse().unwrap(),
                price: dec(fields[2]),
                quantity: dec(fields[3]),
                side: match fields[4] {
                    "buy" => Side::Buy,
                    "sell" => Side::Sell,
                    _ => panic!("recorded aggressor side"),
                },
            }
        })
        .collect()
}

struct Recording {
    prints: Vec<Trade>,
    native: Vec<AggressionPrimitive>,
    config: HeatmapConfig,
    prices: PriceWindow,
    start_ms: i64,
}

impl Recording {
    fn new(name: &str, csv: &str) -> Self {
        Self::with_display(name, csv, false)
    }

    fn with_display(name: &str, csv: &str, legacy: bool) -> Self {
        let prints = trades(csv);
        let perpetual = name.starts_with("perpetual");
        let preset = if perpetual {
            "Binance BTC perpetual spheres"
        } else {
            "Binance BTC Spot spheres"
        };
        let mut config = HeatmapConfig {
            enabled: false,
            show_aggressions: true,
            price_grouping: if perpetual { dec("0.1") } else { dec("0.01") },
            ..HeatmapConfig::default()
        };
        bubble_presets::embedded()
            .get(preset)
            .unwrap()
            .apply_to(&mut config);
        if legacy {
            config.bubbles.size_reference = BubbleSizeReference::VisibleP99;
            config.bubbles.size_reference_quantity = 100.0;
        }
        config.live_lane.enabled = true;
        config.live_lane.show_aggressions = true;
        assert!(config.native_tape() && config.live_lane.stable_size);
        assert!(!config.volume_dots.auto_full);
        let mut history = LiquidityHistory::new(config.clone());
        for trade in &prints {
            history.record_aggression(trade);
        }
        // The user's 50 tick chart: only the final partial bar stays forming.
        let mut bars: Vec<_> = prints
            .chunks(50)
            .map(|chunk| {
                let mut bar = Bar::opened_by(&chunk[0]);
                for trade in &chunk[1..] {
                    bar.extend(trade);
                }
                bar
            })
            .collect();
        let partial = bars.pop().unwrap();
        let start_ms = prints[0].timestamp_ms.div_euclid(WINDOW_MS) * WINDOW_MS;
        let timeline = BarTimeline::from_bars(
            0,
            &bars,
            Some(&partial),
            Some(LiveEdge {
                now_ms: start_ms + WINDOW_MS,
                window_ms: WINDOW_MS,
                reference_ms: WINDOW_MS,
                on_newest_bar: true,
            }),
        )
        .with_full_lane_coverage();
        let prices = PriceWindow::new(
            prints.iter().map(|trade| trade.price).min().unwrap() - Decimal::ONE,
            prints.iter().map(|trade| trade.price).max().unwrap() + Decimal::ONE,
        )
        .unwrap();
        let dots = VolumeDots {
            native_tape: true,
            tape_window_ms: CELL_MS,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            bars: bars
                .iter()
                .chain([&partial])
                .map(|bar| (bar.open_time, bar.close_time))
                .collect(),
            forming: Some(partial.open_time),
        };
        let settled = project_settled(&history, &timeline, prices, Some(&dots));
        let live = project_live(&history, &timeline, prices, &settled, Some(&dots));
        let native = settled
            .with_live(live, &config)
            .aggressions
            .into_iter()
            .filter(|mark| mark.live)
            .collect();
        Self {
            prints,
            native,
            config,
            prices,
            start_ms,
        }
    }

    fn sizing(&self) -> DotSizing {
        DotSizing {
            native_tape: true,
            tape_column_px: 1.0,
            candle_column_px: 1.0,
            px_per_price: 1.0,
            typed_full: Decimal::from_f64(self.config.volume_dots.full_quantity),
        }
    }

    fn view(&self, now_ms: i64, width: f32, height: f32) -> TapeDotView {
        let geometry = TapeHorizontalGeometry::resolve(width, height, &self.config.bubbles);
        TapeDotView {
            now_ms,
            window_ms: WINDOW_MS,
            dot_window_ms: CELL_MS,
            evicted_through_ms: None,
            prices: self.prices,
            geometry: TapeDotGeometry {
                left_x: 0.0,
                right_x: 1.0,
                width_px: geometry.span_px,
                height_px: height,
            },
        }
    }

    fn draw(&self, memory: &mut TapeDotMemory, view: TapeDotView) -> TapeDotFrame {
        let admitted: Vec<_> = self
            .native
            .iter()
            .filter(|mark| mark.last_timestamp_ms < view.now_ms)
            .cloned()
            .collect();
        memory.project(
            &admitted,
            view,
            self.sizing(),
            &self.config.bubbles,
            &self.config.live_lane,
            &[],
        )
    }

    fn replay(&self) -> (TapeDotMemory, TapeDotFrame, TapeDotFrame) {
        let mut memory = TapeDotMemory::default();
        let mut middle = None;
        for offset in (0..WINDOW_MS).step_by(CELL_MS as usize) {
            let frame = self.draw(&mut memory, self.view(self.start_ms + offset, 640.0, 400.0));
            if offset == WINDOW_MS / 2 {
                middle = Some(frame);
            }
        }
        let final_frame = self.draw(
            &mut memory,
            self.view(self.start_ms + WINDOW_MS, 640.0, 400.0),
        );
        (memory, middle.unwrap(), final_frame)
    }

    fn diameter(&self, frame: &TapeDotFrame, mark: &AggressionPrimitive) -> f32 {
        let mut bubbles = self.config.bubbles.clone();
        bubbles.max_radius = frame.max_radius;
        2.0 * self
            .sizing()
            .draw(
                &bubbles,
                &self.config.live_lane,
                mark,
                (frame.full_quantity, frame.full_quantity),
            )
            .1
    }
}

fn assert_facts(recording: &Recording, marks: &[AggressionPrimitive]) -> BTreeSet<u64> {
    let source: BTreeMap<_, _> = recording
        .prints
        .iter()
        .map(|trade| (trade.agg_id, trade))
        .collect();
    let mut ids = BTreeSet::new();
    for mark in marks {
        let members: Vec<_> = mark
            .agg_ids
            .iter()
            .map(|id| {
                assert!(ids.insert(*id), "a source execution cannot appear twice");
                source[id]
            })
            .collect();
        let quantity: Decimal = members.iter().map(|trade| trade.quantity).sum();
        let buy: Decimal = members
            .iter()
            .filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity)
            .sum();
        assert_eq!(mark.quantity, quantity);
        assert_eq!(mark.buy_quantity, buy);
        assert_eq!(mark.trade_count, members.len());
        assert_eq!(
            mark.timestamp_quantity,
            members
                .iter()
                .map(|trade| Decimal::from(trade.timestamp_ms) * trade.quantity)
                .sum::<Decimal>()
        );
        assert!((mark.buy_share - (buy / quantity).to_f32().unwrap()).abs() < 0.000001);
    }
    ids
}

fn assert_unchanged(
    recording: &Recording,
    before: &TapeDotFrame,
    after: &TapeDotFrame,
    closed_before: i64,
    visible_after: i64,
) -> usize {
    let mut compared = 0;
    for old in before.marks.iter().filter(|mark| {
        mark.last_timestamp_ms < closed_before
            && mark.timestamp_quantity / mark.quantity >= Decimal::from(visible_after)
    }) {
        let current = after
            .marks
            .iter()
            .find(|mark| mark.agg_ids == old.agg_ids)
            .expect("a consolidated group keeps its exact membership");
        assert_eq!(current.quantity, old.quantity);
        assert_eq!(current.buy_quantity, old.buy_quantity);
        assert_eq!(current.side, old.side);
        assert_eq!(current.trade_count, old.trade_count);
        assert_eq!(current.price, old.price);
        assert_eq!(current.timestamp_quantity, old.timestamp_quantity);
        assert_eq!(
            recording.diameter(after, current),
            recording.diameter(before, old),
            "a consolidated group keeps its drawn diameter"
        );
        compared += 1;
    }
    assert!(compared > 0, "exercise consolidated groups");
    compared
}

#[test]
fn shipped_btc_tapes_preserve_source_facts_and_consolidated_sizes() {
    for (name, csv) in recordings() {
        let recording = Recording::new(name, csv);
        let source_ids: BTreeSet<_> = recording.prints.iter().map(|trade| trade.agg_id).collect();
        assert_eq!(
            assert_facts(&recording, &recording.native),
            source_ids,
            "{name}"
        );
        let (memory, middle, final_frame) = recording.replay();
        assert_eq!(
            assert_facts(&recording, &final_frame.marks),
            source_ids,
            "{name}"
        );
        // At 640 px a 15 px disc's frontier is under 4 seconds. Ten seconds
        // gives this assertion only historical groups, never the growing tip.
        assert_unchanged(
            &recording,
            &middle,
            &final_frame,
            recording.start_ms + 20_000,
            recording.start_ms,
        );
        for (width, height, window_ms, price_shift) in [
            (640.0, 800.0, 60_000, Decimal::ZERO),
            (640.0, 400.0, 30_000, Decimal::ZERO),
            (250.0, 400.0, 60_000, Decimal::ZERO),
            (640.0, 400.0, 60_000, Decimal::from(10)),
        ] {
            let mut changed = memory.clone();
            let mut view = recording.view(recording.start_ms + WINDOW_MS, width, height);
            view.window_ms = window_ms;
            view.prices = PriceWindow::new(
                recording.prices.low + price_shift,
                recording.prices.high + price_shift,
            )
            .unwrap();
            let frame = recording.draw(&mut changed, view);
            assert_facts(&recording, &frame.marks);
            assert_unchanged(
                &recording,
                &final_frame,
                &frame,
                recording.start_ms + 50_000,
                view.now_ms - view.window_ms,
            );
        }
        // Replay seek restarts the display epoch; replaying the same prefix
        // through the same causal frames must reconstruct the same groups.
        let (_, rewound_middle, replayed) = recording.replay();
        assert_eq!(rewound_middle.marks, middle.marks, "{name} rewind");
        assert_eq!(replayed.marks, final_frame.marks, "{name} replay");
    }
}

#[test]
fn shipped_fixed_btc_scale_distinguishes_formerly_saturated_groups_without_regrouping() {
    for (name, csv) in recordings() {
        let fixed = Recording::new(name, csv);
        let legacy = Recording::with_display(name, csv, true);
        let (_, old_middle, old_frame) = legacy.replay();
        let (_, new_middle, new_frame) = fixed.replay();
        assert_eq!(
            new_middle.marks, old_middle.marks,
            "{name}: same causal prefix groups"
        );
        assert_eq!(
            new_frame.marks, old_frame.marks,
            "{name}: same identities, quantities and sides"
        );
        assert_eq!(new_frame.full_quantity, old_frame.full_quantity);
        assert_eq!(new_frame.max_radius, old_frame.max_radius);
        assert_eq!(
            assert_facts(&fixed, &new_frame.marks),
            assert_facts(&legacy, &old_frame.marks)
        );
        let display_full = fixed
            .sizing()
            .tape_display_full_quantity(&fixed.config.bubbles, &fixed.config.live_lane)
            .expect("the shipped BTC Tape selects its fixed display reference");
        let floor = 2.0 * fixed.config.bubbles.min_radius;
        let cap = 2.0 * new_frame.max_radius;
        let mut former_caps = BTreeMap::new();
        for mark in &new_frame.marks {
            let diameter = fixed.diameter(&new_frame, mark);
            assert!(
                diameter >= floor,
                "{name}: the configured minimum stays visible"
            );
            assert!(
                mark.quantity < display_full,
                "{name}: this recorded range fits the calibration"
            );
            assert!(
                diameter < cap,
                "{name}: no recorded group remains saturated"
            );
            if mark.quantity >= old_frame.full_quantity {
                assert_eq!(legacy.diameter(&old_frame, mark), cap);
                former_caps.insert(mark.quantity, diameter);
            }
        }
        let sizes: Vec<_> = former_caps.values().copied().collect();
        assert!(
            sizes.windows(2).all(|pair| pair[0] < pair[1]),
            "{name}: every distinct formerly capped quantity earns a distinct diameter"
        );
        if name.ends_with("busy") {
            assert!(
                sizes.len() > 1,
                "exercise several formerly capped quantities"
            );
            assert!(
                sizes.last().unwrap() - sizes[0] > 5.0,
                "{name}: the former cap now spans visibly different diameters"
            );
        }
    }
}

fn distribution(values: impl IntoIterator<Item = f64>) -> String {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_by(f64::total_cmp);
    assert!(!values.is_empty());
    [0, 10, 25, 50, 75, 90, 95, 100]
        .map(|percent| format!("{:.6}", values[(values.len() - 1) * percent / 100]))
        .join(",")
}

#[test]
#[ignore = "manual shipped-preset quantity and drawn-diameter report; run without concurrent builds"]
fn report_shipped_btc_tape_range() {
    eprintln!(
        "BTC_TAPE quantiles=min,p10,p25,p50,p75,p90,p95,max; diameters in logical pixels; timing=complete_native_cell_projection,not_app_frame_time"
    );
    for (name, csv) in recordings() {
        let recording = Recording::new(name, csv);
        let started = Instant::now();
        let (memory, _, frame) = recording.replay();
        let replay_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let diameters: Vec<_> = frame
            .marks
            .iter()
            .map(|mark| recording.diameter(&frame, mark))
            .collect();
        let mut ranked: Vec<_> = frame.marks.iter().zip(&diameters).collect();
        ranked.sort_by_key(|(mark, _)| mark.quantity);
        let upper = &ranked[ranked.len() * 3 / 4..];
        let upper_quantities: BTreeSet<_> = upper.iter().map(|(mark, _)| mark.quantity).collect();
        let upper_diameters: BTreeSet<_> = upper
            .iter()
            .map(|(_, diameter)| diameter.to_bits())
            .collect();
        let upper_pixels: BTreeSet<_> = upper
            .iter()
            .map(|(_, diameter)| diameter.round() as u32)
            .collect();
        let capped = diameters
            .iter()
            .filter(|diameter| **diameter >= 2.0 * frame.max_radius * 0.999)
            .count();
        let total: Decimal = frame.marks.iter().map(|mark| mark.quantity).sum();
        let buy: Decimal = frame.marks.iter().map(|mark| mark.buy_quantity).sum();
        eprintln!(
            "BTC_TAPE {name} trades={} native={} groups={} total={total} buy={buy} sell={} full={} configured_min={} configured_max={} effective_max={} capped={capped} upper_n={} upper_quantities={} upper_diameters={} upper_pixel_bins={} replay_ms={replay_ms:.3}",
            recording.prints.len(),
            recording.native.len(),
            frame.marks.len(),
            total - buy,
            frame.full_quantity,
            recording.config.bubbles.min_radius,
            recording.config.bubbles.max_radius,
            frame.max_radius,
            upper.len(),
            upper_quantities.len(),
            upper_diameters.len(),
            upper_pixels.len()
        );
        for (label, values) in [
            (
                "trade_qty",
                recording
                    .prints
                    .iter()
                    .map(|trade| trade.quantity.to_f64().unwrap())
                    .collect::<Vec<_>>(),
            ),
            (
                "native_qty",
                recording
                    .native
                    .iter()
                    .map(|mark| mark.quantity.to_f64().unwrap())
                    .collect(),
            ),
            (
                "group_qty",
                frame
                    .marks
                    .iter()
                    .map(|mark| mark.quantity.to_f64().unwrap())
                    .collect(),
            ),
            (
                "diameter",
                diameters.iter().map(|value| f64::from(*value)).collect(),
            ),
        ] {
            eprintln!("BTC_TAPE {name} {label}={}", distribution(values));
        }
        let mut legacy_bubbles = recording.config.bubbles.clone();
        legacy_bubbles.size_reference = BubbleSizeReference::VisibleP99;
        legacy_bubbles.max_radius = frame.max_radius;
        let legacy_diameters: Vec<_> = frame
            .marks
            .iter()
            .map(|mark| {
                2.0 * recording
                    .sizing()
                    .draw(
                        &legacy_bubbles,
                        &recording.config.live_lane,
                        mark,
                        (frame.full_quantity, frame.full_quantity),
                    )
                    .1
            })
            .collect();
        let legacy_capped: Vec<_> = frame
            .marks
            .iter()
            .zip(&legacy_diameters)
            .filter(|(_, diameter)| **diameter >= 2.0 * frame.max_radius * 0.999)
            .collect();
        let legacy_capped_quantity: Decimal =
            legacy_capped.iter().map(|(mark, _)| mark.quantity).sum();
        eprintln!(
            "BTC_TAPE_LEGACY_ON_SAME_GROUPS {name} diameter={} capped={} capped_quantity={legacy_capped_quantity}",
            distribution(legacy_diameters.iter().map(|diameter| f64::from(*diameter))),
            legacy_capped.len()
        );
        for factor in [1.0, 4.0, 8.0, 16.0] {
            let display_full = recording.config.volume_dots.full_quantity * factor;
            let mut candidate = recording.config.bubbles.clone();
            candidate.size_reference = BubbleSizeReference::Fixed;
            candidate.size_reference_quantity = display_full;
            candidate.max_radius = frame.max_radius;
            let candidate_diameters: Vec<_> = frame
                .marks
                .iter()
                .map(|mark| {
                    2.0 * recording
                        .sizing()
                        .draw(
                            &candidate,
                            &recording.config.live_lane,
                            mark,
                            (frame.full_quantity, frame.full_quantity),
                        )
                        .1
                })
                .collect();
            let saturation_qty: Decimal = frame
                .marks
                .iter()
                .zip(&candidate_diameters)
                .filter(|(_, diameter)| **diameter >= 2.0 * frame.max_radius * 0.999)
                .map(|(mark, _)| mark.quantity)
                .sum();
            let distinct_upper: BTreeSet<_> = frame
                .marks
                .iter()
                .zip(&candidate_diameters)
                .filter(|(mark, _)| mark.quantity >= upper[0].0.quantity)
                .map(|(_, diameter)| diameter.to_bits())
                .collect();
            let upper_pixels: BTreeSet<_> = frame
                .marks
                .iter()
                .zip(&candidate_diameters)
                .filter(|(mark, _)| mark.quantity >= upper[0].0.quantity)
                .map(|(_, diameter)| diameter.round() as u32)
                .collect();
            let capped = candidate_diameters
                .iter()
                .filter(|diameter| **diameter >= 2.0 * frame.max_radius * 0.999)
                .count();
            let tiny = candidate_diameters
                .iter()
                .filter(|diameter| **diameter < 2.0)
                .count();
            let readable = candidate_diameters
                .iter()
                .filter(|diameter| **diameter >= 2.0 * candidate.readable_min_radius)
                .count();
            eprintln!(
                "BTC_TAPE_CANDIDATE {name} display_full={display_full} diameter={} capped={capped} saturated_qty={saturation_qty} upper_diameters={} upper_pixel_bins={} under_2px={tiny} readable={readable}",
                distribution(
                    candidate_diameters
                        .iter()
                        .map(|diameter| f64::from(*diameter))
                ),
                distinct_upper.len(),
                upper_pixels.len(),
            );
        }
        let started = Instant::now();
        let mut redraw = memory;
        for _ in 0..100 {
            std::hint::black_box(recording.draw(
                &mut redraw,
                recording.view(recording.start_ms + WINDOW_MS, 640.0, 400.0),
            ));
        }
        eprintln!(
            "BTC_TAPE {name} redraw_avg_ms={:.3}",
            started.elapsed().as_secs_f64() * 10.0
        );
    }
}
