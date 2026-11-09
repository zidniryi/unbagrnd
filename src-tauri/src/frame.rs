//! Output framing for the background editor and batch runs: crop to the
//! subject, add padding around it, and fit the result into a fixed output
//! size (marketplace squares, ID photos, social posts, stickers). Pure
//! pixel work on an already background-removed RGBA image, on-device like
//! the rest of the app.

use image::imageops::{self, FilterType};
use image::{ImageBuffer, Rgba, RgbaImage};
use serde::Deserialize;

/// Pixels at or below this alpha don't count as "subject" when finding its
/// bounding box, so faint model noise far from the subject doesn't stop a
/// crop from tightening.
const SUBJECT_ALPHA_THRESHOLD: u8 = 8;

/// Upper bound on a requested output dimension. Generous for print-size
/// output, small enough that a bogus value can't allocate gigabytes.
pub const MAX_OUTPUT_DIM: u32 = 8192;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameSpec {
    /// Crop to the bounding box of the subject's visible pixels before
    /// anything else.
    pub crop_to_subject: bool,
    /// Transparent margin added around the (cropped) image, as a percentage
    /// of its longer side. 0-100.
    pub padding_pct: f32,
    /// Fixed output size. Both or neither: the framed image is scaled to fit
    /// inside it (never distorted) and the rest of the canvas is left
    /// transparent for the background fill.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Where the image sits inside a fixed output size: "center" (default)
    /// or "bottom" - for ID photos, where the shoulders should run off the
    /// bottom edge. Bottom alignment also drops the bottom padding.
    pub align: Option<String>,
}

impl FrameSpec {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=100.0).contains(&self.padding_pct) || self.padding_pct.is_nan() {
            return Err(format!("padding must be between 0 and 100% (got {})", self.padding_pct));
        }
        match (self.width, self.height) {
            (None, None) => {}
            (Some(w), Some(h)) => {
                if w == 0 || h == 0 || w > MAX_OUTPUT_DIM || h > MAX_OUTPUT_DIM {
                    return Err(format!(
                        "output size must be between 1 and {MAX_OUTPUT_DIM} pixels per side (got {w}x{h})"
                    ));
                }
            }
            _ => return Err("output width and height must be given together".to_string()),
        }
        match self.align.as_deref() {
            None | Some("center") | Some("bottom") => Ok(()),
            Some(other) => Err(format!("unknown alignment \"{other}\" (expected center or bottom)")),
        }
    }

    /// True if applying this spec would return the image unchanged.
    pub fn is_identity(&self) -> bool {
        !self.crop_to_subject && self.padding_pct <= 0.0 && self.width.is_none()
    }

    /// The same framing for a live preview: a fixed output size is scaled
    /// down (keeping its aspect ratio) so its longer side is at most
    /// `max_dim`. Crop and padding are already relative, so they carry over
    /// unchanged to a downscaled source.
    pub fn for_preview(&self, max_dim: u32) -> FrameSpec {
        let mut spec = self.clone();
        if let (Some(w), Some(h)) = (self.width, self.height) {
            let long = w.max(h);
            if long > max_dim {
                let scale = max_dim as f32 / long as f32;
                spec.width = Some(((w as f32 * scale).round() as u32).max(1));
                spec.height = Some(((h as f32 * scale).round() as u32).max(1));
            }
        }
        spec
    }

    fn bottom_aligned(&self) -> bool {
        self.align.as_deref() == Some("bottom")
    }
}

/// The bounding box `(x0, y0, x1, y1)` (end-exclusive) of the pixels whose
/// alpha is above [`SUBJECT_ALPHA_THRESHOLD`], or `None` for an image with
/// no visible subject at all.
pub fn subject_bounds(image: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0[3] > SUBJECT_ALPHA_THRESHOLD {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

/// Applies `spec` to `image`: optional crop to the subject, then padding,
/// then (if an output size is set) a fit into that canvas.
pub fn apply_frame(image: &RgbaImage, spec: &FrameSpec) -> RgbaImage {
    if spec.is_identity() {
        return image.clone();
    }
    let (w, h) = image.dimensions();

    // Region of the source to keep, in source coordinates. It may extend
    // past the image's edges once padding is added; that part is transparent.
    let (x0, y0, x1, y1) = if spec.crop_to_subject {
        subject_bounds(image).unwrap_or((0, 0, w, h))
    } else {
        (0, 0, w, h)
    };
    let (mut x0, mut y0, mut x1, mut y1) = (x0 as i64, y0 as i64, x1 as i64, y1 as i64);

    let long_side = (x1 - x0).max(y1 - y0) as f32;
    let pad = (spec.padding_pct / 100.0 * long_side).round() as i64;
    x0 -= pad;
    y0 -= pad;
    x1 += pad;
    if !(spec.bottom_aligned() && spec.width.is_some()) {
        y1 += pad;
    }

    let region = extract_region(image, x0, y0, x1, y1);

    let (Some(out_w), Some(out_h)) = (spec.width, spec.height) else {
        return region;
    };

    let (rw, rh) = region.dimensions();
    let scale = (out_w as f32 / rw as f32).min(out_h as f32 / rh as f32);
    let fit_w = ((rw as f32 * scale).round() as u32).clamp(1, out_w);
    let fit_h = ((rh as f32 * scale).round() as u32).clamp(1, out_h);
    let fitted = if (fit_w, fit_h) == (rw, rh) {
        region
    } else {
        resize_premultiplied(&region, fit_w, fit_h)
    };

    let mut canvas: RgbaImage = ImageBuffer::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));
    let left = (out_w - fit_w) / 2;
    let top = if spec.bottom_aligned() { out_h - fit_h } else { (out_h - fit_h) / 2 };
    imageops::replace(&mut canvas, &fitted, left as i64, top as i64);
    canvas
}

