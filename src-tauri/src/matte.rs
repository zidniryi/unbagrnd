//! On-device alpha-matte tools for the single-image editors: exporting the
//! cutout's alpha channel as a black-and-white mask, and adjusting the mask's
//! edge (shift / smooth / feather). Everything here only ever touches the
//! alpha channel of an already background-removed RGBA image - no AI or
//! network involved, consistent with the rest of the app.
//!
//! All edge amounts are expressed as a percentage of the image's longer
//! side (like the refine brush), so the same settings look the same on the
//! downscaled live preview and on the full-resolution export.

use image::{Rgba, RgbaImage};
use serde::Deserialize;

/// Largest edge shift either way, as a % of the image's longer side.
pub const MAX_SHIFT_PCT: f32 = 2.0;
/// Largest smoothing amount, as a % of the image's longer side.
pub const MAX_SMOOTH_PCT: f32 = 1.0;
/// Largest feather amount, as a % of the image's longer side.
pub const MAX_FEATHER_PCT: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EdgeSpec {
    /// Negative erodes (shrinks the subject), positive dilates (grows it).
    /// `-MAX_SHIFT_PCT..=MAX_SHIFT_PCT`.
    pub shift_pct: f32,
    /// Evens out jagged or noisy outlines. `0..=MAX_SMOOTH_PCT`.
    pub smooth_pct: f32,
    /// Softens the edge into a gradual fade. `0..=MAX_FEATHER_PCT`.
    pub feather_pct: f32,
}

impl EdgeSpec {
    pub fn is_identity(&self) -> bool {
        self.shift_pct == 0.0 && self.smooth_pct == 0.0 && self.feather_pct == 0.0
    }

    /// Rejects NaN/infinite and out-of-range values rather than silently
    /// clamping them, so a frontend bug surfaces instead of producing a
    /// subtly different result than the sliders showed.
    pub fn validate(&self) -> Result<(), String> {
        let check = |name: &str, value: f32, min: f32, max: f32| -> Result<(), String> {
            if value.is_finite() && (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be between {min} and {max} (got {value})"))
            }
        };
        check("shift", self.shift_pct, -MAX_SHIFT_PCT, MAX_SHIFT_PCT)?;
        check("smooth", self.smooth_pct, 0.0, MAX_SMOOTH_PCT)?;
        check("feather", self.feather_pct, 0.0, MAX_FEATHER_PCT)
    }
}

/// The cutout's alpha channel as an opaque grayscale image: white where the
/// subject is fully visible, black where it's fully transparent, gray in
/// between - the standard "alpha matte" designers use as a layer mask.
pub fn alpha_matte(image: &RgbaImage) -> RgbaImage {
    let mut out = image.clone();
    for pixel in out.pixels_mut() {
        let a = pixel.0[3];
        *pixel = Rgba([a, a, a, 255]);
    }
    out
}

