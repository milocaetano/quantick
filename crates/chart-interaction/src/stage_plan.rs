//! One shape and one validator for the fixed synchronous stage plans.
//!
//! Each plan is a `const` array of [`Step`]s checked at compile time. The
//! full-coverage mask is derived from the enum itself: [`stage_enum!`] counts
//! the variants it declares, so a stage added to the enum but not placed in
//! its plan fails that plan's `const` assertion instead of being skipped.

/// A stage and the mask of stages that must already have run.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Step<S> {
    pub(crate) stage: S,
    pub(crate) after: u8,
}

/// Declare a stage enum, its one-bit identities and its plan validator.
///
/// The validator (`valid`, a `const fn` in the calling module) requires every
/// stage exactly once, each after every stage its `after` mask names, and no
/// stage of the enum left out. Requiring prerequisites to be visited already
/// also rejects forward edges, self edges, cycles and unknown bits.
macro_rules! stage_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name {
            $($variant),+
        }

        impl $name {
            /// Declared variants, counted by the macro rather than by hand.
            const COUNT: u32 = [$(stringify!($variant)),+].len() as u32;

            pub(crate) const fn bit(self) -> u8 {
                1 << self as u8
            }
        }

        const _: () = assert!($name::COUNT <= u8::BITS, "a stage mask is one byte");

        const fn valid(steps: &[$crate::stage_plan::Step<$name>]) -> bool {
            let mut seen: u8 = 0;
            let mut index = 0;
            while index < steps.len() {
                let step = steps[index];
                if seen & step.stage.bit() != 0 || step.after & seen != step.after {
                    return false;
                }
                seen |= step.stage.bit();
                index += 1;
            }
            seen as u16 == (1u16 << $name::COUNT) - 1
        }
    };
}
pub(crate) use stage_enum;

#[cfg(test)]
mod tests {
    stage_enum! {
        enum Probe { First, Second, Third }
    }
    use super::Step;

    const PLAN: [Step<Probe>; 3] = [
        Step {
            stage: Probe::First,
            after: 0,
        },
        Step {
            stage: Probe::Second,
            after: Probe::First.bit(),
        },
        Step {
            stage: Probe::Third,
            after: Probe::Second.bit(),
        },
    ];

    #[test]
    fn coverage_follows_the_declared_variants_not_a_hand_mask() {
        assert!(valid(&PLAN));
        // Dropping the last stage is refused because the macro counted three.
        assert!(!valid(&PLAN[..2]));
        assert!(!valid(&[]));
    }
}
