//! Independent batch formulas: no production TA kernels or incremental runtime.
use quantick_indicators::IndicatorBar;

pub fn fixture(scenario: &str, count: usize) -> Vec<IndicatorBar> {
    (0..count)
        .map(|i| {
            let close = if scenario == "flat" {
                100.0
            } else if scenario == "zero_price" {
                0.0
            } else {
                100.0 + (i % 11) as f64 * 0.75 - (i % 7) as f64 * 1.25 + i as f64 / 8.0
            };
            let spread = if matches!(scenario, "flat" | "zero_price") {
                0.0
            } else {
                1.0 + (i % 3) as f64 / 4.0
            };
            let volume = match scenario {
                "zero_volume" => 0.0,
                "missing_volume" if (23..27).contains(&i) || i == 0 => f64::NAN,
                _ if i % 13 == 0 => 0.0,
                _ => 10.0 + (i % 9) as f64 * 3.0,
            };
            IndicatorBar {
                open_time: i as i64 * 1000,
                close_time: i as i64 * 1000 + 999,
                open: close - spread / 4.0,
                high: close + spread,
                low: close - spread * 0.75,
                close,
                buy_volume: volume * 0.625,
                sell_volume: volume * 0.375,
                trade_count: 2.0,
            }
        })
        .collect()
}

fn map(x: &[f64], f: impl Fn(usize, f64) -> f64) -> Vec<f64> {
    x.iter().enumerate().map(|(i, x)| f(i, *x)).collect()
}

fn window(x: &[f64], n: usize, f: impl Fn(&[f64]) -> f64) -> Vec<f64> {
    map(x, |i, _| {
        if i + 1 < n || x[i + 1 - n..=i].iter().any(|x| x.is_nan()) {
            f64::NAN
        } else {
            f(&x[i + 1 - n..=i])
        }
    })
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}
fn sma(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, mean)
}
fn sum(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| w.iter().sum())
}
fn high(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| {
        w.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    })
}
fn low(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| w.iter().copied().fold(f64::INFINITY, f64::min))
}
fn wma(x: &[f64], n: usize) -> Vec<f64> {
    window(x, n, |w| {
        w.iter()
            .enumerate()
            .map(|(i, x)| (i + 1) as f64 * x)
            .sum::<f64>()
            / (n * (n + 1) / 2) as f64
    })
}

// Closed-form geometric convolution, independently recomputed for each prefix.
// This deliberately avoids the incremental recurrence used by the runtime.
fn smooth(x: &[f64], n: usize, alpha: f64) -> Vec<f64> {
    map(x, |i, current| {
        let valid: Vec<_> = x[..=i].iter().copied().filter(|x| !x.is_nan()).collect();
        if current.is_nan() || valid.len() < n {
            return f64::NAN;
        }
        let tail = valid.len() - n;
        let seed = mean(&valid[..n]) * (1.0 - alpha).powi(tail as i32);
        seed + valid[n..]
            .iter()
            .enumerate()
            .map(|(j, x)| alpha * (1.0 - alpha).powi((tail - j - 1) as i32) * x)
            .sum::<f64>()
    })
}
fn ema(x: &[f64], n: usize) -> Vec<f64> {
    smooth(x, n, 2.0 / (n + 1) as f64)
}
fn rma(x: &[f64], n: usize) -> Vec<f64> {
    smooth(x, n, 1.0 / n as f64)
}
fn delta(x: &[f64], n: usize) -> Vec<f64> {
    map(
        x,
        |i, value| if i < n { f64::NAN } else { value - x[i - n] },
    )
}
pub fn hma(c: &[f64], length: usize) -> Vec<f64> {
    let half = wma(c, (length / 2).max(1));
    let full = wma(c, length);
    wma(
        &map(&full, |i, x| 2.0 * half[i] - x),
        (length as f64).sqrt().round().max(1.0) as usize,
    )
}
fn rsi(x: &[f64], n: usize) -> Vec<f64> {
    let differences = delta(x, 1);
    let gain = rma(
        &map(&differences, |_, x| if x.is_nan() { x } else { x.max(0.0) }),
        n,
    );
    let loss = rma(
        &map(
            &differences,
            |_, x| if x.is_nan() { x } else { (-x).max(0.0) },
        ),
        n,
    );
    map(&gain, |i, g| {
        if g == 0.0 && loss[i] == 0.0 {
            f64::NAN
        } else {
            100.0 - 100.0 / (1.0 + g / loss[i])
        }
    })
}
fn ratio(a: f64, b: f64) -> f64 {
    if b == 0.0 { f64::NAN } else { a / b }
}
fn channels(basis: Vec<f64>, width: &[f64]) -> Vec<Vec<f64>> {
    let upper = map(&basis, |i, x| x + width[i]);
    let lower = map(&basis, |i, x| x - width[i]);
    vec![basis, upper, lower]
}
fn stochastic(
    x: &[f64],
    highs: &[f64],
    lows: &[f64],
    n: usize,
    k: usize,
    d: usize,
) -> Vec<Vec<f64>> {
    let highs = high(highs, n);
    let lows = low(lows, n);
    let raw = map(x, |i, x| ratio(100.0 * (x - lows[i]), highs[i] - lows[i]));
    let k = sma(&raw, k);
    let d = sma(&k, d);
    vec![k, d]
}