/// Returns `image` with `spec` applied to its alpha channel (shift, then
/// smooth, then feather); colors are left untouched. Dilating therefore
/// reveals the source photo's own pixels just outside the old edge.
pub fn apply_edges(image: &RgbaImage, spec: &EdgeSpec) -> RgbaImage {
    if spec.is_identity() {
        return image.clone();
    }
    let (w, h) = image.dimensions();
    let (wu, hu) = (w as usize, h as usize);
    if wu == 0 || hu == 0 {
        return image.clone();
    }
    let long_side = w.max(h) as f32;

    let mut plane: Vec<u8> = image.pixels().map(|p| p.0[3]).collect();

    let shift_px = (spec.shift_pct.abs() / 100.0 * long_side).round() as usize;
    if shift_px > 0 {
        if spec.shift_pct > 0.0 {
            morph(&mut plane, wu, hu, shift_px, |a, b| a.max(b));
        } else {
            morph(&mut plane, wu, hu, shift_px, |a, b| a.min(b));
        }
    }

    // Both remaining steps are blurs, so share one float plane. The sigma
    // is half the configured amount: the slider maximum then reads as a
    // generous blur rather than an unusable one.
    let smooth_sigma = spec.smooth_pct / 100.0 * long_side * 0.5;
    let feather_sigma = spec.feather_pct / 100.0 * long_side * 0.5;
    if smooth_sigma >= MIN_BLUR_SIGMA || feather_sigma >= MIN_BLUR_SIGMA {
        let mut soft: Vec<f32> = plane.iter().map(|&a| a as f32).collect();

        if smooth_sigma >= MIN_BLUR_SIGMA {
            gaussian_blur(&mut soft, wu, hu, smooth_sigma);
            // Blurring alone would just soften the edge; pushing the result
            // back through a steep curve around the midpoint keeps the edge
            // crisp but follows the blurred (i.e. smoothed) contour.
            let steepness = 1.0 + 2.0 * smooth_sigma;
            for v in soft.iter_mut() {
                let t = *v / 255.0;
                *v = (((t - 0.5) * steepness + 0.5).clamp(0.0, 1.0)) * 255.0;
            }
        }
        if feather_sigma >= MIN_BLUR_SIGMA {
            gaussian_blur(&mut soft, wu, hu, feather_sigma);
        }

        for (dst, v) in plane.iter_mut().zip(soft.iter()) {
            *dst = v.round().clamp(0.0, 255.0) as u8;
        }
    }

    let mut out = image.clone();
    for (pixel, &a) in out.pixels_mut().zip(plane.iter()) {
        pixel.0[3] = a;
    }
    out
}

/// Below this a Gaussian blur is a no-op at pixel granularity.
const MIN_BLUR_SIGMA: f32 = 0.5;

/// Grayscale erosion/dilation (`op` = min/max) by roughly `radius` pixels,
/// built from `radius` successive 3x3 steps that alternate between a full
/// square and a plus-shaped neighborhood. Together they approximate a round
/// brush (an octagon) rather than the boxy look of repeated squares, using
/// nothing but cheap, vectorizable 3-tap passes. Pixels outside the image
/// don't take part, so a subject touching the border isn't eaten from that
/// side when eroding.
fn morph(plane: &mut [u8], w: usize, h: usize, radius: usize, op: impl Fn(u8, u8) -> u8 + Copy) {
    let mut a = vec![0u8; plane.len()];
    let mut b = vec![0u8; plane.len()];
    for step in 0..radius {
        if step % 2 == 0 {
            horizontal_3(plane, &mut a, w, op);
            vertical_3(&a, plane, w, h, op);
        } else {
            horizontal_3(plane, &mut a, w, op);
            vertical_3(plane, &mut b, w, h, op);
            for ((dst, &x), &y) in plane.iter_mut().zip(a.iter()).zip(b.iter()) {
                *dst = op(x, y);
            }
        }
    }
}

fn horizontal_3(src: &[u8], dst: &mut [u8], w: usize, op: impl Fn(u8, u8) -> u8 + Copy) {
    for (s, d) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        if w == 1 {
            d[0] = s[0];
            continue;
        }
        d[0] = op(s[0], s[1]);
        d[w - 1] = op(s[w - 2], s[w - 1]);
        for (out, win) in d[1..w - 1].iter_mut().zip(s.windows(3)) {
            *out = op(op(win[0], win[1]), win[2]);
        }
    }
}

fn vertical_3(src: &[u8], dst: &mut [u8], w: usize, h: usize, op: impl Fn(u8, u8) -> u8 + Copy) {
    for y in 0..h {
        let up = &src[y.saturating_sub(1) * w..][..w];
        let mid = &src[y * w..][..w];
        let down = &src[(y + 1).min(h - 1) * w..][..w];
        for (((out, &u), &m), &d) in dst[y * w..][..w]
            .iter_mut()
            .zip(up.iter())
            .zip(mid.iter())
            .zip(down.iter())
        {
            *out = op(op(u, m), d);
        }
    }
}

