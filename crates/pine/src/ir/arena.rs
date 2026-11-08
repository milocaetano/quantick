//! Pointer-free mutable words. Offsets remain valid when the backing allocation grows.
use super::{ObjKind, Value};
use std::rc::Rc;

pub(crate) const GLOBALS: usize = 0;
pub(crate) const LOCALS: usize = 3;
pub(crate) const REGISTERS: usize = 6;
pub(crate) const RETURNS: usize = 9;
pub(crate) const FLAGS: usize = 12;
const HEADER: usize = 32;
const PATHS: usize = 15;
const HISTORIES: usize = 18;

#[derive(Clone)]
pub(crate) struct Arena {
    pub words: Vec<u64>,
}

fn index(word: u64) -> usize {
    usize::try_from(word).expect("arena offset fits address space")
}

impl Default for Arena {
    fn default() -> Self {
        Self {
            words: vec![0; HEADER],
        }
    }
}

impl Arena {
    pub fn path(&mut self, parent: u32, node: u32) -> u32 {
        let key = u64::from(parent) << 32 | u64::from(node);
        let (record, _) = self.record(PATHS, key, 3);
        u32::try_from(record).expect("path table fits u32")
    }

    /// Keyed records link through offsets, including their index in snapshots.
    pub fn record(&mut self, root: usize, key: u64, words: usize) -> (usize, bool) {
        if let Some(record) = self.find_record(root, key) {
            return (record, false);
        }
        let record = self.allocate(words);
        self.words[record] = self.words[root];
        self.words[record + 1] = key;
        self.words[root] = record as u64;
        (record, true)
    }

    fn find_record(&self, root: usize, key: u64) -> Option<usize> {
        let mut record = index(self.words[root]);
        while record != 0 {
            if self.words[record + 1] == key {
                return Some(record);
            }
            record = index(self.words[record]);
        }
        None
    }

    pub fn history(
        &mut self,
        key: (u32, u32),
        back: usize,
        current: Value,
        cap: usize,
        stage: bool,
    ) -> Value {
        let key = u64::from(key.0) << 32 | u64::from(key.1);
        if !stage && self.find_record(HISTORIES, key).is_none() {
            return if back == 0 {
                current
            } else {
                current.missing_like()
            };
        }
        // next, key, head, filled, staged, staged-value list, committed list.
        let (record, fresh) = self.record(HISTORIES, key, 11);
        if fresh {
            self.resize(record + 5, 1);
            self.resize(record + 8, cap);
        }
        if stage {
            self.set(record + 5, 0, current.clone());
            self.words[record + 4] = 1;
        }
        if back == 0 {
            return current;
        }
        if back > index(self.words[record + 3]) {
            return current.missing_like();
        }
        let head = index(self.words[record + 2]);
        self.get(record + 8, (head + cap - back) % cap)
    }

    pub fn commit_histories(&mut self) {
        let mut record = index(self.words[HISTORIES]);
        while record != 0 {
            let cap = self.len(record + 8);
            let head = index(self.words[record + 2]);
            let value = if self.words[record + 4] == 0 {
                Value::Na
            } else {
                self.get(record + 5, 0)
            };
            if cap > 0 {
                self.set(record + 8, head, value);
                self.words[record + 2] = ((head + 1) % cap) as u64;
                self.words[record + 3] = (index(self.words[record + 3]) + 1).min(cap) as u64;
            }
            self.words[record + 4] = 0;
            self.set(record + 5, 0, Value::Na);
            record = index(self.words[record]);
        }
    }

    /// Blocks carry capacity and occupancy in the same rollback image.
    pub fn allocate(&mut self, size: usize) -> usize {
        let mut block = HEADER;
        while block < self.words.len() {
            let capacity = index(self.words[block]);
            if self.words[block + 1] == 0 && capacity >= size {
                self.words[block + 1] = 1;
                self.words[block + 2..block + 2 + capacity].fill(0);
                return block + 2;
            }
            block += capacity + 2;
        }
        let start = self.words.len();
        self.words.resize(start + size + 2, 0);
        self.words[start] = size as u64;
        self.words[start + 1] = 1;
        start + 2
    }

