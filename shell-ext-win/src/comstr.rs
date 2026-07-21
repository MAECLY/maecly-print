//! Returning a string to Explorer.
//!
//! `IExplorerCommand::GetTitle`/`GetIcon`/`GetToolTip` return a `PWSTR` that
//! **the shell frees with `CoTaskMemFree`**. A pointer into a Rust `String`
//! would be freed by the wrong allocator (and would already be dangling), so
//! every title goes through [`alloc`].

use windows::Win32::Foundation::E_OUTOFMEMORY;
use windows::Win32::System::Com::CoTaskMemAlloc;
use windows_core::PWSTR;

/// Copy `text` into a null-terminated, `CoTaskMemAlloc`'d UTF-16 buffer and
/// hand ownership to the caller.
pub fn alloc(text: &str) -> windows_core::Result<PWSTR> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len().checked_mul(2).ok_or(E_OUTOFMEMORY)?;

    let buffer = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
    if buffer.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer, wide.len()) };
    Ok(PWSTR(buffer))
}
