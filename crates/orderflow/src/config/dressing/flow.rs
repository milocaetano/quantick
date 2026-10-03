//! FLOW emphasis colours in premultiplied gamma-space bytes, without UI types.
use super::HOLLOW_FILL_ALPHA;

pub const BLUE: [u8; 3] = [112, 185, 244];
pub const AMBER: [u8; 3] = [232, 175, 99];
pub const PEAK_OPACITY: f32 = 0.65;
pub const CONTEXT_OPACITY: f32 = 0.12;

/// Compose isolation into the earned sector area. An ordinary context requires
/// an opaque canvas background so overlapping context does not accumulate ink.
/// Background bytes are premultiplied; the oversized opening ignores them.
pub fn colors(background: Option<[u8; 4]>, opening: bool, context: bool) -> [[u8; 4]; 2] {
    assert!(
        opening || !context || background.is_some_and(|rgba| rgba[3] == 255),
        "FLOW context requires an opaque canvas background"
    );
    let opacity = if opening {
        PEAK_OPACITY * HOLLOW_FILL_ALPHA
    } else if context {
        CONTEXT_OPACITY
    } else {
        PEAK_OPACITY
    };
    [BLUE, AMBER].map(|[r, g, b]| {
        let source = [r, g, b, 255].map(|v| (f32::from(v) * opacity + 0.5) as u8);
        let Some(background) = background.filter(|_| !opening) else {
            return source;
        };
        let remaining = 1.0 - f32::from(source[3]) / 255.0;
        std::array::from_fn(|channel| {
            (f32::from(source[channel]) + f32::from(background[channel]) * remaining).round() as u8
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighted_palette_retains_the_existing_gamma_composition() {
        assert_eq!(
            colors(Some([19, 23, 34, 255]), false, false),
            [[80, 128, 171, 255], [158, 122, 76, 255]]
        );
        assert_eq!(
            colors(None, false, false),
            [[73, 120, 159, 166], [151, 114, 64, 166]]
        );
    }

    #[test]
    fn context_is_opaque_and_repeated_paint_does_not_accumulate_opacity() {
        let context = colors(Some([19, 23, 34, 255]), false, true);
        assert_eq!(context, [[30, 42, 59, 255], [45, 41, 42, 255]]);
        for source in context {
            let over = |destination: [u8; 4]| {
                std::array::from_fn::<_, 4, _>(|channel| {
                    (f32::from(source[channel])
                        + f32::from(destination[channel]) * (1.0 - f32::from(source[3]) / 255.0))
                        .round() as u8
                })
            };
            assert_eq!(over(over([19, 23, 34, 255])), source);
        }
    }

    #[test]
    fn translucent_backings_preserve_premultiplied_compositing() {
        let over = |source: [f64; 4], destination: [f64; 4]| {
            std::array::from_fn::<_, 4, _>(|channel| {
                source[channel] + destination[channel] * (1.0 - source[3])
            })
        };
        let normalized = |color: [u8; 4]| color.map(|v| f64::from(v) / 255.0);
        for backing in [[19, 23, 34, 255], [12, 18, 29, 128], [0, 0, 0, 0]] {
            for underlying in [[13, 17, 23, 255], [0, 180, 140, 255]] {
                let backdrop = over(normalized(backing), normalized(underlying));
                for (source, composed) in
                    colors(None, false, false)
                        .into_iter()
                        .zip(colors(Some(backing), false, false))
                {
                    let old = over(normalized(source), backdrop);
                    let new = over(normalized(composed), normalized(underlying));
                    for channel in 0..4 {
                        assert!((old[channel] - new[channel]).abs() * 255.0 <= 1.0);
                    }
                }
            }
        }
    }

    #[test]
    fn opening_stays_translucent_regardless_of_backing_or_context() {
        let expected = colors(None, true, false);
        assert!(expected.iter().all(|color| color[3] < 255));
        assert_eq!(colors(Some([19, 23, 34, 255]), true, true), expected);
        assert_eq!(colors(None, true, true), expected);
    }

    #[test]
    #[should_panic(expected = "FLOW context requires an opaque canvas background")]
    fn context_rejects_a_missing_opaque_canvas() {
        colors(None, false, true);
    }
}
