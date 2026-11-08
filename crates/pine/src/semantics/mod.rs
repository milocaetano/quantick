//! Eager kernel policy and state owned by one evaluation.
use crate::types::Value;
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
