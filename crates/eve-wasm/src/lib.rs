//! Minimal C-ABI exports for wasm32-unknown-unknown (no wasm-bindgen needed).
//!
//! JS: `const p = alloc(n)`; write UTF-8 request bytes at p; `const r = calc(p, n)` (BigInt: ptr<<32 | len);
//! read the response; `dealloc(ptr, len)` both buffers. `rpc` also serves `optimize` (eve-optimizer).
use std::alloc::{alloc as raw_alloc, dealloc as raw_dealloc, Layout};

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    unsafe { raw_alloc(Layout::from_size_align(len.max(1), 1).unwrap()) }
}

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

/// FitRequest JSON (ptr,len) -> FitStats JSON packed as (ptr << 32 | len).
#[no_mangle]
pub unsafe extern "C" fn calc(ptr: *const u8, len: usize) -> u64 {
    let s = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("");
    out(eve_dogma::calc_json(s))
}

/// JSONL RPC line (ptr,len) -> response JSON.
#[no_mangle]
pub unsafe extern "C" fn rpc(ptr: *const u8, len: usize) -> u64 {
    let s = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("");
    // optimize lives in eve-optimizer (on top of the engine); everything else is the engine's rpc
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        if v.get("method").and_then(|m| m.as_str()) == Some("optimize") {
            let p = v.get("params").cloned().unwrap_or(serde_json::Value::Null);
            let r = serde_json::json!({"id": v.get("id").cloned().unwrap_or(serde_json::Value::Null), "result": eve_optimizer::optimize_value(&p)});
            return out(serde_json::to_string(&r).unwrap());
        }
    }
    out(serde_json::to_string(&eve_dogma::rpc(s)).unwrap())
}
