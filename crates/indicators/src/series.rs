//! [`SeriesStore`] — committed history plus one staged row.
//!
//! Pine's execution model, adapted to quantick's forming bar, is a
//! two-phase append (see the crate docs): a **commit run** persists exactly
//! one new row per series when a bar closes, while a **preview run** against
//! the forming bar stages a row, reads through it, and discards it afterwards.
//!
//! The staging discipline is *truncate, don't clone* (locked decision): a
//! preview appends at most one staged cell per column and rolls back by
//! truncating to the committed length — O(number of columns), no data copied.
//!
//! Columns are typed (`f64` / bool / color) and stored flat; per-cell boxing
//! would wreck both memory and the recompute budget. `na` is `f64::NAN` in
//! numeric columns, [`NA_BOOL`] in bool columns and [`NA_COLOR`] in color
//! columns.

/// Handle to one series column, returned by the `add_*` registrars.
///
/// Ids are dense indices private to the store that issued them; using an id
/// from another store is a logic error the store cannot detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesId(usize);

/// The stored encoding of a bool `na` cell (bool columns are `Vec<u8>`:
/// `0` = false, `1` = true, `NA_BOOL` = na).
const NA_BOOL: u8 = 2;

/// The color value meaning "no color" (`na`): fully transparent black.
/// Transparent is invisible under every blend, so the sentinel can never
/// render as a wrong color.
pub const NA_COLOR: u32 = 0;

#[derive(Debug, Clone)]
enum Column {
    F64(Vec<f64>),
    Bool(Vec<u8>),
    Color(Vec<u32>),
}

impl Column {
    fn len(&self) -> usize {
        match self {
            Column::F64(v) => v.len(),
            Column::Bool(v) => v.len(),
            Column::Color(v) => v.len(),
        }
    }

    fn truncate(&mut self, len: usize) {
        match self {
            Column::F64(v) => v.truncate(len),
            Column::Bool(v) => v.truncate(len),
            Column::Color(v) => v.truncate(len),
        }
    }

    /// Append this column's `na` value.
    fn push_na(&mut self) {
        match self {
            Column::F64(v) => v.push(f64::NAN),
            Column::Bool(v) => v.push(NA_BOOL),
            Column::Color(v) => v.push(NA_COLOR),
        }
    }
}

/// Typed columns of full history with an optional one-row staging area.
///
/// Every column is either at the committed length (no staged cell) or one
/// past it (staged). [`commit_staged`](SeriesStore::commit_staged) normalises
/// unstaged columns with `na` — a series a conditional branch never assigned
/// this bar honestly reads `na`, exactly Pine's rule — and advances the
/// committed length. [`discard_staged`](SeriesStore::discard_staged) is the
/// preview rollback: truncate everything back to the committed length.
#[derive(Debug, Clone, Default)]
pub struct SeriesStore {
    columns: Vec<Column>,
    committed_len: usize,
}

impl SeriesStore {
    /// An empty store with no columns and no rows.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a numeric column. Registered late (after rows were
    /// committed), its history is backfilled with `na` — the honest value for
    /// bars it never saw.
    pub fn add_f64(&mut self) -> SeriesId {
        self.add_column(Column::F64(vec![f64::NAN; self.committed_len]))
    }

    /// Register a bool column (backfilled with `na`, see [`add_f64`](Self::add_f64)).
    pub fn add_bool(&mut self) -> SeriesId {
        self.add_column(Column::Bool(vec![NA_BOOL; self.committed_len]))
    }

    /// Register a color column (backfilled with [`NA_COLOR`], see
    /// [`add_f64`](Self::add_f64)).
    pub fn add_color(&mut self) -> SeriesId {
        self.add_column(Column::Color(vec![NA_COLOR; self.committed_len]))
    }

    fn add_column(&mut self, column: Column) -> SeriesId {
        self.columns.push(column);
        SeriesId(self.columns.len() - 1)
    }

    /// Number of committed rows (bars evaluated by commit runs).
    #[must_use]
    pub fn committed_len(&self) -> usize {
        self.committed_len
    }

