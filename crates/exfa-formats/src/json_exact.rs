//! Minimal JSON reader with correctly rounded floats (Rust's `str::parse::<f64>`), for engine stats handed over as
//! text (`params.stats_json`). serde_json's default float parser may be one ulp off, which can flip a display
//! rounding in the ship-stats text; enabling its `float_roundtrip` feature would change the engine's request parsing
//! too (feature unification), so the formats layer reads stats text itself.
use serde_json::{Map, Number, Value};

pub fn parse(s: &str) -> Result<Value, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let v = value(b, &mut i)?;
    ws(b, &mut i);
    if i != b.len() {
        return Err(format!("trailing data at {i}"));
    }
    Ok(v)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<Value, String> {
    ws(b, i);
    let c = *b.get(*i).ok_or("unexpected end")?;
    match c {
        b'{' => {
            *i += 1;
            let mut m = Map::new();
            ws(b, i);
            if b.get(*i) == Some(&b'}') {
                *i += 1;
                return Ok(Value::Object(m));
            }
            loop {
                ws(b, i);
                let k = string(b, i)?;
                ws(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err(format!("expected ':' at {i}"));
                }
                *i += 1;
                let v = value(b, i)?;
                m.insert(k, v);
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {
                        *i += 1;
                        return Ok(Value::Object(m));
                    }
                    _ => return Err(format!("expected ',' or '}}' at {i}")),
                }
            }
        }
        b'[' => {
            *i += 1;
            let mut a = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b']') {
                *i += 1;
                return Ok(Value::Array(a));
            }
            loop {
                a.push(value(b, i)?);
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {
                        *i += 1;
                        return Ok(Value::Array(a));
                    }
                    _ => return Err(format!("expected ',' or ']' at {i}")),
                }
            }
        }
        b'"' => string(b, i).map(Value::String),
        b't' if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(Value::Bool(true))
        }
        b'f' if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(Value::Bool(false))
        }
        b'n' if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(Value::Null)
        }
        _ => {
            let st = *i;
            while *i < b.len() && matches!(b[*i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                *i += 1;
            }
            let t = std::str::from_utf8(&b[st..*i]).map_err(|e| e.to_string())?;
            if t.is_empty() {
                return Err(format!("unexpected byte at {st}"));
            }
            if !t.contains(['.', 'e', 'E']) {
                if let Ok(u) = t.parse::<u64>() {
                    return Ok(Value::Number(u.into()));
                }
                if let Ok(x) = t.parse::<i64>() {
                    return Ok(Value::Number(x.into()));
                }
            }
            let f: f64 = t.parse().map_err(|_| format!("bad number {t}"))?;
            Ok(Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null))
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Result<String, String> {
    // reuse serde_json for the (exact) string unescaping: find the closing quote first
    if b.get(*i) != Some(&b'"') {
        return Err(format!("expected string at {i}"));
    }
    let st = *i;
    *i += 1;
    while *i < b.len() {
        match b[*i] {
            b'\\' => *i += 2,
            b'"' => {
                *i += 1;
                let raw = std::str::from_utf8(&b[st..*i]).map_err(|e| e.to_string())?;
                return serde_json::from_str::<String>(raw).map_err(|e| e.to_string());
            }
            _ => *i += 1,
        }
    }
    Err("unterminated string".into())
}
