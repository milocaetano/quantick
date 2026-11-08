//! Snapshot ownership is decided at run boundaries, never at word accesses.
use crate::eval::arena::Arena;
use std::{ops::Deref, rc::Rc};
#[derive(Clone)]
pub(crate) enum Image {
    Owned(Arena),
    Shared(Rc<Arena>),
}
impl Default for Image {
    fn default() -> Self {
        Self::Owned(Arena::default())
    }
}
impl From<Arena> for Image {
    fn from(arena: Arena) -> Self {
        Self::Owned(arena)
    }
}
impl Deref for Image {
    type Target = Arena;
    fn deref(&self) -> &Arena {
        match self {
            Self::Owned(arena) => arena,
            Self::Shared(arena) => arena,
        }
    }
}
impl Image {
    pub fn make_mut(&mut self) -> &mut Arena {
        match self {
            Self::Owned(arena) => arena,
            Self::Shared(arena) => Rc::make_mut(arena),
        }
    }
    pub fn snapshot(&mut self) -> Self {
        if let Self::Owned(arena) = self
            && arena.words[30] != 0
        {
            let Self::Owned(arena) = std::mem::take(self) else {
                unreachable!("owned image");
            };
            *self = Self::Shared(Rc::new(arena));
        }
        self.clone()
    }
}