    pub fn release(&mut self, offset: usize) {
        if offset != 0 {
            self.words[offset - 1] = 0;
        }
    }

    pub fn len(&self, list: usize) -> usize {
        index(self.words[list + 1])
    }

    fn cell(&self, list: usize, slot: usize) -> usize {
        assert!(slot < self.len(list));
        index(self.words[list]) + slot * 2
    }

    pub fn resize(&mut self, list: usize, length: usize) {
        let old_len = self.len(list);
        if length < old_len {
            for slot in length..old_len {
                self.set(list, slot, Value::Na);
            }
        }
        let capacity = index(self.words[list + 2]);
        if length > capacity {
            let new_capacity = length.max(capacity * 2).max(4);
            let old = index(self.words[list]);
            let next = self.allocate(new_capacity * 2);
            if old_len > 0 {
                self.words.copy_within(old..old + old_len * 2, next);
            }
            self.release(old);
            self.words[list] = next as u64;
            self.words[list + 2] = new_capacity as u64;
        }
        self.words[list + 1] = length as u64;
    }

    pub fn get(&self, list: usize, slot: usize) -> Value {
        let cell = self.cell(list, slot);
        match self.words[cell] {
            4 => {
                let start = index(self.words[cell + 1]);
                let mut cursor = start;
                decode(&self.words, &mut cursor)
            }
            tag => scalar(tag, self.words[cell + 1]),
        }
    }

    pub fn set(&mut self, list: usize, slot: usize, value: Value) {
        let cell = self.cell(list, slot);
        if self.words[cell] == 4 {
            self.release(index(self.words[cell + 1]));
        }
        let (tag, payload) = match value {
            Value::Str(_) | Value::Tuple(_) => {
                let mut encoded = Vec::new();
                encode(&value, &mut encoded);
                let start = self.allocate(encoded.len());
                self.words[start..start + encoded.len()].copy_from_slice(&encoded);
                (4, start as u64)
            }
            value => scalar_words(&value),
        };
        self.words[cell] = tag;
        self.words[cell + 1] = payload;
    }

    pub fn push_address(&mut self, address: usize) {
        let length = self.len(RETURNS);
        self.resize(RETURNS, length + 1);
        let cell = self.cell(RETURNS, length);
        self.words[cell + 1] = address as u64;
    }
    pub fn pop_address(&mut self) -> usize {
        let length = self.len(RETURNS);
        let cell = self.cell(RETURNS, length - 1);
        let address = index(self.words[cell + 1]);
        self.resize(RETURNS, length - 1);
        address
    }
    pub fn values(&self, list: usize) -> Vec<Value> {
        (0..self.len(list))
            .map(|slot| self.get(list, slot))
            .collect()
    }
    pub fn replace(&mut self, list: usize, values: Vec<Value>) {
        self.resize(list, values.len());
        for (slot, value) in values.into_iter().enumerate() {
            self.set(list, slot, value);
        }
    }
}

fn scalar_words(value: &Value) -> (u64, u64) {
    match value {
        Value::Na => (0, 0),
        Value::Num(n) => (1, n.to_bits()),
        Value::Bool(b) => (2, u64::from(*b)),
        Value::Color(c) => (3, u64::from(*c)),
        Value::Obj(kind, id) => (
            match kind {
                ObjKind::Line => 5,
                ObjKind::Box => 6,
                ObjKind::Label => 7,
            },
            u64::from(*id),
        ),
        Value::NaBool => (8, 0),
        Value::NaColor => (9, 0),
        Value::NaString => (10, 0),
        Value::Str(_) | Value::Tuple(_) => unreachable!("aggregate encoding"),
    }
}

fn scalar(tag: u64, payload: u64) -> Value {
    match tag {
        0 => Value::Na,
        1 => Value::Num(f64::from_bits(payload)),
        2 => Value::Bool(payload != 0),
        3 => Value::Color(u32::try_from(payload).expect("color")),
        5..=7 => Value::Obj(
            match tag {
                5 => ObjKind::Line,
                6 => ObjKind::Box,
                _ => ObjKind::Label,
            },
            u32::try_from(payload).expect("object id"),
        ),
        8 => Value::NaBool,
        9 => Value::NaColor,
        10 => Value::NaString,
        _ => unreachable!("packed scalar tag"),
    }
}

