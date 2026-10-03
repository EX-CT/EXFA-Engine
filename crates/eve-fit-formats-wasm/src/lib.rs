//! C-ABI exports of `eve-fit-formats` for wasm32-unknown-unknown (no wasm-bindgen needed). Separate module from the
//! engine's `eve-wasm`, so a frontend can load the formats without the engine (and vice versa).
//!
//! JS: `const p = alloc(n)`; write UTF-8 request bytes at p; `const r = rpc(p, n)` (BigInt: ptr<<32 | len);
//! read the response; `dealloc(ptr, len)` both buffers.
//! Methods: eft_parse, eft_export, format_import, format_export (shipstats needs `params.stats`, see
//! `eve_fit_formats::format_export`).
use std::alloc::{alloc as raw_alloc, dealloc as raw_dealloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    unsafe { raw_alloc(Layout::from_size_align(len.max(1), 1).unwrap()) }
}

/// # Safety
/// `ptr`/`len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    raw_dealloc(ptr, Layout::from_size_align(len.max(1), 1).unwrap())
}

fn out(s: String) -> u64 {
    let b = s.into_bytes();
    let p = alloc(b.len());
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
    ((p as u64) << 32) | b.len() as u64
}

/// JSONL RPC line (ptr,len) -> response JSON `{"id","result"}`.
///
/// # Safety
/// `ptr`/`len` must describe a readable buffer.
#[no_mangle]
pub unsafe extern "C" fn rpc(ptr: *const u8, len: usize) -> u64 {
    let s = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("");
    out(serde_json::to_string(&eve_fit_formats::rpc(s)).unwrap())
}
