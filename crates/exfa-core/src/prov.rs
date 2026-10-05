//! docs/22: version / provenance, SDE pack override checks (`--sde`, RPC `sde_override`), JCS canonical JSON and
//! SHA-256 for price snapshot hashes.
use crate::data as d;
use crate::j::J;
use serde_json::{json, Value};

pub fn sha256(data: &[u8]) -> String {
    sha256_hex(data)
}

/// SHA-256 as lowercase hex (also used by exfa-core at runtime for pack / snapshot hashes).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            v = [t1.wrapping_add(s0.wrapping_add(maj)), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

// ------------------------------------------------------------------ JCS (RFC 8785)
/// ECMAScript Number::toString for a finite double (RFC 8785 §3.2.2.3).
pub fn es_number(f: f64) -> String {
    if f == 0.0 {
        return "0".into();
    }
    if f < 0.0 {
        return format!("-{}", es_number(-f));
    }
    // shortest round-trip digits and exponent from Rust's `{:e}` (e.g. "5.2374e3")
    let e_fmt = format!("{f:e}");
    let (mant, exp) = e_fmt.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { "-" } else { "+" };
        if k == 1 { format!("{digits}e{sign}{}", e.abs()) } else { format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs()) }
    }
}

/// RFC 8785 canonical JSON.
pub fn jcs(v: &Value) -> String {
    let mut out = String::new();
    jcs_into(v, &mut out);
    out
}

fn jcs_into(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64().filter(|i| i.unsigned_abs() < (1u64 << 53)) {
                out.push_str(&i.to_string());
            } else {
                out.push_str(&es_number(n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap()),
        Value::Array(a) => {
            out.push('[');
            for (k, x) in a.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                jcs_into(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (k, key) in keys.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap());
                out.push(':');
                jcs_into(&m[*key], out);
            }
            out.push('}');
        }
    }
}

/// docs/22 §4.6 content hash of a snapshot object (without its `content_hash` key).
pub fn snapshot_hash(v: &Value) -> String {
    let mut o = v.clone();
    if let Value::Object(m) = &mut o {
        m.remove("content_hash");
    }
    format!("sha256:{}", sha256(jcs(&o).as_bytes()))
}

// ------------------------------------------------------------------ SDE identity, version, provenance
pub fn sde_hash() -> String {
    // interim (docs/22 §2.3): no embedded edp pack yet -> hash of the source dataset JSON, flagged by sde_hash_of
    format!("sha256:{}", d::DATASET_SHA256)
}

pub fn target() -> &'static str {
    if cfg!(target_os = "wasi") {
        "wasm32-wasip1"
    } else if cfg!(target_arch = "wasm32") {
        "wasm32-unknown-unknown"
    } else {
        "native"
    }
}

/// Process / session price state for version / provenance: (price_source, snapshot_id, time, hash).
pub fn price_state(use_market: bool, request_table: bool) -> (&'static str, Value, Value, Value) {
    if request_table {
        return ("request", Value::Null, Value::Null, Value::Null);
    }
    if !use_market {
        return ("none", Value::Null, Value::Null, Value::Null);
    }
    match crate::price::market() {
        Some(m) => ("file", opt(&m.id), opt(&m.time), opt(&m.hash)),
        None => {
            use crate::price::{EMBEDDED_HASH, EMBEDDED_ID, EMBEDDED_TIME};
            ("snapshot", json!(EMBEDDED_ID), json!(EMBEDDED_TIME), json!(EMBEDDED_HASH))
        }
    }
}

fn opt(s: &Option<String>) -> Value {
    s.as_ref().map(|x| json!(x)).unwrap_or(Value::Null)
}

pub fn provenance_value(use_market: bool, request_table: bool) -> Value {
    let (src, id, time, hash) = price_state(use_market, request_table);
    json!({
        "engine": concat!("exfa-engine ", env!("CARGO_PKG_VERSION")),
        "sde_build": d::SDE_BUILD, "sde_revision": d::DATASET_REVISION, "sde_release": d::SDE_RELEASE_DATE,
        "sde_hash": sde_hash(), "sde_source": "embedded",
        "price_source": src, "snapshot_time": time, "price_snapshot_id": id, "price_hash": hash,
    })
}

