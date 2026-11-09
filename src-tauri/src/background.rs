//! On-device background fill (solid color or linear gradient) + drop shadow
//! compositing for the single-image "edit background" panel and batch runs. Everything here operates on an
//! already background-removed RGBA image; no AI or network involved,
//! consistent with the rest of the app.

use image::{ImageBuffer, Luma, Rgba, RgbaImage};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowSpec {
    /// One of "natural", "overhead", "left", "right", "custom".
    pub preset: String,
    /// 0-100.
    pub opacity: u8,
    /// Only used when `preset == "custom"`: direction in degrees, 0 =
    /// straight down, increasing clockwise.
    pub angle_deg: Option<f32>,
    /// Only used when `preset == "custom"`: offset distance, as a
    /// percentage of the image's longer side.
    pub distance_pct: Option<f32>,
}

/// A two-stop linear gradient, with the same angle convention as CSS
/// `linear-gradient(<angle>deg, from, to)` so the frontend can draw its
/// swatches with plain CSS and get an identical result: 0 = bottom to top,
/// 90 = left to right, 180 = top to bottom.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradientSpec {
    pub from: String,
    pub to: String,
    pub angle_deg: f32,
}

/// What goes behind the subject.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    Transparent,
    Solid(Rgba<u8>),
    Linear { from: Rgba<u8>, to: Rgba<u8>, angle_deg: f32 },
}

impl Fill {
    /// Resolves the frontend's background options. A gradient takes
    /// precedence over a solid color; neither means transparent.
    pub fn resolve(background_hex: Option<&str>, gradient: Option<&GradientSpec>) -> Result<Fill, String> {
        if let Some(g) = gradient {
            if !g.angle_deg.is_finite() {
                return Err("invalid gradient angle".to_string());
            }
            return Ok(Fill::Linear {
                from: parse_hex_color(&g.from)?,
                to: parse_hex_color(&g.to)?,
                angle_deg: g.angle_deg,
            });
        }
        Ok(match background_hex {
            Some(hex) => Fill::Solid(parse_hex_color(hex)?),
            None => Fill::Transparent,
        })
    }

    pub fn is_transparent(&self) -> bool {
        matches!(self, Fill::Transparent)
    }

    /// A `w` x `h` canvas painted with this fill.
    fn render(&self, w: u32, h: u32) -> RgbaImage {
        match *self {
            Fill::Transparent => ImageBuffer::from_pixel(w, h, Rgba([0, 0, 0, 0])),
            Fill::Solid(color) => ImageBuffer::from_pixel(w, h, color),
            Fill::Linear { from, to, angle_deg } => {
                // CSS gradient line: through the center, along (sin a, -cos a),
                // long enough that the corners land exactly on 0% and 100%.
                let angle = angle_deg.to_radians();
                let (dx, dy) = (angle.sin(), -angle.cos());
                let (wf, hf) = (w as f32, h as f32);
                let length = (wf * dx.abs() + hf * dy.abs()).max(1.0);
                let (cx, cy) = (wf / 2.0, hf / 2.0);
                ImageBuffer::from_fn(w, h, |x, y| {
                    let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let t = ((px * dx + py * dy) / length + 0.5).clamp(0.0, 1.0);
                    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                    Rgba([lerp(from.0[0], to.0[0]), lerp(from.0[1], to.0[1]), lerp(from.0[2], to.0[2]), 255])
                })
            }
        }
    }
}

/// Parses a `#rrggbb` (or bare `rrggbb`) hex string into an opaque color.
pub fn parse_hex_color(hex: &str) -> Result<Rgba<u8>, String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("invalid color \"{hex}\" (expected #rrggbb)"));
    }
    let byte = |i: usize| -> Result<u8, String> {
        u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| format!("invalid color \"{hex}\""))
    };
    Ok(Rgba([byte(0)?, byte(2)?, byte(4)?, 255]))
}

