//! JSON value type, its writer, and the axum response wrapper.

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

/// A minimal JSON value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Serialize the value to a compact JSON string.
    #[allow(clippy::inherent_to_string_shadow_display)]
    #[must_use]
    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// Construct an empty JSON object.
    #[must_use]
    pub fn object() -> Self {
        Value::Object(Vec::new())
    }

    /// Insert a key/value pair into an object.
    ///
    /// # Panics
    /// Panics if called on a non-object value.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        match self {
            Value::Object(entries) => entries.push((key.into(), value.into())),
            _ => panic!("insert called on a non-object value"),
        }
    }

    /// Look up a key in an object value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Borrow the inner string, if this value is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// Borrow the inner array, if this value is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Return the inner number, if this value is a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Return the inner boolean, if this value is a boolean.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => write_number(*n, out),
            Value::String(s) => write_string(s, out),
            Value::Array(arr) => {
                out.push('[');
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Object(obj) => {
                out.push('{');
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

fn write_number(n: f64, out: &mut String) {
    use std::fmt::Write as _;
    if n.is_nan() || n.is_infinite() {
        out.push_str("null");
        return;
    }
    // Exact integer-valuedness test: comparing against the truncation is the
    // semantics we want, so an epsilon comparison would be wrong here.
    #[allow(clippy::float_cmp)]
    let is_integral = n == n.trunc();
    if is_integral && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        let _ = write!(out, "{n:.0}");
    } else {
        let _ = write!(out, "{n}");
    }
}

/// Borrow the entries of an object value.
pub(crate) fn as_object(v: &Value) -> Option<&[(String, Value)]> {
    match v {
        Value::Object(entries) => Some(entries),
        _ => None,
    }
}

/// Borrow the items of an array value.
pub(crate) fn as_array(v: &Value) -> Option<&[Value]> {
    match v {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

/// Borrow the inner string of a string value.
pub(crate) fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) => Some(s),
        _ => None,
    }
}

/// Look up a key in object entries.
pub(crate) fn get<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Look up a key in object entries and borrow it as a string.
pub(crate) fn get_str<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    get(obj, key).and_then(as_str)
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

macro_rules! impl_from_int {
    ($($t:ty),*) => {
        $(
            impl From<$t> for Value {
                fn from(n: $t) -> Self {
                    Value::Number(n as f64)
                }
            }
        )*
    };
}

// Integers up to 32 bits convert to f64 without loss.
macro_rules! impl_from_int_lossless {
    ($($t:ty),*) => {
        $(
            impl From<$t> for Value {
                fn from(n: $t) -> Self {
                    Value::Number(f64::from(n))
                }
            }
        )*
    };
}

impl_from_int!(i64, i128, isize, u64, u128, usize);
impl_from_int_lossless!(i8, i16, i32, u8, u16, u32);