fn encode(value: &Value, words: &mut Vec<u64>) {
    match value {
        Value::Str(text) => {
            words.extend([11, text.len() as u64]);
            for bytes in text.as_bytes().chunks(8) {
                let mut word = [0; 8];
                word[..bytes.len()].copy_from_slice(bytes);
                words.push(u64::from_le_bytes(word));
            }
        }
        Value::Tuple(items) => {
            words.extend([12, items.len() as u64]);
            for item in items.iter() {
                encode(item, words);
            }
        }
        value => {
            let (tag, payload) = scalar_words(value);
            words.extend([tag, payload]);
        }
    }
}

fn decode(words: &[u64], cursor: &mut usize) -> Value {
    let tag = words[*cursor];
    let payload = words[*cursor + 1];
    *cursor += 2;
    match tag {
        11 => {
            let length = index(payload);
            let count = length.div_ceil(8);
            let mut bytes = Vec::with_capacity(count * 8);
            for word in &words[*cursor..*cursor + count] {
                bytes.extend(word.to_le_bytes());
            }
            bytes.truncate(length);
            *cursor += count;
            Value::Str(Rc::new(String::from_utf8(bytes).expect("stored UTF-8")))
        }
        12 => Value::Tuple(Rc::new(
            (0..index(payload)).map(|_| decode(words, cursor)).collect(),
        )),
        _ => scalar(tag, payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_words_own_nested_values_and_bounded_history() {
        let mut arena = Arena::default();
        arena.resize(GLOBALS, 2);
        let nested = Value::Tuple(Rc::new(vec![
            Value::Str(Rc::new("arena text".into())),
            Value::Bool(true),
        ]));
        arena.set(GLOBALS, 0, nested.clone());
        arena.history((0, 0), 0, nested.clone(), 3, true);
        arena.commit_histories();
        let mut restored = Arena {
            words: arena.words.clone(),
        };
        arena.set(GLOBALS, 0, Value::Na);
        assert_eq!(restored.get(GLOBALS, 0), nested);
        assert_eq!(restored.history((0, 0), 1, Value::Na, 3, false), nested);
        for i in 0..20 {
            restored.history((0, 0), 0, Value::Num(f64::from(i)), 3, true);
            restored.commit_histories();
        }
        let words = restored.words.len();
        for i in 20..200 {
            restored.history((0, 0), 0, Value::Num(f64::from(i)), 3, true);
            restored.commit_histories();
        }
        assert_eq!(restored.words.len(), words);
        assert_eq!(
            restored.history((0, 0), 3, Value::Na, 3, false),
            Value::Num(197.0)
        );
        assert_eq!(restored.history((0, 0), 4, Value::Na, 3, false), Value::Na);
    }

    #[test]
    fn overwritten_text_and_temporary_frames_reuse_storage() {
        let mut arena = Arena::default();
        arena.resize(GLOBALS, 1);
        for _ in 0..2 {
            for length in [2, 17, 9, 33] {
                arena.set(GLOBALS, 0, Value::Str(Rc::new("x".repeat(length))));
                arena.resize(LOCALS, 12);
                arena.set(LOCALS, 11, Value::Str(Rc::new("local".into())));
                arena.resize(LOCALS, 0);
            }
        }
        let words = arena.words.len();
        for _ in 0..100 {
            for length in [2, 17, 9, 33] {
                arena.set(GLOBALS, 0, Value::Str(Rc::new("x".repeat(length))));
                arena.resize(LOCALS, 12);
                assert_eq!(arena.get(LOCALS, 11), Value::Na);
                arena.set(LOCALS, 11, Value::Str(Rc::new("local".into())));
                arena.resize(LOCALS, 0);
            }
        }
        assert_eq!(arena.words.len(), words);
    }

    #[test]
    fn skipped_history_does_not_create_committed_samples() {
        let mut arena = Arena::default();
        assert_eq!(
            arena.history((1, 0), 1, Value::Bool(true), 3, false),
            Value::NaBool
        );
        arena.commit_histories();
        assert_eq!(
            arena.history((1, 0), 1, Value::Bool(true), 3, true),
            Value::NaBool
        );
    }
}
