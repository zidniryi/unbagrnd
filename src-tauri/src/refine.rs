//! On-device manual mask refinement (erase / restore with a brush) for the
//! single-image "refine" panel. Strokes are recorded by the frontend in
//! normalized `[0, 1]` image-space coordinates so they're resolution
//! independent, then rasterized here into a soft-edged brush mask and
//! applied to the working image. No AI or network involved.

use image::{Rgba, RgbaImage};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokePoint {
    /// Fraction of the image's width, 0..1.
    pub x: f32,
    /// Fraction of the image's height, 0..1.
    pub y: f32,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub points: Vec<StrokePoint>,
    /// Brush radius, as a fraction of the image's longer side, 0..1.
    pub radius: f32,
}

/// Rasterizes `strokes` into a soft-edged coverage mask (0.0..1.0 per
/// pixel) at `w`x`h`: 1.0 fully inside a stroke, feathered down to 0.0 over
/// roughly a third of the brush radius so the tool has a soft edge rather
/// than a hard-edged one. Consecutive points within a stroke are treated as
/// capsule segments so a fast drag paints a continuous line, not dots.
fn rasterize_strokes(strokes: &[Stroke], w: u32, h: u32) -> Vec<f32> {
    let mut mask = vec![0f32; (w as usize) * (h as usize)];
    let long_side = w.max(h) as f32;

    for stroke in strokes {
        let radius_px = (stroke.radius * long_side).max(1.0);
        let feather = (radius_px * 0.35).max(1.0);

        let segments: Vec<(f32, f32, f32, f32)> = if stroke.points.len() < 2 {
            stroke
                .points
                .first()
                .map(|p| {
                    let (px, py) = (p.x * w as f32, p.y * h as f32);
                    vec![(px, py, px, py)]
                })
                .unwrap_or_default()
        } else {
            stroke
                .points
                .windows(2)
                .map(|pair| {
                    let (a, b) = (&pair[0], &pair[1]);
                    (a.x * w as f32, a.y * h as f32, b.x * w as f32, b.y * h as f32)
                })
                .collect()
        };

        for (x0, y0, x1, y1) in segments {
            let pad = radius_px + feather;
            let min_x = (x0.min(x1) - pad).floor().max(0.0) as u32;
            let max_x = (x0.max(x1) + pad).ceil().min(w as f32 - 1.0) as u32;
            let min_y = (y0.min(y1) - pad).floor().max(0.0) as u32;
            let max_y = (y0.max(y1) + pad).ceil().min(h as f32 - 1.0) as u32;
            if w == 0 || h == 0 || min_x > max_x || min_y > max_y {
                continue;
            }

            let (dx, dy) = (x1 - x0, y1 - y0);
            let len_sq = dx * dx + dy * dy;

            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let t = if len_sq > 0.0 {
                        (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let (cx, cy) = (x0 + t * dx, y0 + t * dy);
                    let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();

                    let coverage = if dist <= radius_px {
                        1.0
                    } else if dist <= radius_px + feather {
                        1.0 - (dist - radius_px) / feather
                    } else {
                        0.0
                    };

                    if coverage > 0.0 {
                        let idx = (y * w + x) as usize;
                        if coverage > mask[idx] {
                            mask[idx] = coverage;
                        }
                    }
                }
            }
        }
    }

    mask
}

fn lerp_pixel(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let mix =
        |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8 };
    Rgba([
        mix(a.0[0], b.0[0]),
        mix(a.0[1], b.0[1]),
        mix(a.0[2], b.0[2]),
        mix(a.0[3], b.0[3]),
    ])
}

/// Applies `strokes` to `current`, either erasing (fading alpha toward 0)
/// or restoring (blending back toward `source`'s color and alpha) within
/// the painted area. `mode` is `"erase"` or `"restore"`; `source` is
/// whichever restore target the user picked — the raw input photo or the
/// model's own cutout as it stood before this refine session — and is
/// unused in erase mode. `current` and `source` must have equal dimensions.
pub fn apply_strokes(current: &RgbaImage, source: &RgbaImage, strokes: &[Stroke], mode: &str) -> RgbaImage {
    let (w, h) = current.dimensions();
    let mask = rasterize_strokes(strokes, w, h);
    let mut out = current.clone();

    for (i, pixel) in out.pixels_mut().enumerate() {
        let coverage = mask[i];
        if coverage <= 0.0 {
            continue;
        }
        if mode == "restore" {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            *pixel = lerp_pixel(*pixel, *source.get_pixel(x, y), coverage);
        } else {
            let faded = pixel.0[3] as f32 * (1.0 - coverage);
            pixel.0[3] = faded.round().clamp(0.0, 255.0) as u8;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageBuffer;

    /// A 20x20 fully-opaque red square (a stand-in "cutout") and a matching
    /// fully-opaque blue "source" image to restore from.
    fn fixtures() -> (RgbaImage, RgbaImage) {
        let current = ImageBuffer::from_pixel(20, 20, Rgba([220, 40, 40, 255]));
        let source = ImageBuffer::from_pixel(20, 20, Rgba([40, 60, 220, 255]));
        (current, source)
    }

    fn dot_stroke(x: f32, y: f32, radius: f32) -> Stroke {
        Stroke {
            points: vec![StrokePoint { x, y }],
            radius,
        }
    }

    #[test]
    fn no_strokes_leaves_the_image_unchanged() {
        let (current, source) = fixtures();
        let out = apply_strokes(&current, &source, &[], "erase");
        assert_eq!(out, current);
    }

    #[test]
    fn erase_fades_alpha_to_zero_at_the_stroke_center() {
        let (current, source) = fixtures();
        let strokes = [dot_stroke(0.5, 0.5, 0.2)];
        let out = apply_strokes(&current, &source, &strokes, "erase");

        assert_eq!(out.get_pixel(10, 10).0[3], 0);
        // A far corner, well outside the brush, is untouched.
        assert_eq!(out.get_pixel(0, 0), current.get_pixel(0, 0));
    }

    #[test]
    fn restore_blends_toward_the_source_color_at_the_stroke_center() {
        let (current, source) = fixtures();
        let strokes = [dot_stroke(0.5, 0.5, 0.2)];
        let out = apply_strokes(&current, &source, &strokes, "restore");

        assert_eq!(out.get_pixel(10, 10), source.get_pixel(10, 10));
        assert_eq!(out.get_pixel(0, 0), current.get_pixel(0, 0));
    }

    #[test]
    fn brush_edge_is_feathered_not_a_hard_cutoff() {
        let (current, source) = fixtures();
        let strokes = [dot_stroke(0.5, 0.5, 0.2)];
        let out = apply_strokes(&current, &source, &strokes, "erase");

        // Just past the solid brush core, alpha should be partially faded
        // (neither fully erased nor fully untouched).
        let radius_px: f32 = 0.2 * 20.0; // 4px
        let edge_x = 10 + radius_px.round() as u32;
        if edge_x < 20 {
            let alpha = out.get_pixel(edge_x, 10).0[3];
            assert!(alpha > 0 && alpha < 255, "expected a feathered edge, got alpha={alpha}");
        }
    }
}
