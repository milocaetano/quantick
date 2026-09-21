//! Stateful anchored studies over borrowed market evidence.
//! Existing engine and indicator kernels own the mathematical computations.
pub mod average;
pub mod profile;
pub use average::{
    AnchoredAverage, AverageInputs, AverageOutput, AverageRequest, AvwapBand, AvwapCacheKey,
};
pub use profile::{
    FrvpCacheKey, FrvpEmpty, LevelPrices, ProfileInputs, ProfileOutput, ProfileRequest,
    RangeProfile,
};