impl From<f32> for Value {
    fn from(n: f32) -> Self {
        Value::Number(f64::from(n))
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_owned())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => Value::Null,
        }
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(vec: Vec<T>) -> Self {
        Value::Array(vec.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>, const N: usize> From<[T; N]> for Value {
    fn from(arr: [T; N]) -> Self {
        Value::Array(arr.into_iter().map(Into::into).collect())
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Number(n) => serializer.serialize_f64(*n),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for v in arr {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            Value::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len()))?;
                for (k, v) in obj {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl IntoResponse for Value {
    fn into_response(self) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(self.to_string()))
            .unwrap()
    }
}

/// Thin wrapper around [`Value`] for axum responses.
#[derive(Debug, Clone)]
pub struct Json(pub Value);

impl IntoResponse for Json {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

impl From<Value> for Json {
    fn from(value: Value) -> Self {
        Json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_match_their_variant() {
        let s = Value::String("x".into());
        assert_eq!(s.as_str(), Some("x"));
        assert_eq!(s.as_f64(), None);
        assert_eq!(s.as_bool(), None);
        assert_eq!(s.as_array(), None);

        let n = Value::Number(2.5);
        assert_eq!(n.as_f64(), Some(2.5));
        assert_eq!(n.as_str(), None);

        let b = Value::Bool(true);
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_f64(), None);

        let a = Value::Array(vec![Value::Null]);
        assert_eq!(a.as_array(), Some(&[Value::Null][..]));
        assert_eq!(a.as_str(), None);

        assert_eq!(Value::Null.as_str(), None);
        assert_eq!(Value::Null.as_bool(), None);
    }

    #[test]
    fn get_finds_object_keys() {
        let mut o = Value::object();
        o.insert("a", 1);
        assert_eq!(o.get("a"), Some(&Value::Number(1.0)));
        assert_eq!(o.get("missing"), None);
        assert_eq!(Value::Null.get("a"), None);
    }

    #[test]
    #[should_panic(expected = "insert called on a non-object value")]
    fn insert_on_non_object_panics() {
        let mut v = Value::Null;
        v.insert("a", 1);
    }

    #[test]
    fn crate_helpers_mirror_inherent_accessors() {
        let mut o = Value::object();
        o.insert("name", "fract");
        o.insert("n", 3);
        let entries = as_object(&o).unwrap();
        assert_eq!(as_str(o.get("name").unwrap()), Some("fract"));
        assert_eq!(get(entries, "n"), Some(&Value::Number(3.0)));
        assert_eq!(get_str(entries, "name"), Some("fract"));
        assert_eq!(get_str(entries, "n"), None);
        assert!(as_object(&Value::Null).is_none());
        assert!(as_array(&Value::Null).is_none());
        assert!(as_str(&Value::Null).is_none());
        let arr = Value::Array(vec![Value::Bool(true)]);
        assert_eq!(as_array(&arr).unwrap().len(), 1);
    }

    #[test]
    fn write_escapes_formfeed_and_carriage_return() {
        let v = Value::String("a\u{000C}b\rc".into());
        assert_eq!(v.to_string(), "\"a\\fb\\rc\"");
    }

    #[test]
    fn from_impls_cover_all_scalar_types() {
        assert_eq!(Value::from(5i64), Value::Number(5.0));
        assert_eq!(Value::from(5i128), Value::Number(5.0));
        assert_eq!(Value::from(5isize), Value::Number(5.0));
        assert_eq!(Value::from(5u64), Value::Number(5.0));
        assert_eq!(Value::from(5u128), Value::Number(5.0));
        assert_eq!(Value::from(5usize), Value::Number(5.0));
        assert_eq!(Value::from(5i8), Value::Number(5.0));
        assert_eq!(Value::from(5i16), Value::Number(5.0));
        assert_eq!(Value::from(5i32), Value::Number(5.0));
        assert_eq!(Value::from(5u8), Value::Number(5.0));
        assert_eq!(Value::from(5u16), Value::Number(5.0));
        assert_eq!(Value::from(5u32), Value::Number(5.0));
        assert_eq!(Value::from(0.5f32), Value::Number(0.5));
        assert_eq!(Value::from(0.5f64), Value::Number(0.5));
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from("s".to_string()), Value::String("s".into()));
        assert_eq!(Value::from("s"), Value::String("s".into()));
        assert_eq!(
            Value::from(vec![1i32, 2]),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
        );
        assert_eq!(
            Value::from([true, false]),
            Value::Array(vec![Value::Bool(true), Value::Bool(false)])
        );
    }

    #[test]
    fn serializing_a_value_round_trips_through_to_value() {
        let mut o = Value::object();
        o.insert("arr", Value::Array(vec![Value::Number(1.0), Value::Null]));
        o.insert("s", "x");
        o.insert("b", true);
        o.insert("n", Value::Number(2.5));
        o.insert("null", Value::Null);
        assert_eq!(crate::json::to_value(o.clone()), o);
    }

    #[test]
    fn json_wrapper_from_value() {
        let j = Json::from(Value::Bool(true));
        assert_eq!(j.0, Value::Bool(true));
    }
}
