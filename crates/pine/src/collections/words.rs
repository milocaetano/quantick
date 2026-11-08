//! Copy-on-write snapshots of the single authoritative contiguous image.
use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Words(Rc<Vec<u64>>);
impl From<Vec<u64>> for Words {
    fn from(value: Vec<u64>) -> Self {
        Self(Rc::new(value))
    }
}
impl Deref for Words {
    type Target = Vec<u64>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Words {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        Rc::make_mut(&mut self.0)
    }
}
