//! The screenshot: the frame as the application thread hands it over, and
//! its encoding into a bundle image with the control regions of the scene
//! captured beside it.
//!
//! The one part of a capture with its own vocabulary -- pixels, a scale, a
//! PNG, a rectangle per control -- and no authority in it: whether a picture
//! may be taken at all was decided by the scopes the parent's capability
//! required, and every reason this file cannot deliver one is reported as a
//! coded gap in the bundle's coverage rather than as a refusal.

use quantick_control::{
    limits::CONTROL_EVIDENCE_MAX_BUNDLE_BYTES,
    wire::{Base64Bytes, WireU64},
};

use super::super::{
    scene::{SceneBoundsSnapshot, SceneSnapshot},
    types::{canonical_f32, canonical_f64, wire_usize},
};
use super::{
    EvidenceControlRegion, EvidenceGap, EvidenceImage, EvidenceScreenshot, base64_len,
    screenshot_gap,
};
// The one thing this file borrows from the other side of the seam: the
// digest helper, so an image and a chunk are hashed the same way.
use super::store::raw_sha256;

/// The image format a bundle carries.
const SCREENSHOT_FORMAT: &str = "png";
/// Places a pixel coordinate is reported to.
///
/// Deliberately coarser than a control's own `SCREEN_DECIMAL_PLACES`: a region
/// of an image is only ever compared against whole pixels, and carrying three
/// fractional places per rectangle would inflate every bundle that has a
/// screenshot for precision no reader can use.
const REGION_DECIMAL_PLACES: u32 = 2;

// ---------------------------------------------------------------------------
// The screenshot, before it is encoded
// ---------------------------------------------------------------------------

/// One frame as the application thread hands it over: its geometry now, and
/// its bytes when somebody is ready to pay for them.
///
/// The interface toolkit's own image type stops at the gateway — nothing
/// downstream of here has an opinion about how the window is drawn — but the
/// *copy* out of it does not belong on the application thread either. A 4K
/// framebuffer is eight million pixels, and converting them between two frames
/// is a visible hitch the moment an agent asks for a picture, inside a budget
/// measured in microseconds. So the geometry travels eagerly and the rows
/// travel as a closure the response worker calls, beside the PNG encoding it
/// was always going to pay for.
pub(crate) struct RawScreenshot {
    pub width_px: u32,
    pub height_px: u32,
    pub pixels_per_point: f32,
    /// Eight-bit straight-alpha RGBA, row-major,
    /// `width_px * height_px * 4` bytes — produced on demand, once.
    pub rgba: ScreenshotPixels,
}

/// The rows of one frame, still unpaid for.
pub(crate) struct ScreenshotPixels(Box<dyn FnOnce() -> Vec<u8> + Send>);

impl ScreenshotPixels {
    pub fn new(produce: impl FnOnce() -> Vec<u8> + Send + 'static) -> Self {
        Self(Box::new(produce))
    }

    fn take(self) -> Vec<u8> {
        (self.0)()
    }
}

impl std::fmt::Debug for RawScreenshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RawScreenshot")
            .field("width_px", &self.width_px)
            .field("height_px", &self.height_px)
            .field("pixels_per_point", &self.pixels_per_point)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Screenshot encoding and control correlation
// ---------------------------------------------------------------------------

