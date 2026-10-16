//! Whether two configs are the same setting, field for field, without
//! formatting either: floats and decimals compare by representation.
use super::{
    BubbleDisplayScale, BubbleRenderMode, BubbleSizeReference, BubbleStyle, ConsumptionMark,
    DisplayGrouping, HeatmapConfig, HeatmapTheme, IntensityMode, LaneWindow, LiveLaneStyle,
    TickBubbleStyle, TickReferenceMode, ViewportBubbleScale, VolumeDotStyle,
};
use rust_decimal::Decimal;

impl HeatmapConfig {
    /// Whether `other` is this very setting. Floats compare by their bits and
    /// decimals by their representation, so unlike `==` a NaN is identical to
    /// itself, while -0.0 differs from 0.0 and 0.10 from 0.1: the distinctions
    /// the `Debug` rendering prints.
    #[must_use]
    pub fn identical(&self, other: &Self) -> bool {
        Exact::exact(self, other)
    }
}

trait Exact {
    fn exact(&self, other: &Self) -> bool;
}

/// Types with no float or decimal inside, where `==` is already exact.
macro_rules! exact_by_eq {
    ($($ty:ty),+ $(,)?) => {$(
        impl Exact for $ty {
            fn exact(&self, other: &Self) -> bool {
                self == other
            }
        }
    )+};
}

exact_by_eq!(
    bool,
    i64,
    u32,
    usize,
    [u8; 3],
    DisplayGrouping,
    HeatmapTheme,
    BubbleRenderMode,
    BubbleSizeReference,
    ConsumptionMark,
    TickReferenceMode,
);

/// Every field by name: the destructuring has no `..`, so a field added to
/// one of these structs does not compile until it is compared here too.
macro_rules! exact_fields {
    ($($ty:ident { $($field:ident),+ $(,)? })+) => {$(
        impl Exact for $ty {
            fn exact(&self, other: &Self) -> bool {
                let $ty { $($field),+ } = self;
                $($field.exact(&other.$field))&&+
            }
        }
    )+};
}

exact_fields! {
    HeatmapConfig {
        enabled, retention_ms, price_grouping, display_grouping, opacity, gamma,
        show_aggressions, projection_demand, bubble_cluster_ms, bubble_dust_merge_ms,
        bubble_region_rows, bubble_region_ms, bubble_candle_summary, volume_dots, bubbles,
        live_lane, show_depth, show_liquidity, show_buy_aggressions, show_sell_aggressions,
        show_aligned_depletion, show_unattributed_reductions, show_gaps,
        min_unattributed_reduction, min_unattributed_pull_share, liquidity_correlation_ms,
        show_legend, show_status_badge, theme, max_history_runs, max_history_bytes,
        max_aggressions, max_visible_cells, max_aggression_primitives, intensity_mode,
    }
    VolumeDotStyle {
        tick_bubbles, enabled, full_quantity, auto_full, ignore_opening_burst_in_scale,
    }
    TickBubbleStyle {
        reference_quantity, reference_mode, viewport_scale, max_radius, label_min_radius, opacity,
    }
    ViewportBubbleScale { x_units_per_pixel, price_units_per_pixel }
    BubbleStyle {
        min_radius, max_radius, opacity, outline_width, halo_strength, detail_min_radius,
        readable_min_radius, hollow_small_buys, render_mode, sphere_shading,
        sphere_highlight, size_reference, size_reference_quantity, min_quantity,
        side_offset, consumption_mark, front_width, front_length_scale, show_impact_ring,
        impact_ring_width, trail_length, trail_opacity, show_quantity_labels,
        show_trade_count, label_min_radius, buy_color, sell_color, front_color,
        trail_color, label_color, viewport_scale, display_scale,
    }
    BubbleDisplayScale { tape, candle }
    LiveLaneStyle {
        width_share, window, cluster_ms, radius_scale, show_marks, enabled, show_depth,
        show_aggressions, native_tape, depth_time_averaging, forward_book_preview,
        stable_size, tape_only,
    }
}

impl Exact for f32 {
    fn exact(&self, other: &Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Exact for f64 {
    fn exact(&self, other: &Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Exact for Decimal {
    fn exact(&self, other: &Self) -> bool {
        self.serialize() == other.serialize()
    }
}

impl<T: Exact> Exact for Option<T> {
    fn exact(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(value), Some(other)) => value.exact(other),
            (None, None) => true,
            (Some(_) | None, _) => false,
        }
    }
}

impl Exact for LaneWindow {
    fn exact(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Auto { zoom }, Self::Auto { zoom: other }) => zoom.exact(other),
            (Self::Fixed { ms }, Self::Fixed { ms: other }) => ms == other,
            (Self::Auto { .. } | Self::Fixed { .. }, _) => false,
        }
    }
}

impl Exact for IntensityMode {
    fn exact(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(maximum), Self::Fixed(other)) => maximum.exact(other),
            (Self::VisibleP99, Self::VisibleP99) => true,
            (Self::VisibleP99 | Self::Fixed(_), _) => false,
        }
    }
}
