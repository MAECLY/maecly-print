//! HEIC/HEIF → PDF through the operating system's own decoder (FR-052).
//!
//! The `image` crate has no HEIF decoder at any feature combination, and
//! bundling `libheif` would drag a native build (and HEVC patent questions)
//! into the app. Both target OSes already ship a decoder, so we use theirs:
//!
//! - **macOS**: `sips -s format png <in> --out <work>/heif.png` (part of the
//!   base system, `/usr/bin/sips`), then the regular image → PDF path.
//! - **Windows**: Windows Imaging Component. WIC decodes HEIF only when the
//!   Microsoft Store *HEIF Image Extensions* (and, for HEIC, *HEVC Video
//!   Extensions*) are installed; without them the decoder lookup fails and the
//!   job pauses with a [`EngineError::DependencyMissing`] that names them.
//! - **Linux**: no system decoder we can rely on, so the same
//!   `DependencyMissing` explains it.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::WorkDir;
use crate::engine::EngineError;

/// HEIF-family extensions.
pub const HEIF_EXTS: &[&str] = &["heic", "heif"];

/// FR-058 message for a Windows machine without the HEIF codec.
pub const WINDOWS_CODEC_MISSING: &str = "The HEIF image decoder is missing — HEIC/HEIF photos \
cannot be printed until it is installed. Install \"HEIF Image Extensions\" (and \"HEVC Video \
Extensions\" for .heic) from the Microsoft Store, then retry the job";

/// FR-058 message on an OS with no system HEIF decoder.
pub const NO_SYSTEM_DECODER: &str = "No HEIF image decoder is available on this system — \
convert the photo to JPEG or PNG first";

/// The `sips` argument vector converting `input` to the PNG `png`. Pure.
pub fn sips_args(input: &Path, png: &Path) -> Vec<String> {
    vec![
        "-s".to_string(),
        "format".to_string(),
        "png".to_string(),
        input.to_string_lossy().into_owned(),
        "--out".to_string(),
        png.to_string_lossy().into_owned(),
    ]
}

/// Convert with the `sips` at `sips` (macOS), writing the PDF to `output`.
pub fn convert_with_sips(
    sips: &Path,
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), EngineError> {
    let png = work.path().join("heif.png");
    let mut cmd = std::process::Command::new(sips);
    cmd.args(sips_args(input, &png)).current_dir(work.path());
    let log = work.path().join("sips.log");
    super::process::run_bounded(cmd, "sips", cancel, timeout, &log)?;
    if !png.is_file() {
        return Err(EngineError::ConversionFailed(
            "the system image converter could not read this HEIC/HEIF file".to_string(),
        ));
    }
    super::image::to_pdf(&png, output, progress)
}

/// Convert `input` to a PDF at `output` with this OS's decoder.
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), EngineError> {
    #[cfg(target_os = "macos")]
    {
        let sips = Path::new("/usr/bin/sips");
        if !sips.is_file() {
            return Err(EngineError::DependencyMissing(
                "sips (the macOS image tool) is missing — reinstall macOS command line tools"
                    .to_string(),
            ));
        }
        convert_with_sips(sips, input, output, work, cancel, timeout, progress)
    }
    #[cfg(windows)]
    {
        let _ = (work, cancel, timeout);
        progress(0.0);
        let (width, height, rgb) = wic::decode_rgb8(input)?;
        progress(0.5);
        super::image::rgb_to_pdf(width, height, rgb, output)?;
        progress(1.0);
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (input, output, work, cancel, timeout, progress);
        Err(EngineError::DependencyMissing(
            NO_SYSTEM_DECODER.to_string(),
        ))
    }
}

/// Windows Imaging Component decoding.
#[cfg(windows)]
mod wic {
    use std::path::Path;

    use windows::core::{GUID, HSTRING};
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat24bppRGB, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };

    use crate::engine::EngineError;

    /// `WINCODEC_ERR_COMPONENTNOTFOUND`: no decoder claims the file — for a
    /// `.heic` that means the Store codec is not installed.
    const COMPONENT_NOT_FOUND: i32 = 0x8898_2F50_u32 as i32;
    /// `WINCODEC_ERR_UNKNOWNIMAGEFORMAT`, the other "no codec" answer.
    const UNKNOWN_IMAGE_FORMAT: i32 = 0x8898_2F07_u32 as i32;
    /// Same pixel ceiling as the built-in decoder (`image::decode_rgb8`).
    const MAX_SIDE: u32 = 30_000;

    /// Balances a successful `CoInitializeEx` on this (blocking-pool) thread.
    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: paired with the successful CoInitializeEx below on
                // the same thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn failed(e: windows::core::Error) -> EngineError {
        let code = e.code().0;
        if code == COMPONENT_NOT_FOUND || code == UNKNOWN_IMAGE_FORMAT {
            EngineError::DependencyMissing(super::WINDOWS_CODEC_MISSING.to_string())
        } else {
            EngineError::ConversionFailed(format!("could not decode the HEIF image: {e}"))
        }
    }

    pub fn decode_rgb8(input: &Path) -> Result<(u32, u32, Vec<u8>), EngineError> {
        // SAFETY: plain COM calls on interfaces owned by this function; every
        // out-pointer is a live local and the pixel buffer is sized from the
        // reported dimensions before CopyPixels writes into it.
        unsafe {
            // S_OK / S_FALSE both need a matching CoUninitialize; a thread
            // already in another apartment (RPC_E_CHANGED_MODE) still lets us
            // create in-proc objects, but must not be uninitialised by us.
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _guard = ComGuard(hr.is_ok());

            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(failed)?;
            let decoder = factory
                .CreateDecoderFromFilename(
                    &HSTRING::from(input.as_os_str()),
                    None,
                    GENERIC_READ,
                    WICDecodeMetadataCacheOnDemand,
                )
                .map_err(failed)?;
            let frame = decoder.GetFrame(0).map_err(failed)?;
            let converter = factory.CreateFormatConverter().map_err(failed)?;
            let rgb24: GUID = GUID_WICPixelFormat24bppRGB;
            converter
                .Initialize(
                    &frame,
                    &rgb24,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .map_err(failed)?;
            let (mut width, mut height) = (0u32, 0u32);
            converter.GetSize(&mut width, &mut height).map_err(failed)?;
            if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
                return Err(EngineError::ConversionFailed(format!(
                    "unsupported HEIF image size {width}×{height}"
                )));
            }
            let stride = width * 3;
            let mut pixels = vec![0u8; stride as usize * height as usize];
            converter
                .CopyPixels(std::ptr::null(), stride, &mut pixels)
                .map_err(failed)?;
            Ok((width, height, pixels))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_052_sips_args_convert_to_png_without_a_shell() {
        assert_eq!(
            sips_args(Path::new("/p/IMG 1.HEIC"), Path::new("/w/heif.png")),
            [
                "-s",
                "format",
                "png",
                "/p/IMG 1.HEIC",
                "--out",
                "/w/heif.png"
            ]
        );
    }

    #[test]
    fn fr_058_windows_codec_message_names_the_store_extension() {
        assert!(WINDOWS_CODEC_MISSING.contains("HEIF Image Extensions"));
        assert!(WINDOWS_CODEC_MISSING.contains("Microsoft Store"));
    }
}