    /// Number of registered columns.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Stage `value` as the current row of `id`, replacing any value already
    /// staged this row (a script may reassign a variable several times inside
    /// one bar; the last write wins).
    ///
    /// # Panics
    ///
    /// Panics if `id` is not an f64 column — series are typed at registration
    /// and the type of every access is fixed at compile time upstream.
    pub fn stage_f64(&mut self, id: SeriesId, value: f64) {
        let committed = self.committed_len;
        match &mut self.columns[id.0] {
            Column::F64(v) => Self::stage_cell(v, committed, value),
            other => panic!("series {id:?} is not f64 (got {other:?})"),
        }
    }

    /// Stage a bool value (`None` = `na`). See [`stage_f64`](Self::stage_f64).
    ///
    /// # Panics
    ///
    /// Panics if `id` is not a bool column.
    pub fn stage_bool(&mut self, id: SeriesId, value: Option<bool>) {
        let committed = self.committed_len;
        let encoded = match value {
            Some(true) => 1,
            Some(false) => 0,
            None => NA_BOOL,
        };
        match &mut self.columns[id.0] {
            Column::Bool(v) => Self::stage_cell(v, committed, encoded),
            other => panic!("series {id:?} is not bool (got {other:?})"),
        }
    }

    /// Stage a color value ([`NA_COLOR`] = `na`). See [`stage_f64`](Self::stage_f64).
    ///
    /// # Panics
    ///
    /// Panics if `id` is not a color column.
    pub fn stage_color(&mut self, id: SeriesId, value: u32) {
        let committed = self.committed_len;
        match &mut self.columns[id.0] {
            Column::Color(v) => Self::stage_cell(v, committed, value),
            other => panic!("series {id:?} is not color (got {other:?})"),
        }
    }

    fn stage_cell<T>(column: &mut Vec<T>, committed_len: usize, value: T) {
        if column.len() == committed_len {
            column.push(value);
        } else {
            // Column is already staged (len == committed + 1): last write wins.
            *column.last_mut().expect("staged column is non-empty") = value;
        }
    }

    /// History read: `back = 0` is the newest row (the staged one if this
    /// column has staged a value, else the last committed), `back = 1` the
    /// row before, … Out of range reads `na` (NaN) — Pine's `[]` semantics.
    ///
    /// # Panics
    ///
    /// Panics if `id` is not an f64 column.
    #[must_use]
    pub fn get_f64(&self, id: SeriesId, back: usize) -> f64 {
        match &self.columns[id.0] {
            Column::F64(v) => Self::read_cell(v, back).copied().unwrap_or(f64::NAN),
            other => panic!("series {id:?} is not f64 (got {other:?})"),
        }
    }

    /// History read of a bool column; `None` = `na` (stored or out of range).
    ///
    /// # Panics
    ///
    /// Panics if `id` is not a bool column.
    #[must_use]
    pub fn get_bool(&self, id: SeriesId, back: usize) -> Option<bool> {
        match &self.columns[id.0] {
            Column::Bool(v) => match Self::read_cell(v, back) {
                Some(&0) => Some(false),
                Some(&1) => Some(true),
                _ => None,
            },
            other => panic!("series {id:?} is not bool (got {other:?})"),
        }
    }

    /// History read of a color column; [`NA_COLOR`] = `na` (stored or out of
    /// range).
    ///
    /// # Panics
    ///
    /// Panics if `id` is not a color column.
    #[must_use]
    pub fn get_color(&self, id: SeriesId, back: usize) -> u32 {
        match &self.columns[id.0] {
            Column::Color(v) => Self::read_cell(v, back).copied().unwrap_or(NA_COLOR),
            other => panic!("series {id:?} is not color (got {other:?})"),
        }
    }

    fn read_cell<T>(column: &[T], back: usize) -> Option<&T> {
        column.len().checked_sub(back + 1).map(|i| &column[i])
    }

    /// Persist the staged row: columns that staged nothing this bar get `na`
    /// (a value no branch assigned is honestly missing, never carried
    /// forward by the store — carry-forward is `var` semantics and lives in
    /// the evaluator), then the committed length advances by one.
    pub fn commit_staged(&mut self) {
        for column in &mut self.columns {
            if column.len() == self.committed_len {
                column.push_na();
            }
        }
        self.committed_len += 1;
    }

    /// Preview rollback: drop every staged cell, leaving exactly the
    /// committed rows. O(columns), copies nothing.
    pub fn discard_staged(&mut self) {
        for column in &mut self.columns {
            column.truncate(self.committed_len);
        }
    }

