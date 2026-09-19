//! Registered pipeline stages with declared dependencies.
//!
//! A pipeline is an enum declared through [`declare_stages!`]: each stage is
//! a registered item with a name and the stages it runs after, and the
//! declaration order is the canonical traversal. A constant assertion proves
//! at compile time that the traversal visits every stage exactly once and
//! never before a stage it depends on, so a reorder that breaks a declared
//! dependency does not build. Executors keep taking the order as an iterator
//! of stages, which lets tests replay a swapped order through the real effect
//! bodies and show the harm the declaration prevents.
//!
//! Nothing here allocates, sorts or dispatches dynamically: the traversal is
//! a constant array, and an executor is a `match` over a `Copy` enum.

#[cfg(test)]
mod tests;

/// The widest pipeline a `u64` dependency mask can describe.
pub const MAX_STAGES: usize = 64;

/// One registered stage: its identity, its name, and what it runs after.
///
/// `bit` is the stage's own mask bit and `after` the union of the bits of the
/// stages it depends on, so a whole pipeline validates with integer tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageNode<S> {
    pub stage: S,
    pub name: &'static str,
    pub bit: u64,
    pub after: u64,
}

/// Whether `nodes` is a valid traversal of a pipeline with `count` stages.
///
/// Every stage must appear exactly once and after every stage it declares.
/// Requiring each prerequisite to have been visited already rejects forward
/// edges, self edges, cycles and unknown prerequisite bits as well as a
/// missing or duplicated stage.
pub const fn nodes_in_valid_order<S>(nodes: &[StageNode<S>], count: usize) -> bool {
    if count == 0 || count > MAX_STAGES {
        return false;
    }
    let full = u64::MAX >> (MAX_STAGES - count);
    let mut seen = 0_u64;
    let mut index = 0;
    while index < nodes.len() {
        let bit = nodes[index].bit;
        let after = nodes[index].after;
        if bit.count_ones() != 1 || bit & full == 0 || seen & bit != 0 || after & seen != after {
            return false;
        }
        seen |= bit;
        index += 1;
    }
    seen == full
}

/// Positions `(earlier, later)` in `nodes` where the later stage declares the
/// earlier one. Moving `later` in front of `earlier` must be refused; tests
/// walk every pair to pin each declared dependency.
#[cfg(test)]
pub(crate) fn dependency_pairs<S>(
    nodes: &[StageNode<S>],
) -> impl Iterator<Item = (usize, usize)> + '_ {
    nodes.iter().enumerate().flat_map(move |(later, node)| {
        nodes[..later]
            .iter()
            .enumerate()
            .filter(move |(_, earlier)| node.after & earlier.bit != 0)
            .map(move |(earlier, _)| (earlier, later))
    })
}

/// `order` with the stage at `later` moved directly in front of `earlier`.
pub fn hoisted<S: Copy, const N: usize>(order: [S; N], earlier: usize, later: usize) -> [S; N] {
    let mut moved = order;
    moved[earlier..=later].rotate_right(1);
    moved
}

/// Declare a pipeline: an enum whose variants are its stages, in canonical
/// order, each with the stages it runs `after`.
///
/// ```ignore
/// declare_stages! {
///     pub enum Tail {
///         Settle after [],
///         Report after [Settle],
///     }
/// }
/// ```
///
/// A stage or an edge may be compiled conditionally: `Name when (predicate)`
/// registers the stage only where `#[cfg(predicate)]` holds, and
/// `after [Other when (predicate)]` declares the edge only there. The
/// predicate is evaluated in the crate that expands the declaration, so a
/// build without a harness has no harness stage at all rather than a no-op.
///
/// Generates `COUNT`, `NODES`, `ORDER`, `bit`, `name`, `after`,
/// `is_valid_order` and `canonical`, and a constant assertion that the
/// declaration order satisfies every declared dependency.
#[macro_export]
#[doc(hidden)]
macro_rules! declare_stages {
    (
        $(#[$meta:meta])*
        $vis:vis enum $Stage:ident {
            $( $(#[$variant_meta:meta])* $Variant:ident $(when ($when:meta))?
                after [$($Dep:ident $(when ($dep_when:meta))?),* $(,)?] ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $Stage {
            $( $(#[$variant_meta])* $(#[cfg($when)])? $Variant ),+
        }

        #[allow(dead_code)]
        impl $Stage {
            /// How many stages the pipeline registers.
            pub const COUNT: usize = [$($(#[cfg($when)])? stringify!($Variant)),+].len();
            /// The registered stages with their names and dependencies, in
            /// canonical order.
            pub const NODES: [$crate::stage_registry::StageNode<Self>; Self::COUNT] = [
                $($(#[cfg($when)])? $crate::stage_registry::StageNode {
                    stage: Self::$Variant,
                    name: stringify!($Variant),
                    bit: Self::$Variant.bit(),
                    after: Self::$Variant.after(),
                }),+
            ];
            /// The canonical traversal.
            pub const ORDER: [Self; Self::COUNT] = [$($(#[cfg($when)])? Self::$Variant),+];

            /// This stage's dependency-mask bit.
            pub const fn bit(self) -> u64 {
                1 << self as u32
            }

            /// The registered name.
            pub const fn name(self) -> &'static str {
                match self {
                    $($(#[cfg($when)])? Self::$Variant => stringify!($Variant)),+
                }
            }

            /// The mask of the stages this one runs after.
            pub const fn after(self) -> u64 {
                match self {
                    $($(#[cfg($when)])? Self::$Variant => {
                        #[allow(unused_mut)]
                        let mut mask = 0;
                        $($(#[cfg($dep_when)])? {
                            mask |= Self::$Dep.bit();
                        })*
                        mask
                    }),+
                }
            }

            /// Whether `order` visits every stage once, each after the stages
            /// it declares.
            pub const fn is_valid_order(order: &[Self]) -> bool {
                let full = u64::MAX >> ($crate::stage_registry::MAX_STAGES - Self::COUNT);
                let mut seen = 0_u64;
                let mut index = 0;
                while index < order.len() {
                    let stage = order[index];
                    if seen & stage.bit() != 0 || stage.after() & seen != stage.after() {
                        return false;
                    }
                    seen |= stage.bit();
                    index += 1;
                }
                seen == full
            }

            /// The validated traversal an executor runs: a constant array,
            /// no allocation, sort or payload copy.
            pub fn canonical() -> impl ExactSizeIterator<Item = Self> + Clone {
                Self::ORDER.into_iter()
            }
        }

        const _: () = assert!(
            $Stage::COUNT <= $crate::stage_registry::MAX_STAGES
                && $crate::stage_registry::nodes_in_valid_order(&$Stage::NODES, $Stage::COUNT)
                && $Stage::is_valid_order(&$Stage::ORDER)
        );
    };
}

pub use crate::declare_stages;