pub fn provenance_j(use_market: bool, request_table: bool) -> J {
    value_to_j(&provenance_value(use_market, request_table))
}

pub fn value_to_j(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                J::U(u)
            } else if let Some(i) = n.as_i64() {
                J::I(i)
            } else {
                J::F(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => J::Str(s.clone()),
        Value::Array(a) => J::A(a.iter().map(value_to_j).collect()),
        Value::Object(m) => J::O(m.iter().map(|(k, x)| (std::borrow::Cow::Owned(k.clone()), value_to_j(x))).collect()),
    }
}

/// `exfa version` / RPC `version`.
pub fn version() -> Value {
    let mut v = provenance_value(true, false);
    let m = v.as_object_mut().unwrap();
    m.insert("pack_format".into(), json!("1.0"));
    m.insert("snapshot_schema_version".into(), json!(1));
    m.insert("target".into(), json!(target()));
    m.insert("sde_hash_of".into(), json!("dataset"));
    v
}

// ------------------------------------------------------------------ --sde / sde_override (docs/22 §2.4)
pub fn sde_error(reason: &str, message: impl Into<String>) -> Value {
    json!({"error": {"code": "SDE_LOAD_FAILED", "reason": reason, "message": message.into()}})
}

/// Check an edp pack; this build cannot run a different pack yet (no runtime interpreter, docs/22 §2.1), so a
/// structurally valid pack is refused with `incompatible_version` and the session keeps the embedded data.
pub fn check_pack(b: &[u8]) -> Result<(), Value> {
    if b.len() < 64 {
        return Err(sde_error("corrupt", "pack shorter than the 64-byte header"));
    }
    if &b[0..4] != b"EDPK" {
        return Err(sde_error("corrupt", "bad magic (want EDPK)"));
    }
    let major = u16::from_le_bytes([b[4], b[5]]);
    if major != 1 {
        return Err(sde_error("incompatible_version", format!("pack format_major {major}, engine reads 1")));
    }
    let n = u16::from_le_bytes([b[14], b[15]]) as usize;
    if 64 + 24 * n > b.len() {
        return Err(sde_error("corrupt", "section directory truncated"));
    }
    for i in 0..n {
        let o = 64 + 24 * i;
        let off = u64::from_le_bytes(b[o + 8..o + 16].try_into().unwrap());
        let len = u64::from_le_bytes(b[o + 16..o + 24].try_into().unwrap());
        if off.checked_add(len).map(|e| e > b.len() as u64).unwrap_or(true) {
            return Err(sde_error("corrupt", format!("section {i} out of range")));
        }
    }
    let want: String = b[24..56].iter().map(|x| format!("{x:02x}")).collect();
    if sha256(&b[64..]) != want {
        return Err(sde_error("hash_mismatch", "content_sha256 does not match bytes 64..EOF"));
    }
    if format!("sha256:{want}") == sde_hash() {
        return Ok(());
    }
    Err(sde_error("incompatible_version", "runtime SDE override is not available in this build yet (docs/22 §2.1 pack interpreter pending); embedded data stays in use"))
}

pub fn load_pack_path(path: &str) -> Result<(), Value> {
    let b = std::fs::read(path).map_err(|e| sde_error("not_found", format!("{path}: {e}")))?;
    check_pack(&b)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// RPC `sde_override`.
pub fn sde_override(p: &Value) -> Value {
    if p.get("reset").and_then(|r| r.as_bool()) == Some(true) {
        return version();
    }
    let r = if let Some(path) = p.get("path").and_then(|x| x.as_str()) {
        load_pack_path(path)
    } else if let Some(b) = p.get("pack_b64").and_then(|x| x.as_str()) {
        match b64_decode(b) {
            Some(bytes) => check_pack(&bytes),
            None => Err(sde_error("corrupt", "pack_b64 is not valid base64")),
        }
    } else {
        Err(sde_error("not_found", "sde_override needs path, pack_b64 or reset"))
    };
    match r {
        Ok(()) => version(),
        Err(e) => e,
    }
}