    /// Drop all rows but keep the registered columns — ready for a full
    /// replay (spec switch, replay seek).
    pub fn clear(&mut self) {
        for column in &mut self.columns {
            column.truncate(0);
        }
        self.committed_len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_then_commit_appends_one_row() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.5);
        assert_eq!(store.committed_len(), 0);
        store.commit_staged();
        assert_eq!(store.committed_len(), 1);
        assert_eq!(store.get_f64(a, 0), 1.5);
    }

    #[test]
    fn restaging_overwrites_within_the_same_row() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.0);
        store.stage_f64(a, 2.0);
        store.commit_staged();
        assert_eq!(store.committed_len(), 1);
        assert_eq!(store.get_f64(a, 0), 2.0);
    }

    #[test]
    fn discard_staged_rolls_back_to_committed() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.0);
        store.commit_staged();

        store.stage_f64(a, 99.0);
        assert_eq!(store.get_f64(a, 0), 99.0, "read-through sees the stage");
        store.discard_staged();
        assert_eq!(store.committed_len(), 1);
        assert_eq!(store.get_f64(a, 0), 1.0, "rollback restored the commit");
    }

    #[test]
    fn unstaged_column_commits_na() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        let b = store.add_f64();
        store.stage_f64(a, 1.0);
        store.commit_staged();
        assert_eq!(store.get_f64(a, 0), 1.0);
        assert!(store.get_f64(b, 0).is_nan(), "unassigned this bar => na");
    }

    #[test]
    fn history_reads_count_back_from_newest() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        for v in [10.0, 20.0, 30.0] {
            store.stage_f64(a, v);
            store.commit_staged();
        }
        assert_eq!(store.get_f64(a, 0), 30.0);
        assert_eq!(store.get_f64(a, 1), 20.0);
        assert_eq!(store.get_f64(a, 2), 10.0);
        assert!(store.get_f64(a, 3).is_nan(), "before history => na");
    }

    #[test]
    fn staged_cell_shifts_history_reads_by_one() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.0);
        store.commit_staged();
        store.stage_f64(a, 2.0);
        assert_eq!(store.get_f64(a, 0), 2.0, "[0] is the staged row");
        assert_eq!(store.get_f64(a, 1), 1.0, "[1] is the last commit");
    }

    #[test]
    fn late_registration_backfills_na() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.0);
        store.commit_staged();

        let b = store.add_f64();
        assert!(store.get_f64(b, 0).is_nan(), "backfilled history is na");
        store.stage_f64(b, 5.0);
        store.commit_staged();
        assert_eq!(store.get_f64(b, 0), 5.0);
        assert!(store.get_f64(b, 1).is_nan());
    }

    #[test]
    fn bool_columns_round_trip_true_false_na() {
        let mut store = SeriesStore::new();
        let b = store.add_bool();
        store.stage_bool(b, Some(true));
        store.commit_staged();
        store.stage_bool(b, Some(false));
        store.commit_staged();
        store.stage_bool(b, None);
        store.commit_staged();
        assert_eq!(store.get_bool(b, 2), Some(true));
        assert_eq!(store.get_bool(b, 1), Some(false));
        assert_eq!(store.get_bool(b, 0), None);
        assert_eq!(store.get_bool(b, 3), None, "out of range => na");
    }

    #[test]
    fn color_columns_use_the_transparent_na_sentinel() {
        let mut store = SeriesStore::new();
        let c = store.add_color();
        store.stage_color(c, 0xFF00_00FF);
        store.commit_staged();
        assert_eq!(store.get_color(c, 0), 0xFF00_00FF);
        assert_eq!(store.get_color(c, 1), NA_COLOR, "out of range => na");
    }

    #[test]
    fn clear_keeps_columns_drops_rows() {
        let mut store = SeriesStore::new();
        let a = store.add_f64();
        store.stage_f64(a, 1.0);
        store.commit_staged();
        store.clear();
        assert_eq!(store.committed_len(), 0);
        assert_eq!(store.column_count(), 1);
        assert!(store.get_f64(a, 0).is_nan());
    }

    #[test]
    #[should_panic(expected = "is not f64")]
    fn type_mismatch_is_a_loud_bug_not_a_wrong_value() {
        let mut store = SeriesStore::new();
        let b = store.add_bool();
        store.stage_f64(b, 1.0);
    }
}
