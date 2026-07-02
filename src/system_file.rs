use std::path::Path;

#[cfg(windows)]
use std::{collections::HashMap, sync::Arc, sync::Mutex, sync::OnceLock};

#[cfg(windows)]
use gpui::{Image, ImageFormat};

#[cfg(windows)]
static FILE_ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Image>>>>> = OnceLock::new();

#[cfg(windows)]
pub(crate) fn file_icon(path: &Path) -> Option<Arc<Image>> {
    let key = icon_cache_key(path);
    let cache = FILE_ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(cache) = cache.lock()
        && let Some(icon) = cache.get(&key)
    {
        return icon.clone();
    }

    let icon = load_file_icon(&key);

    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, icon.clone());
    }

    icon
}

#[cfg(not(windows))]
pub(crate) fn file_icon(_path: &Path) -> Option<std::sync::Arc<gpui::Image>> {
    None
}

pub(crate) fn reveal(path: &Path) -> Result<(), String> {
    platform_reveal(path)
}

#[cfg(windows)]
fn icon_cache_key(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
        .unwrap_or_else(|| "__file__".to_string())
}

#[cfg(windows)]
fn load_file_icon(extension: &str) -> Option<Arc<Image>> {
    use std::{ffi::OsStr, mem::size_of, os::windows::ffi::OsStrExt};
    use windows::Win32::{
        Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
        UI::{
            Shell::{
                SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
            },
            WindowsAndMessaging::DestroyIcon,
        },
    };
    use windows::core::PCWSTR;

    let query = OsStr::new(extension)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut file_info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(query.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut file_info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        )
    };

    if result == 0 || file_info.hIcon.is_invalid() {
        return None;
    }

    let icon = unsafe { hicon_to_png(file_info.hIcon) }
        .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));

    let _ = unsafe { DestroyIcon(file_info.hIcon) };
    icon
}

#[cfg(windows)]
unsafe fn hicon_to_png(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    use windows::Win32::{
        Graphics::Gdi::DeleteObject,
        UI::WindowsAndMessaging::{GetIconInfo, ICONINFO},
    };

    let mut icon_info = ICONINFO::default();
    unsafe { GetIconInfo(hicon, &mut icon_info).ok()? };

    let png = if !icon_info.hbmColor.is_invalid() {
        unsafe { hbitmap_to_png(icon_info.hbmColor) }
    } else {
        None
    };

    if !icon_info.hbmColor.is_invalid() {
        let _ = unsafe { DeleteObject(icon_info.hbmColor) };
    }
    if !icon_info.hbmMask.is_invalid() {
        let _ = unsafe { DeleteObject(icon_info.hbmMask) };
    }

    png
}

#[cfg(windows)]
unsafe fn hbitmap_to_png(bitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Vec<u8>> {
    use std::{ffi::c_void, mem::size_of};
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW, ReleaseDC,
        },
    };

    let mut bitmap_info = BITMAP::default();
    let object_size = size_of::<BITMAP>() as i32;
    let object_result = unsafe {
        GetObjectW(
            bitmap,
            object_size,
            Some((&mut bitmap_info as *mut BITMAP).cast::<c_void>()),
        )
    };
    if object_result != object_size || bitmap_info.bmWidth <= 0 || bitmap_info.bmHeight <= 0 {
        return None;
    }

    let width = bitmap_info.bmWidth as u32;
    let height = bitmap_info.bmHeight as u32;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];

    let mut dib = BITMAPINFO::default();
    dib.bmiHeader.biSize = size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
    dib.bmiHeader.biWidth = bitmap_info.bmWidth;
    dib.bmiHeader.biHeight = -bitmap_info.bmHeight;
    dib.bmiHeader.biPlanes = 1;
    dib.bmiHeader.biBitCount = 32;
    dib.bmiHeader.biCompression = BI_RGB.0;

    let hdc = unsafe { GetDC(HWND::default()) };
    if hdc.is_invalid() {
        return None;
    }

    let lines = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast::<c_void>()),
            &mut dib,
            DIB_RGB_COLORS,
        )
    };
    let _ = unsafe { ReleaseDC(HWND::default(), hdc) };

    if lines == 0 {
        return None;
    }

    let has_alpha = pixels.chunks_exact(4).any(|pixel| pixel[3] != 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if !has_alpha {
            pixel[3] = 255;
        }
    }

    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    image::ImageEncoder::write_image(
        encoder,
        &pixels,
        width,
        height,
        image::ColorType::Rgba8.into(),
    )
    .ok()?;

    Some(png)
}

#[cfg(windows)]
fn platform_reveal(path: &Path) -> Result<(), String> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows::Win32::{
        Foundation::HWND,
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{SHOW_WINDOW_CMD, SW_SHOWNORMAL},
        },
    };
    use windows::core::PCWSTR;

    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("Failed to resolve current directory: {err}"))?
            .join(path)
    };
    let parameters = format!("/select,\"{}\"", path.display());
    let operation = wide_null("open");
    let executable = wide_null("explorer.exe");
    let parameters = OsStr::new(&parameters)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();

    let result = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            PCWSTR::null(),
            SHOW_WINDOW_CMD(SW_SHOWNORMAL.0),
        )
    };

    if (result.0 as isize) <= 32 {
        return Err(format!("Explorer failed with code {}", result.0 as isize));
    }

    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "macos")]
fn platform_reveal(path: &Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Failed to open Finder: {err}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_reveal(path: &Path) -> Result<(), String> {
    let directory = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    std::process::Command::new("xdg-open")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Failed to open file manager: {err}"))
}

#[cfg(not(any(windows, unix)))]
fn platform_reveal(_path: &Path) -> Result<(), String> {
    Err("Opening a file manager is not supported on this platform.".to_string())
}
