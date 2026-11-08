//! Numeric array reductions. Missing samples are excluded from calculations.
use super::{
    Op,
    runtime::{create, number, receiver},
    view,
};
use crate::{eval::arena::Arena, types::Value};
pub(crate) fn truncate(arena: &mut Arena, value: &Value) {
    if let Value::Collection(false, handle) = value {
        let handle = *handle as usize;
        for i in 0..view::len(arena, handle) {
            if let Value::Num(n) = view::get(arena, handle, i) {
                view::set(arena, handle, i, Value::Num(n.trunc()));
            }
        }
    }
}
fn numbers(arena: &Arena, record: usize) -> Vec<f64> {
    view::values(arena, record)
        .iter()
        .filter_map(Value::as_num)
        .filter(|v| !v.is_nan())
        .collect()
}
fn variance(values: &[f64], biased: bool) -> f64 {
    let count = values.len() as f64;
    let denominator = count - if biased { 0.0 } else { 1.0 };
    if denominator <= 0.0 {
        return f64::NAN;
    }
    let mean = values.iter().sum::<f64>() / count;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / denominator
}
fn biased(args: &[Value], index: usize) -> bool {
    args.get(index).is_none_or(|v| v.as_cond().unwrap_or(false))
}
pub(super) fn execute(
    arena: &mut Arena,
    op: Op,
    args: &[Value],
    record: usize,
) -> Result<Value, String> {
    use Op::*;
    let mut nums = numbers(arena, record);
    if op == Abs {
        let values = view::values(arena, record)
            .into_iter()
            .map(|v| Value::Num(v.as_num().unwrap_or(f64::NAN).abs()))
            .collect();
        return Ok(create(arena, false, values));
    }
    if op == Standardize {
        let values = if nums.is_empty() {
            vec![]
        } else {
            let mean = nums.iter().sum::<f64>() / nums.len() as f64;
            let deviation = variance(&nums, true).sqrt();
            view::values(arena, record)
                .into_iter()
                .map(|v| Value::Num((v.as_num().unwrap_or(f64::NAN) - mean) / deviation))
                .collect()
        };
        return Ok(create(arena, false, values));
    }
    if op == Covariance {
        return covariance(arena, args, record);
    }
    let percentage = if matches!(op, PercentileLinear | PercentileNearest) {
        let p = args[1].as_num().unwrap_or(f64::NAN);
        if !p.is_finite() || !(0.0..=100.0).contains(&p) {
            return Err("array percentile percentage must be within 0..100".into());
        }
        p
    } else {
        0.0
    };
    let nth = if matches!(op, Min | Max) {
        args.get(1).map_or(Ok(0), number)?
    } else {
        0
    };
    let selected = if op == PercentRank {
        let i = number(&args[1])?;
        if i >= view::len(arena, record) {
            return Err("array.percentrank index out of bounds".into());
        }
        view::get(arena, record, i).as_num().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    };
    if nums.is_empty() {
        return Ok(Value::Num(f64::NAN));
    }
    let count = nums.len();
    let value = match op {
        Sum => nums.iter().sum(),
        Avg => nums.iter().sum::<f64>() / count as f64,
        Stdev => variance(&nums, biased(args, 1)).sqrt(),
        Variance => variance(&nums, biased(args, 1)),
        PercentRank => {
            if selected.is_nan() {
                f64::NAN
            } else {
                nums.iter().filter(|v| **v <= selected).count() as f64 * 100.0 / count as f64
            }
        }
        _ => {
            nums.sort_by(f64::total_cmp);
            match op {
                Min => nums.get(nth).copied().unwrap_or(f64::NAN),
                Max => count.checked_sub(nth + 1).map_or(f64::NAN, |i| nums[i]),
                Range => nums[count - 1] - nums[0],
                Median => {
                    if count.is_multiple_of(2) {
                        (nums[count / 2 - 1] + nums[count / 2]) / 2.0
                    } else {
                        nums[count / 2]
                    }
                }
                Mode => mode(&nums),
                PercentileNearest => {
                    nums[((percentage * count as f64 / 100.0).ceil() as usize).saturating_sub(1)]
                }
                PercentileLinear => {
                    let rank = percentage * (count - 1) as f64 / 100.0;
                    let low = rank.floor() as usize;
                    nums[low] + (nums[rank.ceil() as usize] - nums[low]) * rank.fract()
                }
                _ => unreachable!("numeric operation"),
            }
        }
    };
    Ok(Value::Num(value))
}
fn mode(sorted: &[f64]) -> f64 {
    let mut best = sorted[0];
    let mut maximum = 0;
    let mut start = 0;
    while start < sorted.len() {
        let mut end = start + 1;
        while end < sorted.len() && sorted[end] == sorted[start] {
            end += 1;
        }
        if end - start > maximum {
            maximum = end - start;
            best = sorted[start];
        }
        start = end;
    }
    best
}
fn covariance(arena: &Arena, args: &[Value], record: usize) -> Result<Value, String> {
    let other = receiver(&args[1], false)?;
    if view::len(arena, record) != view::len(arena, other) {
        return Err("array.covariance requires equal array sizes".into());
    }
    let pairs: Vec<_> = (0..view::len(arena, record))
        .filter_map(|i| {
            let a = view::get(arena, record, i).as_num()?;
            let b = view::get(arena, other, i).as_num()?;
            if a.is_nan() || b.is_nan() {
                None
            } else {
                Some((a, b))
            }
        })
        .collect();
    let count = pairs.len() as f64;
    let denominator = count - if biased(args, 2) { 0.0 } else { 1.0 };
    if denominator <= 0.0 {
        return Ok(Value::Num(f64::NAN));
    }
    let ma = pairs.iter().map(|v| v.0).sum::<f64>() / count;
    let mb = pairs.iter().map(|v| v.1).sum::<f64>() / count;
    Ok(Value::Num(
        pairs.iter().map(|(a, b)| (a - ma) * (b - mb)).sum::<f64>() / denominator,
    ))
}