/// Approximates a Gaussian blur with three successive box blurs, which
/// costs the same regardless of `sigma` (a true kernel gets very slow for
/// the large sigmas a full-resolution feather needs). Edges replicate.
fn gaussian_blur(plane: &mut [f32], w: usize, h: usize, sigma: f32) {
    let mut tmp = vec![0f32; plane.len()];
    for radius in box_blur_radii(sigma) {
        if radius == 0 {
            continue;
        }
        box_blur_horizontal(plane, &mut tmp, w, radius);
        box_blur_vertical(&tmp, plane, w, h, radius);
    }
}

/// Radii of the three box blurs that together best match a Gaussian of
/// standard deviation `sigma` (the usual "ideal averaging filter" widths).
fn box_blur_radii(sigma: f32) -> [usize; 3] {
    const PASSES: f32 = 3.0;
    let sigma = sigma as f64;
    let ideal = ((12.0 * sigma * sigma / PASSES as f64) + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let l = lower as f64;
    let n = PASSES as f64;
    let wide_passes = ((12.0 * sigma * sigma - n * l * l - 4.0 * n * l - 3.0 * n) / (-4.0 * l - 4.0))
        .round()
        .clamp(0.0, n) as usize;

    let mut radii = [0usize; 3];
    for (i, r) in radii.iter_mut().enumerate() {
        let width = if i < wide_passes { lower } else { upper };
        *r = ((width - 1) / 2) as usize;
    }
    radii
}

fn box_blur_horizontal(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    let window = (2 * r + 1) as f64;
    for (s, d) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        // Window centered on x = 0: `r` virtual copies of s[0] to its left
        // (edge replication) plus s[0..=r].
        let mut acc = s[0] as f64 * r as f64;
        for i in 0..=r {
            acc += s[i.min(w - 1)] as f64;
        }
        for x in 0..w {
            d[x] = (acc / window) as f32;
            acc += s[(x + r + 1).min(w - 1)] as f64 - s[x.saturating_sub(r)] as f64;
        }
    }
}

