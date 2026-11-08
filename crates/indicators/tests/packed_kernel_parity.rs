//! Native arithmetic pinned before adapting kernel storage to a packed arena.
use quantick_indicators::ta;
use std::fmt::Write;

type Native = (&'static str, Box<dyn FnMut(f64, f64) -> f64>);

fn natives(length: usize) -> Vec<Native> {
    let mut kernels: Vec<Native> = Vec::new();
    macro_rules! window {
        ($($name:ident),+) => { $(
            let mut kernel = ta::$name::new(length);
            kernels.push((stringify!($name), Box::new(move |x, _| kernel.push(x))));
        )+ };
    }
    window!(
        Sma,
        Ema,
        Rma,
        Wma,
        Rsi,
        Stdev,
        Highest,
        Lowest,
        HighestBars,
        LowestBars,
        WindowSum,
        Change
    );
    let mut vwma = ta::Vwma::new(length);
    kernels.push(("Vwma", Box::new(move |x, volume| vwma.push(x, volume))));
    let mut tr = ta::Tr::new(true);
    kernels.push(("TrTrue", Box::new(move |x, _| tr.push(x + 1.0, x - 1.0, x))));
    let mut tr = ta::Tr::new(false);
    kernels.push((
        "TrFalse",
        Box::new(move |x, _| tr.push(x + 1.0, x - 1.0, x)),
    ));
    let mut atr = ta::Atr::new(length);
    kernels.push(("Atr", Box::new(move |x, _| atr.push(x + 1.0, x - 1.0, x))));
    let mut cum = ta::Cum::new();
    kernels.push(("Cum", Box::new(move |x, _| cum.push(x))));
    let mut since = ta::BarsSince::new();
    kernels.push(("BarsSince", Box::new(move |_, b| since.push(b > 0.0))));
    let mut when = ta::ValueWhen::new(length - 1);
    kernels.push(("ValueWhen", Box::new(move |x, b| when.push(b > 0.0, x))));
    let mut high = ta::PivotHigh::new(length, 1);
    kernels.push(("PivotHigh", Box::new(move |x, _| high.push(x))));
    let mut low = ta::PivotLow::new(length, 1);
    kernels.push(("PivotLow", Box::new(move |x, _| low.push(x))));
    macro_rules! cross {
        ($($name:ident),+) => { $(
            let mut kernel = ta::$name::new();
            kernels.push((stringify!($name), Box::new(move |x, b| f64::from(kernel.push(x, b)))));
        )+ };
    }
    cross!(Cross, CrossOver, CrossUnder);
    kernels
}

#[test]
fn native_kernel_storage_reference() {
    let mut actual = String::from("length,index,kernel,value_bits\n");
    for length in [1, 4] {
        let mut kernels = natives(length);
        for index in 0..64 {
            let x = if matches!(index, 3 | 11 | 37) {
                f64::NAN
            } else {
                f64::from((index * 7) % 19 - 9) / 3.0
            };
            let b = if index % 7 == 0 {
                0.0
            } else {
                f64::from(index % 5 - 2)
            };
            for (name, push) in &mut kernels {
                let value = push(x, b);
                if value.is_nan() {
                    writeln!(actual, "{length},{index},{name},na").unwrap();
                } else {
                    writeln!(actual, "{length},{index},{name},{:016x}", value.to_bits()).unwrap();
                }
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pine/tests/golden/ir/native-kernels.csv");

    assert_eq!(std::fs::read_to_string(path).unwrap(), actual);
}

fn layout(name: &str, length: usize) -> ta::packed::KernelLayout {
    use ta::packed::{KernelKind::*, KernelLayout};
    let kind = match name {
        "Sma" => Sma,
        "Ema" => Ema,
        "Rma" => Rma,
        "Wma" => Wma,
        "Vwma" => Vwma,
        "Rsi" => Rsi,
        "Stdev" => Stdev,
        "Highest" => Highest,
        "Lowest" => Lowest,
        "HighestBars" => HighestBars,
        "LowestBars" => LowestBars,
        "WindowSum" => WindowSum,
        "Change" => Change,
        "TrTrue" | "TrFalse" => Tr,
        "Atr" => Atr,
        "Cum" => Cum,
        "BarsSince" => BarsSince,
        "ValueWhen" => ValueWhen,
        "PivotHigh" => PivotHigh,
        "PivotLow" => PivotLow,
        "Cross" => Cross,
        "CrossOver" => CrossOver,
        "CrossUnder" => CrossUnder,
        _ => panic!("unknown kernel {name}"),
    };
    let n = match name {
        "TrTrue" => 1,
        "TrFalse" => 0,
        "ValueWhen" => length - 1,
        _ => length,
    };
    KernelLayout::new(kind, n, 1)
}

fn inputs(name: &str, x: f64, b: f64) -> [f64; 3] {
    match name {
        "TrTrue" | "TrFalse" | "Atr" => [x + 1.0, x - 1.0, x],
        "BarsSince" | "ValueWhen" => [f64::from(b > 0.0), x, 0.0],
        _ => [x, b, 0.0],
    }
}

fn assert_value(actual: f64, expected: f64, context: &str) {
    if expected.is_nan() {
        assert!(actual.is_nan(), "{context}: expected NaN, got {actual}");
    } else {
        assert_eq!(actual.to_bits(), expected.to_bits(), "{context}");
    }
}

#[test]
fn packed_matches_independently_pinned_native_outputs() {
    use std::collections::BTreeMap;
    let fixture = include_str!("../../pine/tests/golden/ir/native-kernels.csv");
    let mut states = BTreeMap::new();
    for line in fixture.lines().skip(1) {
        let fields: Vec<_> = line.split(',').collect();
        let length: usize = fields[0].parse().unwrap();
        let index: i32 = fields[1].parse().unwrap();
        let name = fields[2];
        let (layout, state) = states.entry((length, name)).or_insert_with(|| {
            let layout = layout(name, length);
            let mut state = vec![u64::MAX; layout.words()];
            layout.initialize(&mut state);
            (layout, state)
        });
        let x = if matches!(index, 3 | 11 | 37) {
            f64::NAN
        } else {
            f64::from((index * 7) % 19 - 9) / 3.0
        };
        let b = if index % 7 == 0 {
            0.0
        } else {
            f64::from(index % 5 - 2)
        };
        let expected = if fields[3] == "na" {
            f64::NAN
        } else {
            f64::from_bits(u64::from_str_radix(fields[3], 16).unwrap())
        };
        assert_value(layout.push(state, inputs(name, x, b)), expected, line);
    }
}

#[test]
fn packed_rings_queues_and_rollback_match_native_long_streams() {
    for length in [1, 2, 4, 17, 200] {
        for (name, mut native) in natives(length) {
            let layout = layout(name, length);
            // Adjacent sentinels detect accidental writes outside the state view.
            let mut arena = vec![0xdead_beef; layout.words() + 2];
            layout.initialize(&mut arena[1..=layout.words()]);
            for i in 0..3000 {
                let x = match i % 113 {
                    0..=2 => f64::NAN,
                    3 => -0.0,
                    4..=10 => 0.0,
                    11..=30 => 4.0,
                    31 if i >= 2900 => f64::INFINITY,
                    32 if i >= 2900 => f64::NEG_INFINITY,
                    _ => 104_532.0 + f64::from((i * 7) % 19 - 9) / 300.0,
                };
                let b = if i % 29 < 10 {
                    0.0
                } else {
                    f64::from(i % 5 - 2) / 10.0
                };
                let state = &mut arena[1..=layout.words()];
                let saved = state.to_vec();
                layout.push(state, [900.0, -13.0, 20.0]);
                layout.push(state, [f64::NAN; 3]);
                state.copy_from_slice(&saved);
                let actual = layout.push(state, inputs(name, x, b));
                let expected = native(x, b);
                assert_value(actual, expected, &format!("{name} length {length} bar {i}"));
                assert_eq!(arena[0], 0xdead_beef);
                assert_eq!(arena[layout.words() + 1], 0xdead_beef);
            }
        }
    }
}

#[test]
fn parameterless_and_zero_sided_pivot_layouts() {
    use ta::packed::{KernelKind::*, KernelLayout};
    for kind in [
        Cum, Cross, CrossOver, CrossUnder, BarsSince, Fixnan, Tr, ValueWhen, PivotHigh, PivotLow,
    ] {
        let layout = KernelLayout::new(kind, 0, 0);
        let mut state = vec![0; layout.words()];
        layout.initialize(&mut state);
        layout.push(&mut state, [1.0, 3.0, 2.0]);
    }
    let fix = KernelLayout::new(Fixnan, 0, 0);
    let mut state = vec![0; fix.words()];
    fix.initialize(&mut state);
    assert!(fix.push(&mut state, [f64::NAN; 3]).is_nan());
    assert_eq!(
        fix.push(&mut state, [-0.0, 0.0, 0.0]).to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        fix.push(&mut state, [f64::NAN; 3]).to_bits(),
        (-0.0_f64).to_bits()
    );
}