/// Turn one frame's pixels into a bundle image with its control regions.
///
/// The regions come from the scene captured in the *same* pass, scaled from
/// the logical points the scene reports into the physical pixels the image is
/// measured in. That scaling is the only arithmetic here, and it is why the
/// two have to share a capture revision: a scene from another frame would name
/// controls that have since moved.
pub(super) fn encode_screenshot(
    raw: RawScreenshot,
    capture_revision: WireU64,
    scene: Option<&SceneSnapshot>,
) -> Result<EvidenceImage, EvidenceGap> {
    let (width_px, height_px) = (raw.width_px, raw.height_px);
    let expected = (width_px as usize)
        .saturating_mul(height_px as usize)
        .saturating_mul(4);
    if width_px == 0
        || height_px == 0
        || !raw.pixels_per_point.is_finite()
        || raw.pixels_per_point <= 0.0
    {
        return Err(screenshot_gap("frame_pixels_inconsistent"));
    }
    // The rows are produced here, on the response worker, and checked against
    // the geometry that travelled with them before anything is encoded.
    let rgba = raw.rgba.take();
    if rgba.len() != expected {
        return Err(screenshot_gap("frame_pixels_inconsistent"));
    }
    let png = encode_png(width_px, height_px, &rgba)
        .map_err(|_| screenshot_gap("image_encoding_failed"))?;
    // Against the size the image costs *inside the document*, not the size it
    // is on its own: it travels as base64, and comparing the raw length would
    // admit an image a third larger than the ceiling admits.
    if base64_len(png.len()) > CONTROL_EVIDENCE_MAX_BUNDLE_BYTES {
        return Err(screenshot_gap("exceeds_evidence_bundle_budget"));
    }

    // The factor is rounded *before* it is used, not after, so the number the
    // descriptor publishes is the number the regions were actually built with.
    // A client that redoes the arithmetic the field's own doc describes lands
    // on the same pixel; publishing full precision and reporting two places
    // would put it several pixels out at the right-hand edge.
    let pixels_per_point = canonical_f32(raw.pixels_per_point, REGION_DECIMAL_PLACES)
        .ok_or_else(|| screenshot_gap("frame_scale_not_representable"))?;
    let scale = pixels_per_point
        .as_str()
        .parse::<f64>()
        .map_err(|_| screenshot_gap("frame_scale_not_representable"))?;
    let mut control_regions = Vec::new();
    let mut controls_without_region = Vec::new();
    // A missing scene is reported by the caller, which is the only place that
    // knows *why* it is missing — never captured, or captured and unreadable.
    // Here it simply means there is nothing to map.
    match scene {
        None => {}
        Some(scene) => {
            for control in &scene.controls {
                let Some(bounds) = &control.bounds else {
                    controls_without_region.push(EvidenceGap {
                        subject: control.control_id.clone(),
                        reason: control
                            .bounds_availability
                            .reason
                            .clone()
                            .unwrap_or_else(|| "bounds_unavailable".to_owned()),
                    });
                    continue;
                };
                match region_of(&control.control_id, bounds, scale, width_px, height_px) {
                    Some(region) => control_regions.push(region),
                    None => controls_without_region.push(EvidenceGap {
                        subject: control.control_id.clone(),
                        reason: "bounds_not_representable".to_owned(),
                    }),
                }
            }
        }
    }

    let descriptor = EvidenceScreenshot {
        capture_revision,
        width_px,
        height_px,
        pixels_per_point,
        format: SCREENSHOT_FORMAT.to_owned(),
        image_digest: raw_sha256(&png),
        image_bytes: wire_usize(png.len()),
        control_regions,
        controls_without_region,
    };
    Ok(EvidenceImage {
        image_base64: Base64Bytes::from_bytes(&png),
        descriptor,
    })
}

/// Why this bundle's image carries no control regions.
pub(super) fn region_gap(reason: &str) -> EvidenceGap {
    EvidenceGap {
        subject: "screenshot.control_regions".to_owned(),
        reason: reason.to_owned(),
    }
}

fn region_of(
    control_id: &str,
    bounds: &SceneBoundsSnapshot,
    scale: f64,
    image_width_px: u32,
    image_height_px: u32,
) -> Option<EvidenceControlRegion> {
    let x = bounds.x_pt.as_str().parse::<f64>().ok()? * scale;
    let y = bounds.y_pt.as_str().parse::<f64>().ok()? * scale;
    let width = bounds.width_pt.as_str().parse::<f64>().ok()? * scale;
    let height = bounds.height_pt.as_str().parse::<f64>().ok()? * scale;
    let within_image = x >= 0.0
        && y >= 0.0
        && width >= 0.0
        && height >= 0.0
        && x + width <= f64::from(image_width_px)
        && y + height <= f64::from(image_height_px);
    Some(EvidenceControlRegion {
        control_id: control_id.to_owned(),
        x_px: canonical_f64(x, REGION_DECIMAL_PLACES)?,
        y_px: canonical_f64(y, REGION_DECIMAL_PLACES)?,
        width_px: canonical_f64(width, REGION_DECIMAL_PLACES)?,
        height_px: canonical_f64(height, REGION_DECIMAL_PLACES)?,
        within_image,
    })
}

fn encode_png(width_px: u32, height_px: u32, rgba: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut buffer = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buffer, width_px, height_px);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(buffer)
}
