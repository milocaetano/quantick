//! The retained trade tape: every print a chart holds, oldest first, stored
//! in fixed-size chunks so that growing it never copies what it holds.
//!
//! A contiguous `Vec<Trade>` must move its whole contents when it outgrows
//! its block. On a live chart that is one print whose ingest copies the
//! entire session on the UI thread: 117 MB at print 2,097,152 (25 ms on the
//! reference host) and 235 MB at 4,194,304 (52 ms) — a dropped frame, then
//! three. [`TradeTape`] keeps its prints in chunks of [`CHUNK_TRADES`], each
//! allocated once at full size and never reallocated, so an append writes
//! one print and at most allocates one new chunk; no print already held ever
//! moves.
//!
//! **Reading it.** Positions are the same as the `Vec`'s: print `i` is the
//! `i`-th oldest, [`TradeTape::get`] and indexing find it with a shift and a
//! mask, and [`TradeTape::range`] walks any stretch front to back or back to
//! front. A reader that wants contiguous memory — a bulk copy, a fold over a
//! slice — takes [`TradeTape::slices`], the stretch as at most one slice per
//! chunk; nothing hands out the whole tape as one slice, because nothing
//! holds it as one. [`TradeSeq`] is the positional read the feed's history
//! reach walks with, over this tape or over a plain slice alike.
//!
//! **Memory.** Every chunk but the last is full, so the tape reserves at most
//! one chunk it does not use, where a doubling vector reserves up to the
//! whole tape again. The chunk directory is one pointer triple per chunk: 61
//! of them at the live envelope's 3,960,000 prints.

use std::iter::FusedIterator;
use std::ops::{Bound, Index, RangeBounds};

use crate::Trade;

/// Bits of a print's position that pick its offset inside a chunk.
const CHUNK_SHIFT: u32 = 16;

/// Prints per chunk: 65,536, which is 3.5 MiB of 56-byte trades.
///
/// A power of two, so a position splits into a chunk and an offset with a
/// shift and a mask. Small beside the tapes it holds — under 2 % of the live
/// envelope's 222 MB tape, which bounds the reserved-but-unused tail of the
/// last chunk — and large enough that a pane opens a chunk rarely: one every
/// 3.6 minutes at the envelope's sustained 300 prints a second, 61 of them
/// at its edge.
pub const CHUNK_TRADES: usize = 1 << CHUNK_SHIFT;

const OFFSET_MASK: usize = CHUNK_TRADES - 1;

/// Every print a chart holds, oldest first, in fixed-size chunks.
///
/// See the [module docs](self) for why chunks, and what an append costs.
#[derive(Debug, Default)]
pub struct TradeTape {
    /// Every chunk holds exactly [`CHUNK_TRADES`] prints except the last,
    /// which holds at least one; each was allocated at [`CHUNK_TRADES`]
    /// capacity and is never grown. An empty tape has no chunks.
    chunks: Vec<Vec<Trade>>,
    len: usize,
}

impl TradeTape {
    /// An empty tape; it allocates nothing until the first print.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }

