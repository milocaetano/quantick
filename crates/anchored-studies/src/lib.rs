//! Stateful anchored studies over borrowed market evidence.
//! Existing engine and indicator kernels own the mathematical computations.
pub mod average;
pub mod profile;
pub use average::{
    AnchoredAverage, AverageInputs, AverageOutput, AverageRequest, AvwapBand, AvwapCacheKey,
    AvwapPartialSig,
};
pub use profile::{
    FrvpCacheKey, FrvpEmpty, ProfileInputs, ProfileOutput, ProfileRequest, RangeProfile,
};