fn box_blur_vertical(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let window = (2 * r + 1) as f64;
    // One running sum per column, so the whole pass walks memory row by row
    // instead of striding down columns.
    let mut acc: Vec<f64> = src[..w].iter().map(|&v| v as f64 * r as f64).collect();
    for y in 0..=r {
        for (a, &v) in acc.iter_mut().zip(src[y.min(h - 1) * w..][..w].iter()) {
            *a += v as f64;
        }
    }
    for y in 0..h {
        for (out, &a) in dst[y * w..][..w].iter_mut().zip(acc.iter()) {
            *out = (a / window) as f32;
        }
        let add = &src[(y + r + 1).min(h - 1) * w..][..w];
        let remove = &src[y.saturating_sub(r) * w..][..w];
        for ((a, &p), &m) in acc.iter_mut().zip(add.iter()).zip(remove.iter()) {
            *a += p as f64 - m as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageBuffer;

    /// A 200x200 image with a fully-opaque 40x40 red square at
    /// x, y in 80..120, transparent elsewhere.
    fn square() -> RgbaImage {
        ImageBuffer::from_fn(200, 200, |x, y| {
            if (80..120).contains(&x) && (80..120).contains(&y) {
                Rgba([220, 40, 40, 255])
            } else {
                Rgba([220, 40, 40, 0])
            }
        })
    }

    fn alpha(image: &RgbaImage, x: u32, y: u32) -> u8 {
        image.get_pixel(x, y).0[3]
    }

    #[test]
    fn alpha_matte_is_opaque_grayscale_of_the_alpha_channel() {
        let mut image = square();
        image.put_pixel(0, 0, Rgba([10, 20, 30, 77]));
        let matte = alpha_matte(&image);

        assert_eq!(matte.get_pixel(100, 100), &Rgba([255, 255, 255, 255]));
        assert_eq!(matte.get_pixel(5, 5), &Rgba([0, 0, 0, 255]));
        assert_eq!(matte.get_pixel(0, 0), &Rgba([77, 77, 77, 255]));
    }

    #[test]
    fn identity_spec_leaves_the_image_unchanged() {
        let image = square();
        assert_eq!(apply_edges(&image, &EdgeSpec::default()), image);
    }

    #[test]
    fn positive_shift_grows_the_subject() {
        let image = square();
        // 2% of 200px = 4px.
        let spec = EdgeSpec { shift_pct: 2.0, ..Default::default() };
        let out = apply_edges(&image, &spec);

        assert_eq!(alpha(&out, 76, 100), 255, "edge should have moved out 4px");
        assert_eq!(alpha(&out, 74, 100), 0, "but no further");
        // Colors are untouched.
        assert_eq!(out.get_pixel(100, 100).0[..3], [220, 40, 40]);
    }

    #[test]
    fn negative_shift_shrinks_the_subject() {
        let image = square();
        let spec = EdgeSpec { shift_pct: -2.0, ..Default::default() };
        let out = apply_edges(&image, &spec);

        assert_eq!(alpha(&out, 82, 100), 0, "edge should have moved in 4px");
        assert_eq!(alpha(&out, 84, 100), 255);
        assert_eq!(alpha(&out, 100, 100), 255, "interior is untouched");
    }

    #[test]
    fn erosion_does_not_eat_a_subject_from_the_image_border() {
        // Opaque everywhere: nothing is "outside", so nothing should erode.
        let image: RgbaImage = ImageBuffer::from_pixel(50, 50, Rgba([1, 2, 3, 255]));
        let spec = EdgeSpec { shift_pct: -2.0, ..Default::default() };
        let out = apply_edges(&image, &spec);
        assert_eq!(out, image);
    }

    #[test]
    fn feather_fades_the_edge_gradually() {
        let image = square();
        let spec = EdgeSpec { feather_pct: 2.0, ..Default::default() };
        let out = apply_edges(&image, &spec);

        let at_edge = alpha(&out, 80, 100);
        assert!(at_edge > 0 && at_edge < 255, "expected a partial alpha at the old edge, got {at_edge}");
        assert_eq!(alpha(&out, 100, 100), 255);
        assert_eq!(alpha(&out, 20, 20), 0);

        // Fade is monotonic going outward.
        let mut previous = 255u8;
        for x in (60..=100).rev() {
            let a = alpha(&out, x, 100);
            assert!(a <= previous, "alpha should not increase moving outward (x={x})");
            previous = a;
        }
    }

    #[test]
    fn smooth_fills_a_one_pixel_hole() {
        let mut image: RgbaImage = ImageBuffer::from_fn(400, 400, |x, y| {
            if (100..300).contains(&x) && (100..300).contains(&y) {
                Rgba([9, 9, 9, 255])
            } else {
                Rgba([9, 9, 9, 0])
            }
        });
        image.put_pixel(200, 200, Rgba([9, 9, 9, 0]));

        let spec = EdgeSpec { smooth_pct: 1.0, ..Default::default() };
        let out = apply_edges(&image, &spec);

        assert_eq!(alpha(&out, 200, 200), 255, "isolated hole should be smoothed away");
        assert_eq!(alpha(&out, 200, 200 - 60), 255);
        assert_eq!(alpha(&out, 50, 50), 0);
    }

    #[test]
    fn validate_rejects_out_of_range_and_non_finite_values() {
        assert!(EdgeSpec { shift_pct: 2.5, ..Default::default() }.validate().is_err());
        assert!(EdgeSpec { shift_pct: -2.5, ..Default::default() }.validate().is_err());
        assert!(EdgeSpec { smooth_pct: -0.1, ..Default::default() }.validate().is_err());
        assert!(EdgeSpec { feather_pct: f32::NAN, ..Default::default() }.validate().is_err());
        assert!(EdgeSpec { shift_pct: -2.0, smooth_pct: 1.0, feather_pct: 2.0 }.validate().is_ok());
    }

    #[test]
    fn box_blur_is_a_no_op_on_a_constant_plane() {
        let mut plane = vec![200.0f32; 30 * 20];
        gaussian_blur(&mut plane, 30, 20, 3.0);
        assert!(plane.iter().all(|&v| (v - 200.0).abs() < 0.01));
    }
}