    /// Prints held.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the tape holds no print.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Prints the tape can hold before it allocates again: whole chunks, so
    /// never more than [`CHUNK_TRADES`] − 1 beyond [`len`](Self::len).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.chunks.len() * CHUNK_TRADES
    }

    /// Print `index`, oldest first, or `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Trade> {
        if index < self.len {
            Some(&self.chunks[index >> CHUNK_SHIFT][index & OFFSET_MASK])
        } else {
            None
        }
    }

    /// The oldest print held.
    #[must_use]
    pub fn first(&self) -> Option<&Trade> {
        self.chunks.first().and_then(|chunk| chunk.first())
    }

    /// The newest print held.
    #[must_use]
    pub fn last(&self) -> Option<&Trade> {
        self.chunks.last().and_then(|chunk| chunk.last())
    }

    /// Append one print. Writes it into the last chunk, or allocates one new
    /// chunk when that is full; never moves a print already held.
    pub fn push(&mut self, trade: Trade) {
        self.open_chunk_if_full().push(trade);
        self.len += 1;
    }

    /// Append `trades`, in order: chunk by chunk, allocating each new chunk
    /// once and never moving a print already held.
    pub fn extend_from_slice(&mut self, mut trades: &[Trade]) {
        while !trades.is_empty() {
            let chunk = self.open_chunk_if_full();
            let take = (CHUNK_TRADES - chunk.len()).min(trades.len());
            chunk.extend_from_slice(&trades[..take]);
            self.len += take;
            trades = &trades[take..];
        }
    }

    /// Put `older` in front of everything held, keeping every chunk but the
    /// last full.
    ///
    /// A copy of the whole tape, which is why only a rare path calls it —
    /// paging older history in, which then rebuilds every bar from the tape
    /// anyway. Each old chunk is freed as soon as it has been copied, so the
    /// peak is the tape plus one chunk rather than two tapes.
    pub fn prepend(&mut self, older: &[Trade]) {
        if older.is_empty() {
            return;
        }
        let held = std::mem::take(self);
        self.extend_from_slice(older);
        for chunk in held.chunks {
            self.extend_from_slice(&chunk);
        }
    }

    /// Every print, oldest first.
    #[must_use]
    pub fn iter(&self) -> Iter<'_> {
        self.range(..)
    }

    /// The prints at positions `range`, oldest first. Panics as slicing a
    /// `Vec` would when the range is out of order or runs past the end.
    #[must_use]
    pub fn range(&self, range: impl RangeBounds<usize>) -> Iter<'_> {
        let (start, end) = self.bounds(&range);
        Iter {
            front: [].iter(),
            rest: self.slices_between(start, end),
            back: [].iter(),
            len: end - start,
        }
    }

    /// Every print from position `start` on — what arrived since a reader
    /// last looked, when it remembers how many it had seen.
    #[must_use]
    pub fn since(&self, start: usize) -> Iter<'_> {
        self.range(start..)
    }

    /// The newest `n` prints, oldest of them first; the whole tape when it
    /// holds fewer.
    #[must_use]
    pub fn last_n(&self, n: usize) -> Iter<'_> {
        self.range(self.len.saturating_sub(n)..)
    }

    /// The prints at positions `range` as contiguous slices, at most one per
    /// chunk, oldest first: for a reader that copies or folds in bulk. Panics
    /// as [`range`](Self::range) does.
    #[must_use]
    pub fn slices(&self, range: impl RangeBounds<usize>) -> Slices<'_> {
        let (start, end) = self.bounds(&range);
        self.slices_between(start, end)
    }

    /// The position of the first print for which `pred` is false, given that
    /// it is true for every print before that one and false for every print
    /// after — `slice::partition_point`, over the chunks: a binary search
    /// for the chunk, then one inside it.
    pub fn partition_point(&self, mut pred: impl FnMut(&Trade) -> bool) -> usize {
        let whole = self
            .chunks
            .partition_point(|chunk| chunk.last().is_some_and(&mut pred));
        match self.chunks.get(whole) {
            Some(chunk) => (whole << CHUNK_SHIFT) + chunk.partition_point(pred),
            None => self.len,
        }
    }

    /// `range` as a start and an end position, checked as slicing a `Vec`
    /// would check it.
    fn bounds(&self, range: &impl RangeBounds<usize>) -> (usize, usize) {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len,
        };
        assert!(
            start <= end,
            "tape range starts at {start} but ends at {end}"
        );
        assert!(
            end <= self.len,
            "tape range end {end} is out of range for a tape of {} prints",
            self.len
        );
        (start, end)
    }

    /// The prints at positions `start..end`, already checked, as slices.
    fn slices_between(&self, start: usize, end: usize) -> Slices<'_> {
        if start == end {
            return Slices::empty();
        }
        let last = end - 1;
        let (first_chunk, last_chunk) = (start >> CHUNK_SHIFT, last >> CHUNK_SHIFT);
        let (from, to) = (start & OFFSET_MASK, (last & OFFSET_MASK) + 1);
        if first_chunk == last_chunk {
            return Slices {
                head: &self.chunks[first_chunk][from..to],
                middle: [].iter(),
                tail: &[],
            };
        }
        Slices {
            head: &self.chunks[first_chunk][from..],
            middle: self.chunks[first_chunk + 1..last_chunk].iter(),
            tail: &self.chunks[last_chunk][..to],
        }
    }

    /// The last chunk, after opening a new one if it is full (or if there is
    /// none): the only allocation an append ever makes.
    fn open_chunk_if_full(&mut self) -> &mut Vec<Trade> {
        if self
            .chunks
            .last()
            .is_none_or(|chunk| chunk.len() == CHUNK_TRADES)
        {
            self.chunks.push(Vec::with_capacity(CHUNK_TRADES));
        }
        let last = self.chunks.len() - 1;
        &mut self.chunks[last]
    }
}

impl Index<usize> for TradeTape {
    type Output = Trade;

    fn index(&self, index: usize) -> &Trade {
        self.get(index).unwrap_or_else(|| {
            panic!(
                "tape index {index} is out of range for a tape of {} prints",
                self.len
            )
        })
    }
}

impl<'a> IntoIterator for &'a TradeTape {
    type Item = &'a Trade;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// Appends clones of the prints, in order, as [`TradeTape::push`] does.
impl<'a> Extend<&'a Trade> for TradeTape {
    fn extend<I: IntoIterator<Item = &'a Trade>>(&mut self, trades: I) {
        for trade in trades {
            self.push(trade.clone());
        }
    }
}

