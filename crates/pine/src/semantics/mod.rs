//! Eager kernel policy and state owned by one evaluation.
use crate::types::Value;
use quantick_indicators::ta;
use std::collections::BTreeMap;

pub(crate) struct Run<K> {
    pub skipped: bool,
    pub arguments: Option<Vec<Value>>,
    pub initial: BTreeMap<(u32, u32), Option<K>>,
    pub values: BTreeMap<(u32, u32), (Vec<Value>, Value)>,
}

impl<K> Default for Run<K> {
    fn default() -> Self {
        Self {
            skipped: false,
            arguments: None,
            initial: BTreeMap::new(),
            values: BTreeMap::new(),
        }
    }
}

/// Independent composition of the existing EMA kernels.
#[derive(Clone)]
pub(crate) struct Macd {
    pub lengths: [usize; 3],
    fast: ta::Ema,
    slow: ta::Ema,
    signal: ta::Ema,
}
impl Macd {
    pub fn new(lengths: [usize; 3]) -> Self {
        Self {
            lengths,
            fast: ta::Ema::new(lengths[0]),
            slow: ta::Ema::new(lengths[1]),
            signal: ta::Ema::new(lengths[2]),
        }
    }
    pub fn push(&mut self, source: f64) -> Value {
        let line = self.fast.push(source) - self.slow.push(source);
        let signal = self.signal.push(line);
        Value::Tuple(std::rc::Rc::new(vec![
            Value::Num(line),
            Value::Num(signal),
            Value::Num(line - signal),
        ]))
    }
}