/// Copies the source rectangle `[x0, x1) x [y0, y1)` into a new image,
/// filling anything outside the source with transparency.
fn extract_region(image: &RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64) -> RgbaImage {
    let out_w = (x1 - x0).max(1) as u32;
    let out_h = (y1 - y0).max(1) as u32;
    let mut out: RgbaImage = ImageBuffer::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));
    imageops::replace(&mut out, image, -x0, -y0);
    out
}

/// Resizes in premultiplied alpha. Resampling straight-alpha RGBA bleeds
/// the (usually black) color of fully transparent pixels into the
/// subject's edge, leaving a dark fringe once it's put on a background.
fn resize_premultiplied(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut premultiplied = image.clone();
    for pixel in premultiplied.pixels_mut() {
        let a = pixel.0[3] as u32;
        for c in 0..3 {
            pixel.0[c] = ((pixel.0[c] as u32 * a + 127) / 255) as u8;
        }
    }
    let mut out = imageops::resize(&premultiplied, width, height, FilterType::CatmullRom);
    for pixel in out.pixels_mut() {
        let a = pixel.0[3] as u32;
        if a == 0 {
            pixel.0 = [0, 0, 0, 0];
            continue;
        }
        for c in 0..3 {
            pixel.0[c] = ((pixel.0[c] as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100x80 canvas with an opaque 20x10 subject at (30, 40).
    fn cutout() -> RgbaImage {
        ImageBuffer::from_fn(100, 80, |x, y| {
            if (30..50).contains(&x) && (40..50).contains(&y) {
                Rgba([200, 30, 30, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    #[test]
    fn finds_the_subject_bounds() {
        assert_eq!(subject_bounds(&cutout()), Some((30, 40, 50, 50)));
        let empty: RgbaImage = ImageBuffer::new(4, 4);
        assert_eq!(subject_bounds(&empty), None);
    }

    #[test]
    fn identity_spec_returns_the_image_unchanged() {
        let image = cutout();
        assert_eq!(apply_frame(&image, &FrameSpec::default()), image);
    }

    #[test]
    fn crops_tightly_to_the_subject() {
        let spec = FrameSpec { crop_to_subject: true, ..Default::default() };
        let out = apply_frame(&cutout(), &spec);
        assert_eq!(out.dimensions(), (20, 10));
        assert!(out.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn padding_is_relative_to_the_longer_side() {
        let spec = FrameSpec { crop_to_subject: true, padding_pct: 10.0, ..Default::default() };
        let out = apply_frame(&cutout(), &spec);
        // 10% of the subject's 20px longer side = 2px on each edge.
        assert_eq!(out.dimensions(), (24, 14));
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(2, 2).0[3], 255);
    }

    #[test]
    fn fits_into_a_fixed_size_centered() {
        let spec = FrameSpec {
            crop_to_subject: true,
            width: Some(40),
            height: Some(40),
            ..Default::default()
        };
        let out = apply_frame(&cutout(), &spec);
        assert_eq!(out.dimensions(), (40, 40));
        // 20x10 scaled 2x is 40x20, centered vertically: rows 10..30.
        assert_eq!(out.get_pixel(20, 5).0[3], 0);
        assert_eq!(out.get_pixel(20, 20).0[3], 255);
        assert_eq!(out.get_pixel(20, 35).0[3], 0);
    }

    #[test]
    fn bottom_alignment_puts_the_subject_on_the_bottom_edge() {
        let spec = FrameSpec {
            crop_to_subject: true,
            padding_pct: 10.0,
            width: Some(40),
            height: Some(80),
            align: Some("bottom".to_string()),
        };
        let out = apply_frame(&cutout(), &spec);
        assert_eq!(out.dimensions(), (40, 80));
        assert_eq!(out.get_pixel(20, 79).0[3], 255, "subject should touch the bottom edge");
        assert_eq!(out.get_pixel(20, 0).0[3], 0);
    }

    #[test]
    fn preview_spec_caps_the_output_size_and_keeps_the_aspect_ratio() {
        let spec = FrameSpec { width: Some(1080), height: Some(1920), ..Default::default() };
        let preview = spec.for_preview(1024);
        assert_eq!((preview.width, preview.height), (Some(576), Some(1024)));
    }

    #[test]
    fn rejects_bad_specs() {
        let half = FrameSpec { width: Some(10), ..Default::default() };
        assert!(half.validate().is_err());
        let huge = FrameSpec { width: Some(100_000), height: Some(10), ..Default::default() };
        assert!(huge.validate().is_err());
        let padding = FrameSpec { padding_pct: 500.0, ..Default::default() };
        assert!(padding.validate().is_err());
        let align = FrameSpec { align: Some("left".to_string()), ..Default::default() };
        assert!(align.validate().is_err());
    }
}