impl FromIterator<Trade> for TradeTape {
    fn from_iter<I: IntoIterator<Item = Trade>>(trades: I) -> Self {
        let mut tape = Self::new();
        for trade in trades {
            tape.push(trade);
        }
        tape
    }
}

/// A stretch of a [`TradeTape`] as contiguous slices, at most one per chunk,
/// oldest first; see [`TradeTape::slices`]. Never yields an empty slice.
#[derive(Clone, Debug)]
pub struct Slices<'a> {
    head: &'a [Trade],
    middle: std::slice::Iter<'a, Vec<Trade>>,
    tail: &'a [Trade],
}

impl Slices<'_> {
    fn empty() -> Self {
        Self {
            head: &[],
            middle: [].iter(),
            tail: &[],
        }
    }
}

impl<'a> Iterator for Slices<'a> {
    type Item = &'a [Trade];

    fn next(&mut self) -> Option<&'a [Trade]> {
        if !self.head.is_empty() {
            return Some(std::mem::take(&mut self.head));
        }
        if let Some(chunk) = self.middle.next() {
            return Some(chunk);
        }
        (!self.tail.is_empty()).then(|| std::mem::take(&mut self.tail))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = usize::from(!self.head.is_empty())
            + self.middle.len()
            + usize::from(!self.tail.is_empty());
        (len, Some(len))
    }
}

impl<'a> DoubleEndedIterator for Slices<'a> {
    fn next_back(&mut self) -> Option<&'a [Trade]> {
        if !self.tail.is_empty() {
            return Some(std::mem::take(&mut self.tail));
        }
        if let Some(chunk) = self.middle.next_back() {
            return Some(chunk);
        }
        (!self.head.is_empty()).then(|| std::mem::take(&mut self.head))
    }
}

impl ExactSizeIterator for Slices<'_> {}
impl FusedIterator for Slices<'_> {}

/// A stretch of a [`TradeTape`], print by print; see [`TradeTape::range`].
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    front: std::slice::Iter<'a, Trade>,
    rest: Slices<'a>,
    back: std::slice::Iter<'a, Trade>,
    len: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Trade;

    fn next(&mut self) -> Option<&'a Trade> {
        loop {
            if let Some(trade) = self.front.next() {
                self.len -= 1;
                return Some(trade);
            }
            match self.rest.next() {
                Some(slice) => self.front = slice.iter(),
                None => {
                    let trade = self.back.next()?;
                    self.len -= 1;
                    return Some(trade);
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(trade) = self.back.next_back() {
                self.len -= 1;
                return Some(trade);
            }
            match self.rest.next_back() {
                Some(slice) => self.back = slice.iter(),
                None => {
                    let trade = self.front.next_back()?;
                    self.len -= 1;
                    return Some(trade);
                }
            }
        }
    }
}

impl ExactSizeIterator for Iter<'_> {}
impl FusedIterator for Iter<'_> {}

/// Oldest-first prints read by position: what a reader needs that walks a
/// tape by index — a binary search, a walk backwards from an anchor — rather
/// than front to back. A [`TradeTape`], a slice and a vector all read the
/// same through it.
pub trait TradeSeq {
    /// Prints held.
    fn len(&self) -> usize;

    /// Print `index`, oldest first, or `None` past the end.
    fn get(&self, index: usize) -> Option<&Trade>;

    /// Whether no print is held.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The oldest print held.
    fn first(&self) -> Option<&Trade> {
        self.get(0)
    }

    /// `slice::partition_point`: the position of the first print for which
    /// `pred` is false, given it is true for every print before that one.
    fn partition_point<P: FnMut(&Trade) -> bool>(&self, mut pred: P) -> usize {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if self.get(mid).is_some_and(&mut pred) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }
}

impl TradeSeq for TradeTape {
    fn len(&self) -> usize {
        Self::len(self)
    }

    fn get(&self, index: usize) -> Option<&Trade> {
        Self::get(self, index)
    }

    fn partition_point<P: FnMut(&Trade) -> bool>(&self, pred: P) -> usize {
        Self::partition_point(self, pred)
    }
}

impl TradeSeq for [Trade] {
    fn len(&self) -> usize {
        <[Trade]>::len(self)
    }

    fn get(&self, index: usize) -> Option<&Trade> {
        <[Trade]>::get(self, index)
    }

    fn partition_point<P: FnMut(&Trade) -> bool>(&self, pred: P) -> usize {
        <[Trade]>::partition_point(self, pred)
    }
}

impl TradeSeq for Vec<Trade> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn get(&self, index: usize) -> Option<&Trade> {
        self.as_slice().get(index)
    }

    fn partition_point<P: FnMut(&Trade) -> bool>(&self, pred: P) -> usize {
        self.as_slice().partition_point(pred)
    }
}
