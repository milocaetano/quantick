//! Stateful arithmetic operates directly on slices of the rollback arena.
use super::{Arena, Value};
use quantick_indicators::ta::packed::{KernelKind, KernelLayout};
use std::rc::Rc;

const KERNELS: usize = 24;
const LENGTHS: usize = 27;
fn key(key: (u32, u32)) -> u64 {
    u64::from(key.0) << 32 | u64::from(key.1)
}
fn index(word: u64) -> usize {
    usize::try_from(word).expect("kernel arena offset")
}

impl Arena {
    pub fn pin_length(&mut self, identity: (u32, u32), length: usize) -> usize {
        let (record, fresh) = self.record(LENGTHS, key(identity), 3);
        if fresh {
            self.words[record + 2] = length as u64;
        }
        index(self.words[record + 2])
    }

    fn kernel_record(&self, identity: (u32, u32)) -> Option<usize> {
        let mut record = index(self.words[KERNELS]);
        while record != 0 {
            if self.words[record + 1] == key(identity) {
                return Some(record);
            }
            record = index(self.words[record]);
        }
        None
    }

    pub fn kernel_snapshot(&self, identity: (u32, u32)) -> Option<Vec<u64>> {
        let record = self.kernel_record(identity)?;
        let capacity = index(self.words[record - 2]);
        Some(self.words[record + 2..record + capacity].to_vec())
    }

    pub fn restore_kernel(&mut self, identity: (u32, u32), snapshot: Option<&[u64]>) {
        let Some(record) = self.kernel_record(identity) else {
            return;
        };
        if let Some(snapshot) = snapshot {
            self.words[record + 2..record + 2 + snapshot.len()].copy_from_slice(snapshot);
        } else {
            let mut link = KERNELS;
            while index(self.words[link]) != record {
                link = index(self.words[link]);
            }
            self.words[link] = self.words[record];
            self.release(record);
        }
    }

    pub fn push_kernel(
        &mut self,
        identity: (u32, u32),
        kind: KernelKind,
        length: usize,
        auxiliary: usize,
        inputs: [f64; 3],
    ) -> f64 {
        let existing = self.kernel_record(identity);
        let requested = KernelLayout::new(
            kind,
            existing.map_or(length, |record| index(self.words[record + 2])),
            existing.map_or(auxiliary, |record| index(self.words[record + 3])),
        );
        let (record, fresh) = existing.map_or_else(
            || self.record(KERNELS, key(identity), 4 + requested.words()),
            |record| (record, false),
        );
        if fresh {
            self.words[record + 2] = length as u64;
            self.words[record + 3] = auxiliary as u64;
            requested.initialize(&mut self.words[record + 4..record + 4 + requested.words()]);
        }
        let layout = requested;
        layout.push(
            &mut self.words[record + 4..record + 4 + layout.words()],
            inputs,
        )
    }

    pub fn push_macd(
        &mut self,
        identity: (u32, u32),
        lengths: [usize; 3],
        source: f64,
    ) -> Option<Value> {
        let layouts = lengths.map(|length| KernelLayout::new(KernelKind::Ema, length, 0));
        let count: usize = layouts.iter().map(KernelLayout::words).sum();
        let (record, fresh) = self.record(KERNELS, key(identity), 5 + count);
        if fresh {
            for (i, length) in lengths.iter().enumerate() {
                self.words[record + 2 + i] = *length as u64;
            }
        } else if lengths
            .iter()
            .enumerate()
            .any(|(i, length)| self.words[record + 2 + i] != *length as u64)
        {
            return None;
        }
        let mut offset = record + 5;
        let mut results = [0.0; 3];
        for (i, layout) in layouts.iter().enumerate() {
            let words = &mut self.words[offset..offset + layout.words()];
            if fresh {
                layout.initialize(words);
            }
            let input = if i == 2 {
                results[0] - results[1]
            } else {
                source
            };
            results[i] = layout.push(words, [input, 0.0, 0.0]);
            offset += layout.words();
        }
        let line = results[0] - results[1];
        Some(Value::Tuple(Rc::new(vec![
            Value::Num(line),
            Value::Num(results[2]),
            Value::Num(line - results[2]),
        ])))
    }
}