/// `short` binds every integer input to 3; the default EMA keeps its legacy 9.
pub fn expected(id: &str, bars: &[IndicatorBar], short: bool) -> Vec<Vec<f64>> {
    let c: Vec<_> = bars.iter().map(|b| b.close).collect();
    let h: Vec<_> = bars.iter().map(|b| b.high).collect();
    let l: Vec<_> = bars.iter().map(|b| b.low).collect();
    let v: Vec<_> = bars.iter().map(|b| b.buy_volume + b.sell_volume).collect();
    let typical = map(&c, |i, c| (h[i] + l[i] + c) / 3.0);
    let tr = map(&h, |i, h| {
        if i == 0 {
            h - l[i]
        } else {
            (h - l[i])
                .max((h - c[i - 1]).abs())
                .max((l[i] - c[i - 1]).abs())
        }
    });
    let n = |default| if short { 3 } else { default };
    let values = match id {
        "sma" => sma(&c, n(20)),
        "ema" => ema(&c, n(9)),
        "wma" => wma(&c, n(20)),
        "rma" => rma(&c, n(14)),
        "vwma" => {
            let numerator = sum(&map(&c, |i, c| c * v[i]), n(20));
            let denominator = sum(&v, n(20));
            map(&numerator, |i, x| ratio(x, denominator[i]))
        }
        "hma" => hma(&c, n(9)),
        "rsi" => rsi(&c, n(14)),
        "stochastic" => return stochastic(&c, &h, &l, n(14), n(3), n(3)),
        "stochastic_rsi" => {
            let r = rsi(&c, n(14));
            return stochastic(&r, &r, &r, n(14), n(3), n(3));
        }
        "macd" => {
            let fast = ema(&c, n(12));
            let slow = ema(&c, n(26));
            let line = map(&fast, |i, x| x - slow[i]);
            let signal = ema(&line, n(9));
            let histogram = map(&line, |i, x| x - signal[i]);
            return vec![line, signal, histogram];
        }
        "bollinger_bands" => {
            let width = window(&c, n(20), |w| {
                let average = mean(w);
                2.0 * (w.iter().map(|x| (x - average).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
            });
            return channels(sma(&c, n(20)), &width);
        }
        "atr" => rma(&tr, n(14)),
        "adx" => return adx(&h, &l, &tr, n(14), n(14)),
        "cci" => window(&typical, n(20), |w| {
            let average = mean(w);
            ratio(
                w[w.len() - 1] - average,
                0.015 * w.iter().map(|x| (x - average).abs()).sum::<f64>() / w.len() as f64,
            )
        }),
        "roc" => map(&c, |i, x| {
            if i < n(12) {
                f64::NAN
            } else {
                ratio(100.0 * (x - c[i - n(12)]), c[i - n(12)])
            }
        }),
        "momentum" => delta(&c, n(10)),
        "williams_r" => {
            let upper = high(&h, n(14));
            let lower = low(&l, n(14));
            map(&c, |i, c| {
                ratio(-100.0 * (upper[i] - c), upper[i] - lower[i])
            })
        }
        "obv" => map(&c, |i, _| {
            if v[i].is_nan() {
                return f64::NAN;
            }
            (1..=i)
                .filter(|j| !v[*j].is_nan())
                .map(|j| {
                    let d = c[j] - c[j - 1];
                    if d > 0.0 {
                        v[j]
                    } else if d < 0.0 {
                        -v[j]
                    } else {
                        0.0
                    }
                })
                .sum()
        }),
        "mfi" => {
            let positive = map(&typical, |i, t| {
                if v[i].is_nan() {
                    f64::NAN
                } else if i > 0 && t > typical[i - 1] {
                    t * v[i]
                } else {
                    0.0
                }
            });
            let negative = map(&typical, |i, t| {
                if v[i].is_nan() {
                    f64::NAN
                } else if i > 0 && t < typical[i - 1] {
                    t * v[i]
                } else {
                    0.0
                }
            });
            let positive = sum(&positive, n(14));
            let negative = sum(&negative, n(14));
            map(&positive, |i, x| ratio(100.0 * x, x + negative[i]))
        }
        "cmf" => {
            let flow = map(&c, |i, c| {
                if h[i] == l[i] {
                    0.0 * v[i]
                } else {
                    (2.0 * c - h[i] - l[i]) / (h[i] - l[i]) * v[i]
                }
            });
            let flow = sum(&flow, n(20));
            let total = sum(&v, n(20));
            map(&flow, |i, x| ratio(x, total[i]))
        }
        "donchian_channels" => {
            let upper = high(&h, n(20));
            let lower = low(&l, n(20));
            return vec![map(&upper, |i, x| (x + lower[i]) / 2.0), upper, lower];
        }
        "keltner_channels" => {
            return channels(ema(&c, n(20)), &map(&rma(&tr, n(10)), |_, x| 2.0 * x));
        }
        _ => panic!("unknown fixture {id}"),
    };
    vec![values]
}

fn adx(h: &[f64], l: &[f64], tr: &[f64], length: usize, smoothing: usize) -> Vec<Vec<f64>> {
    let movement = |positive| {
        map(h, |i, _| {
            if i == 0 {
                return 0.0;
            }
            let up = h[i] - h[i - 1];
            let down = l[i - 1] - l[i];
            if positive && up > down && up > 0.0 {
                up
            } else if !positive && down > up && down > 0.0 {
                down
            } else {
                0.0
            }
        })
    };
    let range = rma(tr, length);
    let positive = rma(&movement(true), length);
    let negative = rma(&movement(false), length);
    let plus = map(&range, |i, x| {
        if x == 0.0 {
            0.0
        } else {
            100.0 * positive[i] / x
        }
    });
    let minus = map(&range, |i, x| {
        if x == 0.0 {
            0.0
        } else {
            100.0 * negative[i] / x
        }
    });
    let dx = map(&plus, |i, x| {
        if x + minus[i] == 0.0 {
            0.0
        } else {
            100.0 * (x - minus[i]).abs() / (x + minus[i])
        }
    });
    let average = rma(&dx, smoothing);
    vec![plus, minus, average]
}
