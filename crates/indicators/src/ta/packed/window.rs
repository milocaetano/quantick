//! Rings and monotonic queues stored entirely in borrowed words.
use super::{KernelKind, zero_if_nan};
pub(super) fn read(s: &[u64], i: usize) -> f64 {
    f64::from_bits(s[i])
}
pub(super) fn write(s: &mut [u64], i: usize, x: f64) {
    s[i] = x.to_bits();
}
pub(super) fn full(s: &[u64], n: usize) -> bool {
    s[1] == n as u64
}
fn ready(s: &[u64], n: usize) -> bool {
    full(s, n) && s[2] == 0
}
// Header: next write, filled, NaN count, nonzero count. Payload follows.
pub(super) fn push_window(s: &mut [u64], n: usize, x: f64) -> Option<f64> {
    let head = s[0] as usize;
    let evicted = if full(s, n) {
        Some(read(s, 4 + head))
    } else {
        s[1] += 1;
        None
    };
    if let Some(e) = evicted {
        if e.is_nan() {
            s[2] -= 1;
        } else if e != 0.0 {
            s[3] -= 1;
        }
    }
    if x.is_nan() {
        s[2] += 1;
    } else if x != 0.0 {
        s[3] += 1;
    }
    write(s, 4 + head, x);
    s[0] = ((head + 1) % n) as u64;
    evicted
}
pub(super) fn average(s: &mut [u64], n: usize, x: f64, kind: KernelKind) -> f64 {
    let base = n + 4;
    let mut sum = read(s, base);
    let mut second = read(s, base + 1);
    let mut offset = read(s, base + 2);
    if kind == KernelKind::Stdev && offset.is_nan() && !x.is_nan() {
        offset = x;
        write(s, base + 2, offset);
    }
    let shifted = |v: f64| if v.is_nan() { 0.0 } else { v - offset };
    let clean = if kind == KernelKind::Stdev {
        shifted(x)
    } else {
        zero_if_nan(x)
    };
    let evicted = push_window(s, n, x);
    if kind == KernelKind::Wma {
        if let Some(e) = evicted {
            second = second - sum + n as f64 * clean;
            sum = sum - zero_if_nan(e) + clean;
        } else {
            second += s[1] as f64 * clean;
            sum += clean;
        }
    } else {
        if let Some(e) = evicted {
            let e = if kind == KernelKind::Stdev {
                shifted(e)
            } else {
                zero_if_nan(e)
            };
            sum -= e;
            if kind == KernelKind::Stdev {
                second -= e * e;
            }
        }
        sum += clean;
        if kind == KernelKind::Stdev {
            second += clean * clean;
        }
    }
    write(s, base, sum);
    write(s, base + 1, second);
    if !ready(s, n) {
        return f64::NAN;
    }
    match kind {
        KernelKind::Sma => sum / n as f64,
        KernelKind::Wma => second / (n * (n + 1) / 2) as f64,
        KernelKind::Stdev => {
            let mean = sum / n as f64;
            (second / n as f64 - mean * mean).max(0.0).sqrt()
        }
        _ => sum,
    }
}
pub(super) fn extreme(s: &mut [u64], n: usize, x: f64, kind: KernelKind) -> f64 {
    let b = n + 4;
    let i = s[b];
    s[b] += 1;
    push_window(s, n, x);
    let capacity = n + 1; // append before expiry, matching native queue order
    let mut head = s[b + 1] as usize;
    let mut count = s[b + 2] as usize;
    let is_max = matches!(kind, KernelKind::Highest | KernelKind::HighestBars);
    if !x.is_nan() {
        while count > 0 {
            let slot = (head + count - 1) % capacity;
            let v = read(s, b + 4 + 2 * slot);
            if if is_max { v <= x } else { v >= x } {
                count -= 1;
            } else {
                break;
            }
        }
        let slot = (head + count) % capacity;
        s[b + 3 + 2 * slot] = i;
        write(s, b + 4 + 2 * slot, x);
        count += 1;
    }
    let min = (i + 1).saturating_sub(n as u64);
    while count > 0 && s[b + 3 + 2 * head] < min {
        head = (head + 1) % capacity;
        count -= 1;
    }
    s[b + 1] = head as u64;
    s[b + 2] = count as u64;
    if !ready(s, n) || count == 0 {
        return f64::NAN;
    }
    if matches!(kind, KernelKind::HighestBars | KernelKind::LowestBars) {
        -((i - s[b + 3 + 2 * head]) as f64)
    } else {
        read(s, b + 4 + 2 * head)
    }
}
pub(super) fn pivot(s: &mut [u64], left: usize, right: usize, x: f64, high: bool) -> f64 {
    let n = left + 1 + right;
    push_window(s, n, x);
    if !ready(s, n) {
        return f64::NAN;
    }
    let candidate_slot = (s[0] as usize + left) % n;
    let candidate = read(s, 4 + candidate_slot);
    for slot in 0..n {
        if slot == candidate_slot {
            continue;
        }
        let value = read(s, 4 + slot);
        if !(if high {
            candidate > value
        } else {
            candidate < value
        }) {
            return f64::NAN;
        }
    }
    candidate
}
