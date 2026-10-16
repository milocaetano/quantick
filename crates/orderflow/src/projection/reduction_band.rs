//! Screen-independent geometry of a factual liquidity-reduction marker.

#[derive(Debug, Clone, Copy)]
pub struct EventBand {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

impl EventBand {
    pub fn height(self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    pub fn center_y(self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

pub fn marker_band(band: EventBand, reduction: f32, full: bool) -> EventBand {
    if full {
        return band;
    }
    let fraction = if reduction.is_finite() {
        reduction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let height = (band.height() * (0.30 + 0.70 * fraction)).max(1.5);
    let center = band.center_y();
    EventBand {
        x: band.x,
        top: center - height / 2.0,
        bottom: center + height / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_marker_height_grows_with_reduction_fraction() {
        let band = EventBand {
            x: 50.0,
            top: 10.0,
            bottom: 30.0,
        };
        let quiet = marker_band(band, 0.1, false);
        let strong = marker_band(band, 0.8, false);
        assert!(quiet.height() < strong.height());
        assert!(strong.height() < band.height());
        assert_eq!(marker_band(band, 0.2, true).height(), band.height());
    }
}
