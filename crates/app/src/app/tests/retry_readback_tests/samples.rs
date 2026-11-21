//! Ordered gateway scenarios, selected through their readback family's manifest binding.
use super::{LAYOUT_V2, Readback};
use serde_json::Value;
type Sample = (&'static str, u32, Value, Readback);

pub(super) fn replay_plan() -> Vec<Sample> {
    let mut plan = Vec::new();
    macro_rules! sample_families {
        ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $($(plan.extend($sample::samples());)*)* };
    }
    quantick_control::control_families!(sample_families);
    // Scenario dependencies are explicit ordinals; manifest rows remain in ID order.
    plan.sort_by_key(|(order, _)| *order);
    assert!(plan.windows(2).all(|pair| pair[0].0 < pair[1].0));
    plan.into_iter().map(|(_, sample)| sample).collect()
}

macro_rules! sample_modules {
    ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $($(mod $sample;)*)* };
}
quantick_control::control_families!(sample_modules);
