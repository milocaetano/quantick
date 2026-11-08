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
    if std::env::var_os("QUANTICK_UPDATE_PACKED_REFERENCE").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), actual);
}