/// Standard "source over destination" alpha compositing, in straight
/// (non-premultiplied) alpha.
fn over(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src.0[3] as f32 / 255.0;
    if sa <= 0.0 {
        return dst;
    }
    let da = dst.0[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    let mix = |s: u8, d: u8| -> u8 {
        let s = s as f32 / 255.0;
        let d = d as f32 / 255.0;
        (((s * sa + d * da * (1.0 - sa)) / out_a) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([
        mix(src.0[0], dst.0[0]),
        mix(src.0[1], dst.0[1]),
        mix(src.0[2], dst.0[2]),
        (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
    ])
}

/// Offset (dx, dy) in pixels and blur sigma for a shadow, sized relative to
/// the image so the look stays consistent across resolutions.
fn shadow_params(spec: &ShadowSpec, w: u32, h: u32) -> (f32, f32, f32) {
    let (w, h) = (w as f32, h as f32);
    let long_side = w.max(h);
    match spec.preset.as_str() {
        "overhead" => (0.0, 0.03 * h, 0.05 * long_side),
        "left" => (-0.12 * w, 0.05 * h, 0.025 * long_side),
        "right" => (0.12 * w, 0.05 * h, 0.025 * long_side),
        "custom" => {
            let angle = spec.angle_deg.unwrap_or(0.0).to_radians();
            let distance = spec.distance_pct.unwrap_or(8.0).clamp(0.0, 100.0) / 100.0 * long_side;
            (distance * angle.sin(), distance * angle.cos(), 0.03 * long_side)
        }
        // "natural" and anything unrecognized.
        _ => (0.0, 0.06 * h, 0.02 * long_side),
    }
}

/// Composites `image` (an already background-removed RGBA cutout) onto
/// `fill`, with an optional drop shadow rendered from the cutout's own
/// alpha channel — entirely on-device, no AI involved.
pub fn composite(image: &RgbaImage, fill: &Fill, shadow: Option<&ShadowSpec>) -> RgbaImage {
    let (w, h) = image.dimensions();
    let mut out = fill.render(w, h);

    if let Some(spec) = shadow {
        let (dx, dy, sigma) = shadow_params(spec, w, h);
        let (dx, dy) = (dx.round() as i32, dy.round() as i32);

        // The shadow's raw shape, before blurring: the subject's own alpha
        // channel, shifted by the preset's offset.
        let mut mask: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(w, h);
        for (x, y, pixel) in image.enumerate_pixels() {
            let alpha = pixel.0[3];
            if alpha == 0 {
                continue;
            }
            let (tx, ty) = (x as i32 + dx, y as i32 + dy);
            if tx >= 0 && ty >= 0 && (tx as u32) < w && (ty as u32) < h {
                mask.put_pixel(tx as u32, ty as u32, Luma([alpha]));
            }
        }
        let blurred = image::imageops::blur(&mask, sigma.max(0.5));
        let opacity = spec.opacity.min(100) as f32 / 100.0;

        for (x, y, pixel) in out.enumerate_pixels_mut() {
            let strength = blurred.get_pixel(x, y).0[0] as f32 * opacity;
            if strength <= 0.0 {
                continue;
            }
            *pixel = over(*pixel, Rgba([0, 0, 0, strength.round().clamp(0.0, 255.0) as u8]));
        }
    }

    for (x, y, pixel) in out.enumerate_pixels_mut() {
        *pixel = over(*pixel, *image.get_pixel(x, y));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10x10 fully-opaque red square on an otherwise transparent canvas.
    fn cutout() -> RgbaImage {
        ImageBuffer::from_fn(10, 10, |x, y| {
            if (2..8).contains(&x) && (2..8).contains(&y) {
                Rgba([220, 40, 40, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    #[test]
    fn no_background_no_shadow_is_unchanged() {
        let image = cutout();
        let out = composite(&image, &Fill::Transparent, None);
        assert_eq!(out, image);
    }

    #[test]
    fn solid_background_fills_transparent_pixels_and_keeps_subject() {
        let image = cutout();
        let fill = Fill::resolve(Some("#ffffff"), None).unwrap();
        let out = composite(&image, &fill, None);
        assert_eq!(out.get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(out.get_pixel(4, 4), &Rgba([220, 40, 40, 255]));
    }

    #[test]
    fn rejects_malformed_hex_colors() {
        assert!(Fill::resolve(Some("not-a-color"), None).is_err());
        let gradient = GradientSpec { from: "#fff".to_string(), to: "#000000".to_string(), angle_deg: 90.0 };
        assert!(Fill::resolve(None, Some(&gradient)).is_err());
    }

    #[test]
    fn gradient_takes_precedence_and_runs_in_the_css_direction() {
        let gradient = GradientSpec { from: "#000000".to_string(), to: "#ffffff".to_string(), angle_deg: 90.0 };
        let fill = Fill::resolve(Some("#ff0000"), Some(&gradient)).unwrap();
        assert!(matches!(fill, Fill::Linear { .. }));

        // 90deg = left to right: dark on the left edge, light on the right,
        // and the same down each column.
        let out = composite(&ImageBuffer::new(100, 10), &fill, None);
        assert!(out.get_pixel(0, 5).0[0] < 5);
        assert!(out.get_pixel(99, 5).0[0] > 250);
        assert_eq!(out.get_pixel(50, 0), out.get_pixel(50, 9));

        // 180deg = top to bottom.
        let down = Fill::Linear { from: Rgba([0, 0, 0, 255]), to: Rgba([255, 255, 255, 255]), angle_deg: 180.0 };
        let out = composite(&ImageBuffer::new(10, 100), &down, None);
        assert!(out.get_pixel(5, 0).0[0] < out.get_pixel(5, 99).0[0]);
    }

    #[test]
    fn shadow_darkens_pixels_below_the_subject_without_touching_the_subject_itself() {
        let image = cutout();
        let shadow = ShadowSpec {
            preset: "natural".to_string(),
            opacity: 100,
            angle_deg: None,
            distance_pct: None,
        };
        let fill = Fill::resolve(Some("#ffffff"), None).unwrap();
        let out = composite(&image, &fill, Some(&shadow));

        // The subject itself is drawn last, untouched by the shadow layer beneath it.
        assert_eq!(out.get_pixel(4, 4), &Rgba([220, 40, 40, 255]));

        // A pixel below the subject, on the white background, should have
        // been darkened by the blurred shadow ("natural" offsets downward).
        let shadowed = out.get_pixel(4, 8);
        assert!(
            shadowed.0[0] < 255,
            "expected the shadow to darken pixels below the subject, got {shadowed:?}"
        );
    }
}
