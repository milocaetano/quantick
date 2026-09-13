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
//! whole tape again. The newest chunk, the one appends write to, sits in the
//! tape itself; the others are listed in a directory of one pointer triple
//! each — 60 of them at the live envelope's 3,960,000 prints. The other end
//! of the bound: the first print allocates a whole chunk, 3.5 MiB, so a
//! short tape reserves more than a vector of its length would. Its untouched
//! pages cost commit charge, not working set; growing the first chunk
//! geometrically instead would copy it, which is what this type exists not
//! to do.

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
    /// The full chunks, oldest first: [`CHUNK_TRADES`] prints each.
    full: Vec<Vec<Trade>>,
    /// The newest chunk, the one an append writes to: allocated at
    /// [`CHUNK_TRADES`] capacity with its first print and never grown, and
    /// empty only while the whole tape is. Kept beside the directory rather
    /// than in it, so a print's append reads nothing but this.
    tail: Vec<Trade>,
}

impl TradeTape {
    /// An empty tape; it allocates nothing until the first print.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            full: Vec::new(),
            tail: Vec::new(),
        }
    }

    /// Prints held.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.full.len() * CHUNK_TRADES + self.tail.len()
    }

    /// Whether the tape holds no print.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.tail.is_empty()
    }

    /// Prints the tape can hold before it allocates again: whole chunks, so
    /// never more than [`CHUNK_TRADES`] − 1 beyond [`len`](Self::len).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.full.len() * CHUNK_TRADES + self.tail.capacity()
    }

    /// Print `index`, oldest first, or `None` past the end.
    #[must_use]
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Trade> {
        self.chunk(index >> CHUNK_SHIFT)?.get(index & OFFSET_MASK)
    }

    /// The oldest print held.
    #[must_use]
    #[inline]
    pub fn first(&self) -> Option<&Trade> {
        self.full.first().unwrap_or(&self.tail).first()
    }

    /// The newest print held.
    #[must_use]
    #[inline]
    pub fn last(&self) -> Option<&Trade> {
        self.tail.last()
    }

    /// Append one print. Writes it into the last chunk, or allocates one new
    /// chunk when that is full; never moves a print already held.
    #[inline]
    pub fn push(&mut self, trade: Trade) {
        // The tail is empty only on a new tape and full at a whole chunk:
        // either way its length is a multiple of one.
        if self.tail.len() & OFFSET_MASK == 0 {
            self.open_tail();
        }
        self.tail.push(trade);
    }

    /// Append `trades`, in order: chunk by chunk, allocating each new chunk
    /// once and never moving a print already held.
    pub fn extend_from_slice(&mut self, mut trades: &[Trade]) {
        while !trades.is_empty() {
            if self.tail.len() & OFFSET_MASK == 0 {
                self.open_tail();
            }
            let take = (CHUNK_TRADES - self.tail.len()).min(trades.len());
            self.tail.extend_from_slice(&trades[..take]);
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
        for chunk in held.full.into_iter().chain([held.tail]) {
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
        self.range(self.len().saturating_sub(n)..)
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
            .full
            .partition_point(|chunk| chunk.last().is_some_and(&mut pred));
        let chunk = self.full.get(whole).unwrap_or(&self.tail);
        (whole << CHUNK_SHIFT) + chunk.partition_point(pred)
    }

    /// `range` as a start and an end position, checked as slicing a `Vec`
    /// would check it.
    fn bounds(&self, range: &impl RangeBounds<usize>) -> (usize, usize) {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start
                .checked_add(1)
                .expect("tape range start overflows usize"),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end.checked_add(1).expect("tape range end overflows usize"),
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len(),
        };
        assert!(
            start <= end,
            "tape range starts at {start} but ends at {end}"
        );
        assert!(
            end <= self.len(),
            "tape range end {end} is out of range for a tape of {} prints",
            self.len()
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
        let chunk = |index| self.chunk(index).unwrap_or_default();
        if first_chunk == last_chunk {
            return Slices {
                head: &chunk(first_chunk)[from..to],
                middle: [].iter(),
                tail: &[],
            };
        }
        // Only the last chunk of a range can be the tail, so every chunk
        // strictly between its ends is a full one.
        Slices {
            head: &chunk(first_chunk)[from..],
            middle: self.full[first_chunk + 1..last_chunk].iter(),
            tail: &chunk(last_chunk)[..to],
        }
    }

    /// Chunk `index`: a full one, the tail, or `None` past it.
    #[inline]
    fn chunk(&self, index: usize) -> Option<&[Trade]> {
        match self.full.get(index) {
            Some(chunk) => Some(chunk),
            None if index == self.full.len() => Some(&self.tail),
            None => None,
        }
    }

    /// Retire a full tail into the directory (an empty one, on a new tape,
    /// is simply replaced) and allocate the next: the only allocation an
    /// append ever makes, once per [`CHUNK_TRADES`] prints.
    #[cold]
    fn open_tail(&mut self) {
        let filled = std::mem::replace(&mut self.tail, Vec::with_capacity(CHUNK_TRADES));
        if !filled.is_empty() {
            self.full.push(filled);
        }
    }
}

impl Index<usize> for TradeTape {
    type Output = Trade;

    #[inline]
    fn index(&self, index: usize) -> &Trade {
        self.get(index).unwrap_or_else(|| {
            panic!(
                "tape index {index} is out of range for a tape of {} prints",
                self.len()
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

    #[inline]
    fn next(&mut self) -> Option<&'a [Trade]> {
        if !self.head.is_empty() {
            return Some(std::mem::take(&mut self.head));
        }
        if let Some(chunk) = self.middle.next() {
            return Some(chunk);
        }
        (!self.tail.is_empty()).then(|| std::mem::take(&mut self.tail))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = usize::from(!self.head.is_empty())
            + self.middle.len()
            + usize::from(!self.tail.is_empty());
        (len, Some(len))
    }
}

impl<'a> DoubleEndedIterator for Slices<'a> {
    #[inline]
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

    #[inline]
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

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    #[inline]
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
