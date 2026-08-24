//! Preprocessing, inference and postprocessing for background removal.
//!
//! Everything here runs entirely on-device via the `ort` ONNX Runtime
//! bindings. No image, filename or metadata this module touches ever
//! leaves the machine.

use std::path::Path;
use std::sync::Mutex;

use image::{imageops::FilterType, ImageBuffer, Luma, Rgba, RgbaImage};
use ndarray::Array4;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;

/// IS-Net's expected square input resolution.
const INPUT_SIZE: u32 = 1024;

/// Holds the lazily-built inference session. `ort` allows only one
/// environment per process, and a `Session` isn't safe to `run()`
/// concurrently from multiple threads, so every inference call goes
/// through this single mutex-guarded instance.
pub struct InferenceState(pub Mutex<Option<Session>>);

impl InferenceState {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

fn ensure_ort_environment() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // `commit()` is safe to call at most once per process; later calls
        // are simply ignored, so guarding with `Once` just avoids a wasted
        // build of the execution-provider list.
        ort::init().with_name("unbagrnd").commit();
    });
}

fn build_session(model_path: &Path) -> Result<Session, String> {
    ensure_ort_environment();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    Session::builder()
        .map_err(|e| format!("failed to initialize the inference session: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| format!("failed to configure the inference session: {e}"))?
        .with_intra_threads(threads)
        .map_err(|e| format!("failed to configure the inference session: {e}"))?
        .commit_from_file(model_path)
        .map_err(|e| format!("failed to load the model: {e}"))
}

/// Removes the background from an already-decoded image, returning the
/// result as an RGBA image with the cut-out subject on a transparent
/// background, at the original image's resolution.
pub fn remove_background(
    state: &InferenceState,
    model_path: &Path,
    original: &image::DynamicImage,
) -> Result<RgbaImage, String> {
    let (orig_w, orig_h) = (original.width(), original.height());
    if orig_w == 0 || orig_h == 0 {
        return Err("image has zero width or height".to_string());
    }

    let mask_1024 = {
        let mut guard = state
            .0
            .lock()
            .map_err(|_| "inference session lock was poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(build_session(model_path)?);
        }
        let session = guard.as_mut().expect("session was just initialized");
        run_inference(session, original)?
    };

    let mask_full = image::imageops::resize(&mask_1024, orig_w, orig_h, FilterType::Lanczos3);

    let original_rgba = original.to_rgba8();
    let mut out: RgbaImage = ImageBuffer::new(orig_w, orig_h);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let src = original_rgba.get_pixel(x, y);
        let alpha = mask_full.get_pixel(x, y).0[0];
        *pixel = Rgba([src.0[0], src.0[1], src.0[2], alpha]);
    }

    Ok(out)
}

/// Runs one forward pass and returns the predicted alpha mask at the
/// model's native 1024x1024 resolution.
fn run_inference(
    session: &mut Session,
    original: &image::DynamicImage,
) -> Result<ImageBuffer<Luma<u8>, Vec<u8>>, String> {
    let resized = original.resize_exact(INPUT_SIZE, INPUT_SIZE, FilterType::Lanczos3);
    let rgb = resized.to_rgb8();

    // Matches rembg's `BaseSession.normalize` exactly: every channel is
    // divided by the single brightest pixel value in the resized image
    // (not a fixed 255), then centered around a mean of 0.5 with a std of
    // 1.0. This is the input distribution IS-Net was trained on.
    let max_val = rgb
        .pixels()
        .flat_map(|p| p.0)
        .map(|c| c as f32)
        .fold(1e-6_f32, f32::max);

    let mut input = Array4::<f32>::zeros((1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize));
    for (x, y, pixel) in rgb.enumerate_pixels() {
        let [r, g, b] = pixel.0;
        let (x, y) = (x as usize, y as usize);
        input[[0, 0, y, x]] = (r as f32 / max_val) - 0.5;
        input[[0, 1, y, x]] = (g as f32 / max_val) - 0.5;
        input[[0, 2, y, x]] = (b as f32 / max_val) - 0.5;
    }

    let input_tensor =
        TensorRef::from_array_view(&input).map_err(|e| format!("failed to prepare input: {e}"))?;
    let outputs = session
        .run(ort::inputs![input_tensor])
        .map_err(|e| format!("inference failed: {e}"))?;

    let mask = outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| format!("failed to read model output: {e}"))?;

    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &v in mask.iter() {
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    }
    let range = (max - min).max(1e-6);

    let mask_bytes: Vec<u8> = mask
        .iter()
        .map(|&v| (((v - min) / range) * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();

    ImageBuffer::from_raw(INPUT_SIZE, INPUT_SIZE, mask_bytes)
        .ok_or_else(|| "unexpected model output size".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the full decode -> preprocess -> inference -> postprocess
    /// pipeline against a real cached model file. Skipped unless
    /// `UNBAGRND_TEST_MODEL_PATH` and `UNBAGRND_TEST_IMAGE_PATH` are set, so
    /// `cargo test` works offline in CI without a 170 MB model download.
    ///
    /// Run locally with:
    /// ```sh
    /// UNBAGRND_TEST_MODEL_PATH=/path/to/isnet-general-use.onnx \
    /// UNBAGRND_TEST_IMAGE_PATH=/path/to/a/photo.jpg \
    /// cargo test --release removes_background_from_a_real_photo -- --nocapture
    /// ```
    #[test]
    fn removes_background_from_a_real_photo() {
        let (Ok(model_path), Ok(image_path)) = (
            std::env::var("UNBAGRND_TEST_MODEL_PATH"),
            std::env::var("UNBAGRND_TEST_IMAGE_PATH"),
        ) else {
            eprintln!(
                "skipping: set UNBAGRND_TEST_MODEL_PATH and UNBAGRND_TEST_IMAGE_PATH to run this test"
            );
            return;
        };

        let original = image::open(&image_path).expect("test image should decode");
        let (orig_w, orig_h) = (original.width(), original.height());

        let state = InferenceState::new();
        let result = remove_background(&state, Path::new(&model_path), &original)
            .expect("background removal should succeed");

        assert_eq!(result.width(), orig_w);
        assert_eq!(result.height(), orig_h);

        if let Ok(out_path) = std::env::var("UNBAGRND_TEST_OUTPUT_PATH") {
            result.save(&out_path).expect("should save output preview");
        }

        // A real photo should produce a mask with genuine variation, not a
        // flat "everything transparent" or "everything opaque" output.
        let alphas: Vec<u8> = result.pixels().map(|p| p.0[3]).collect();
        let min = *alphas.iter().min().unwrap();
        let max = *alphas.iter().max().unwrap();
        assert!(
            max > min + 32,
            "expected the alpha mask to show real subject/background contrast, got min={min} max={max}"
        );

        // Sanity-check the RGB channels of visible pixels still match the
        // source photo (we must never alter color, only alpha).
        let original_rgba = original.to_rgba8();
        for (x, y, pixel) in result.enumerate_pixels() {
            let src = original_rgba.get_pixel(x, y);
            assert_eq!(pixel.0[0..3], src.0[0..3]);
        }
    }
}
