//! Copy-on-write snapshots of the single authoritative contiguous image.
use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Words {
    Owned(Vec<u64>),
    Shared(Rc<Vec<u64>>),
}
impl Words {
    /// Scalar programs retain their direct vector; collection creation enables
    /// copy-on-write before any collection can become part of a snapshot.
    pub fn share(&mut self) {
        if let Self::Owned(words) = self {
            *self = Self::Shared(Rc::new(std::mem::take(words)));
        }
    }
}
impl From<Vec<u64>> for Words {
    fn from(value: Vec<u64>) -> Self {
        Self::Owned(value)
    }
}
impl Deref for Words {
    type Target = Vec<u64>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(words) => words,
            Self::Shared(words) => words,
        }
    }
}
impl DerefMut for Words {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Owned(words) => words,
            Self::Shared(words) => Rc::make_mut(words),
        }
    }
}
