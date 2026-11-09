//! System clipboard image access for "paste an image" and "copy the
//! result". Desktop only: `arboard` doesn't support mobile, and on Android
//! the frontend hides these buttons (a DOM paste event still works there,
//! through `save_pasted_image`).

use std::path::PathBuf;

use image::RgbaImage;

/// What a paste found on the clipboard.
pub enum Pasted {
    /// Files copied in a file manager. Checked first: Finder also puts each
    /// copied file's *icon* on the clipboard as an image.
    Files(Vec<PathBuf>),
    /// Raw image data (a screenshot, "Copy image" in a browser, ...).
    Image(RgbaImage),
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod imp {
    use std::borrow::Cow;
    use std::sync::Mutex;

    use image::RgbaImage;

    use super::Pasted;

    /// One long-lived clipboard handle. On X11 the app owns whatever it
    /// copies and has to keep serving it; dropping the handle right after a
    /// copy can make the data vanish before anyone pastes it.
    static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

    fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, String>) -> Result<T, String> {
        let mut guard = CLIPBOARD
            .lock()
            .map_err(|_| "clipboard lock was poisoned".to_string())?;
        if guard.is_none() {
            *guard = Some(
                arboard::Clipboard::new().map_err(|e| format!("could not open the clipboard: {e}"))?,
            );
        }
        f(guard.as_mut().expect("initialized above"))
    }

    pub fn read() -> Result<Pasted, String> {
        with_clipboard(|cb| {
            if let Ok(files) = cb.get().file_list() {
                if !files.is_empty() {
                    return Ok(Pasted::Files(files));
                }
            }
            let data = cb.get_image().map_err(|e| match e {
                arboard::Error::ContentNotAvailable => "the clipboard doesn't contain an image".to_string(),
                other => format!("could not read the clipboard: {other}"),
            })?;
            RgbaImage::from_raw(data.width as u32, data.height as u32, data.bytes.into_owned())
                .map(Pasted::Image)
                .ok_or_else(|| "the clipboard image has an unexpected size".to_string())
        })
    }

    pub fn write_image(image: &RgbaImage) -> Result<(), String> {
        with_clipboard(|cb| {
            cb.set_image(arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: Cow::Borrowed(image.as_raw()),
            })
            .map_err(|e| format!("could not copy to the clipboard: {e}"))
        })
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use image::RgbaImage;

    use super::Pasted;

    const UNSUPPORTED: &str = "clipboard images aren't supported on this platform";

    pub fn read() -> Result<Pasted, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn write_image(_image: &RgbaImage) -> Result<(), String> {
        Err(UNSUPPORTED.to_string())
    }
}

/// The files or image currently on the system clipboard.
pub fn read() -> Result<Pasted, String> {
    imp::read()
}

/// Puts `image` on the system clipboard, with its transparency.
pub fn write_image(image: &RgbaImage) -> Result<(), String> {
    imp::write_image(image)
}
