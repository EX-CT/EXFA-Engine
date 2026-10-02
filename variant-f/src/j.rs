//! Minimal JSON output tree for the hot path (replaces `serde_json::Value` there): objects are small vectors with
//! `&'static str` keys, no maps are built, and output is written straight to a `String`. Keys are emitted in sorted
//! order (byte-identical to the former BTreeMap output) and floats are rounded to 6 decimals on write.
use std::borrow::Cow;

pub type Key = Cow<'static, str>;

#[derive(Debug, Clone)]
pub enum J {
    Null,
    Bool(bool),
    U(u64),
    I(i64),
    F(f64),
    S(&'static str),
    Str(String),
    A(Vec<J>),
    O(Vec<(Key, J)>),
}

#[inline]
fn round6(v: f64) -> f64 {
    if v.is_finite() { (v * 1e6).round() / 1e6 } else { v }
}

macro_rules! from_u {
    ($($t:ty),*) => { $(impl From<$t> for J { #[inline] fn from(v: $t) -> J { J::U(v as u64) } })* };
}
macro_rules! from_i {
    ($($t:ty),*) => { $(impl From<$t> for J { #[inline] fn from(v: $t) -> J { if v >= 0 { J::U(v as u64) } else { J::I(v as i64) } } })* };
}
from_u!(u8, u16, u32, u64, usize);
from_i!(i8, i16, i32, i64, isize);
impl From<f64> for J {
    #[inline]
    fn from(v: f64) -> J {
        J::F(v)
    }
}
impl From<f32> for J {
    #[inline]
    fn from(v: f32) -> J {
        J::F(v as f64)
    }
}
impl From<bool> for J {
    #[inline]
    fn from(v: bool) -> J {
        J::Bool(v)
    }
}
impl From<&'static str> for J {
    #[inline]
    fn from(v: &'static str) -> J {
        J::S(v)
    }
}
impl From<String> for J {
    #[inline]
    fn from(v: String) -> J {
        J::Str(v)
    }
}
impl<T: Into<J>> From<Option<T>> for J {
    #[inline]
    fn from(v: Option<T>) -> J {
        match v {
            Some(x) => x.into(),
            None => J::Null,
        }
    }
}
impl<T: Into<J>> From<Vec<T>> for J {
    fn from(v: Vec<T>) -> J {
        J::A(v.into_iter().map(Into::into).collect())
    }
}

static NULL: J = J::Null;

impl std::ops::Index<&str> for J {
    type Output = J;
    fn index(&self, k: &str) -> &J {
        match self {
            J::O(o) => o.iter().find(|x| x.0 == k).map(|x| &x.1).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl std::ops::IndexMut<&'static str> for J {
    fn index_mut(&mut self, k: &'static str) -> &mut J {
        if !matches!(self, J::O(_)) {
            *self = J::O(Vec::new());
        }
        let J::O(o) = self else { unreachable!() };
        let pos = match o.iter().position(|x| x.0 == k) {
            Some(p) => p,
            None => {
                o.push((Cow::Borrowed(k), J::Null));
                o.len() - 1
            }
        };
        &mut o[pos].1
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    let b = s.as_bytes();
    let mut start = 0;
    for (i, &c) in b.iter().enumerate() {
        let esc: &str = match c {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x08 => "\\b",
            0x0c => "\\f",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() {
            out.push_str(&format!("\\u{:04x}", c));
        } else {
            out.push_str(esc);
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

impl J {
    pub fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::U(v) => out.push_str(itoa::Buffer::new().format(*v)),
            J::I(v) => out.push_str(itoa::Buffer::new().format(*v)),
            J::F(v) => {
                let v = round6(*v);
                if v.is_finite() {
                    out.push_str(zmij::Buffer::new().format_finite(v));
                } else {
                    out.push_str("null");
                }
            }
            J::S(s) => write_str(out, s),
            J::Str(s) => write_str(out, s),
            J::A(a) => {
                out.push('[');
                for (k, x) in a.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            J::O(o) => {
                // sorted keys; objects are small, so an index sort on the stack is cheap
                let mut idx_small = [0u16; 48];
                let mut idx_big: Vec<u16> = Vec::new();
                let idx: &mut [u16] = if o.len() <= 48 {
                    &mut idx_small[..o.len()]
                } else {
                    idx_big.resize(o.len(), 0);
                    &mut idx_big[..]
                };
                for (k, x) in idx.iter_mut().enumerate() {
                    *x = k as u16;
                }
                idx.sort_unstable_by(|&x, &y| o[x as usize].0.cmp(&o[y as usize].0));
                out.push('{');
                for (n, &k) in idx.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    let (key, v) = &o[k as usize];
                    write_str(out, key);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub fn to_json_string(&self) -> String {
        let mut s = String::with_capacity(8192);
        self.write(&mut s);
        s
    }
}

impl serde::Serialize for J {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // slow path (RPC / tooling): go through the canonical string form
        let v: serde_json::Value = serde_json::from_str(&self.to_json_string()).map_err(serde::ser::Error::custom)?;
        v.serialize(s)
    }
}

#[macro_export]
macro_rules! jv {
    (null) => { $crate::j::J::Null };
    ({ $($tt:tt)* }) => {{
        #[allow(unused_mut)]
        let mut o: Vec<($crate::j::Key, $crate::j::J)> = Vec::new();
        $crate::jv_obj!(o; $($tt)*);
        $crate::j::J::O(o)
    }};
    ([ $($tt:tt)* ]) => {{
        #[allow(unused_mut)]
        let mut a: Vec<$crate::j::J> = Vec::new();
        $crate::jv_arr!(a; $($tt)*);
        $crate::j::J::A(a)
    }};
    ($e:expr) => { $crate::j::J::from($e) };
}

#[macro_export]
macro_rules! jv_obj {
    ($o:ident;) => {};
    ($o:ident; $k:literal : null $(, $($rest:tt)*)?) => {
        $o.push((::std::borrow::Cow::Borrowed($k), $crate::j::J::Null));
        $crate::jv_obj!($o; $($($rest)*)?);
    };
    ($o:ident; $k:literal : { $($v:tt)* } $(, $($rest:tt)*)?) => {
        $o.push((::std::borrow::Cow::Borrowed($k), $crate::jv!({ $($v)* })));
        $crate::jv_obj!($o; $($($rest)*)?);
    };
    ($o:ident; $k:literal : [ $($v:tt)* ] $(, $($rest:tt)*)?) => {
        $o.push((::std::borrow::Cow::Borrowed($k), $crate::jv!([ $($v)* ])));
        $crate::jv_obj!($o; $($($rest)*)?);
    };
    ($o:ident; $k:literal : $v:expr $(, $($rest:tt)*)?) => {
        $o.push((::std::borrow::Cow::Borrowed($k), $crate::j::J::from($v)));
        $crate::jv_obj!($o; $($($rest)*)?);
    };
}

#[macro_export]
macro_rules! jv_arr {
    ($a:ident;) => {};
    ($a:ident; { $($v:tt)* } $(, $($rest:tt)*)?) => {
        $a.push($crate::jv!({ $($v)* }));
        $crate::jv_arr!($a; $($($rest)*)?);
    };
    ($a:ident; $v:expr $(, $($rest:tt)*)?) => {
        $a.push($crate::j::J::from($v));
        $crate::jv_arr!($a; $($($rest)*)?);
    };
}

pub trait PushKv {
    fn push_kv(&mut self, k: Key, v: J);
}
impl PushKv for Vec<(Key, J)> {
    #[inline]
    fn push_kv(&mut self, k: Key, v: J) {
        self.push((k, v));
    }
}

impl From<crate::request::State> for J {
    fn from(s: crate::request::State) -> J {
        use crate::request::State::*;
        J::S(match s {
            Offline => "offline",
            Online => "online",
            Active => "active",
            Overheated => "overheated",
        })
    }
}
impl From<crate::request::Slot> for J {
    fn from(s: crate::request::Slot) -> J {
        use crate::request::Slot::*;
        J::S(match s {
            High => "high",
            Mid => "mid",
            Low => "low",
            Rig => "rig",
            Subsystem => "subsystem",
            Service => "service",
        })
    }
}
