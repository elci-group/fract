//! Lightweight `serde_json` replacement for simple dashboard payloads.

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::ser::{
    self, Serialize, SerializeMap as SerializeMapTrait, SerializeSeq as SerializeSeqTrait,
    Serializer,
};

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
    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// Construct an empty JSON object.
    pub fn object() -> Self {
        Value::Object(Vec::new())
    }

    /// Insert a key/value pair into an object. Panics if called on a non-object.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        match self {
            Value::Object(entries) => entries.push((key.into(), value.into())),
            _ => panic!("insert called on a non-object value"),
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
    if n.is_nan() || n.is_infinite() {
        out.push_str("null");
        return;
    }
    if n == n.trunc() && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        out.push_str(&format!("{:.0}", n));
    } else {
        out.push_str(&format!("{}", n));
    }
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
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

impl_from_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl From<f32> for Value {
    fn from(n: f32) -> Self {
        Value::Number(n as f64)
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

/// Convert any serde-serializable value into a [`Value`].
pub fn to_value<T: Serialize>(value: T) -> Value {
    value
        .serialize(ValueSerializer)
        .expect("failed to serialize value")
}

#[derive(Debug)]
struct SerError(String);

impl std::fmt::Display for SerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SerError {}

impl ser::Error for SerError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        SerError(msg.to_string())
    }
}

struct ValueSerializer;

impl Serializer for ValueSerializer {
    type Ok = Value;
    type Error = SerError;
    type SerializeSeq = ValueSeq;
    type SerializeTuple = ValueSeq;
    type SerializeTupleStruct = ValueSeq;
    type SerializeTupleVariant = ValueSeq;
    type SerializeMap = ValueMap;
    type SerializeStruct = ValueMap;
    type SerializeStructVariant = ValueMap;

    fn serialize_bool(self, v: bool) -> Result<Value, SerError> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, SerError> {
        Ok(Value::Number(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, SerError> {
        Ok(Value::String(v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Value, SerError> {
        Ok(Value::String(v.to_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, SerError> {
        Ok(Value::Array(
            v.iter().map(|&b| Value::Number(b as f64)).collect(),
        ))
    }

    fn serialize_none(self) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value, SerError> {
        Ok(to_value(value))
    }

    fn serialize_unit(self) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, SerError> {
        Ok(Value::String(variant.to_owned()))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, SerError> {
        Ok(to_value(value))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, SerError> {
        let mut obj = Value::object();
        obj.insert(variant, to_value(value));
        Ok(obj)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<ValueSeq, SerError> {
        Ok(ValueSeq {
            vec: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<ValueSeq, SerError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<ValueSeq, SerError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<ValueSeq, SerError> {
        Ok(ValueSeq {
            vec: Vec::with_capacity(len),
            variant: Some(variant),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<ValueMap, SerError> {
        Ok(ValueMap {
            map: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
            variant: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<ValueMap, SerError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<ValueMap, SerError> {
        Ok(ValueMap {
            map: Vec::with_capacity(len),
            key: None,
            variant: Some(variant),
        })
    }
}

struct ValueSeq {
    vec: Vec<Value>,
    variant: Option<&'static str>,
}

impl ser::SerializeSeq for ValueSeq {
    type Ok = Value;
    type Error = SerError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.vec.push(to_value(value));
        Ok(())
    }

    fn end(self) -> Result<Value, SerError> {
        match self.variant {
            Some(variant) => {
                let mut obj = Value::object();
                obj.insert(variant, Value::Array(self.vec));
                Ok(obj)
            }
            None => Ok(Value::Array(self.vec)),
        }
    }
}

impl ser::SerializeTuple for ValueSeq {
    type Ok = Value;
    type Error = SerError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, SerError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for ValueSeq {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, SerError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for ValueSeq {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, SerError> {
        ser::SerializeSeq::end(self)
    }
}

struct ValueMap {
    map: Vec<(String, Value)>,
    key: Option<String>,
    variant: Option<&'static str>,
}

impl ser::SerializeMap for ValueMap {
    type Ok = Value;
    type Error = SerError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), SerError> {
        match to_value(key) {
            Value::String(s) => {
                self.key = Some(s);
                Ok(())
            }
            _ => Err(SerError("JSON map keys must be strings".into())),
        }
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        let k = self
            .key
            .take()
            .ok_or_else(|| SerError("missing map key".into()))?;
        self.map.push((k, to_value(value)));
        Ok(())
    }

    fn end(self) -> Result<Value, SerError> {
        match self.variant {
            Some(variant) => {
                let mut obj = Value::object();
                obj.insert(variant, Value::Object(self.map));
                Ok(obj)
            }
            None => Ok(Value::Object(self.map)),
        }
    }
}

impl ser::SerializeStruct for ValueMap {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        self.map.push((key.to_owned(), to_value(value)));
        Ok(())
    }

    fn end(self) -> Result<Value, SerError> {
        ser::SerializeMap::end(self)
    }
}

impl ser::SerializeStructVariant for ValueMap {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        ser::SerializeStruct::serialize_field(self, key, value)
    }

    fn end(self) -> Result<Value, SerError> {
        ser::SerializeMap::end(self)
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

#[macro_export]
#[doc(hidden)]
macro_rules! __json_key {
    ($key:ident) => {
        stringify!($key)
    };
    ($key:literal) => {
        $key
    };
}

#[macro_export]
macro_rules! json {
    (null) => {
        $crate::json::Value::Null
    };
    (true) => {
        $crate::json::Value::Bool(true)
    };
    (false) => {
        $crate::json::Value::Bool(false)
    };
    ([]) => {
        $crate::json::Value::Array(Vec::new())
    };
    ([ $($tt:tt),* $(,)? ]) => {
        $crate::json::Value::Array(vec![ $( $crate::json!($tt) ),* ])
    };
    ({}) => {
        $crate::json::Value::object()
    };
    ({ $($key:tt : $value:tt),* $(,)? }) => {{
        let mut obj = $crate::json::Value::object();
        $( obj.insert($crate::__json_key!($key), $crate::json!($value)); )*
        obj
    }};
    ({ $($key:tt : $value:expr),* $(,)? }) => {{
        let mut obj = $crate::json::Value::object();
        $( obj.insert($crate::__json_key!($key), $crate::json!($value)); )*
        obj
    }};
    ($other:expr) => {
        $crate::json::to_value($other)
    };
}

pub use crate::json;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestModule {
        path: String,
        lines: usize,
        score: f64,
    }

    #[derive(Serialize)]
    #[serde(tag = "type", content = "data")]
    enum TestEvent {
        FileSaved,
        GitCommit { sha: String },
    }

    #[test]
    fn null() {
        assert_eq!(json!(null), Value::Null);
        assert_eq!(json!(null).to_string(), "null");
    }

    #[test]
    fn booleans() {
        assert_eq!(json!(true), Value::Bool(true));
        assert_eq!(json!(false), Value::Bool(false));
        assert_eq!(json!(true).to_string(), "true");
        assert_eq!(json!(false).to_string(), "false");
    }

    #[test]
    fn numbers() {
        assert_eq!(json!(42), Value::Number(42.0));
        assert_eq!(json!(-7), Value::Number(-7.0));
        assert_eq!(json!(2.71), Value::Number(2.71));
        assert_eq!(json!(42).to_string(), "42");
        assert_eq!(json!(2.71).to_string(), "2.71");
    }

    #[test]
    fn strings() {
        assert_eq!(json!("hello"), Value::String("hello".into()));
        assert_eq!(json!("hello").to_string(), "\"hello\"");
    }

    #[test]
    fn string_escaping() {
        let s = "line1\nline2\tquote:\"slash:\\backspace:\u{0008}";
        let v = Value::String(s.into());
        let out = v.to_string();
        assert!(out.contains("\\n"));
        assert!(out.contains("\\t"));
        assert!(out.contains("\\\""));
        assert!(out.contains("\\\\"));
        assert!(out.contains("\\b"));
    }

    #[test]
    fn arrays() {
        let v = json!([1, "two", true, null]);
        assert_eq!(
            v,
            Value::Array(vec![
                Value::Number(1.0),
                Value::String("two".into()),
                Value::Bool(true),
                Value::Null,
            ])
        );
        assert_eq!(v.to_string(), "[1,\"two\",true,null]");
    }

    #[test]
    fn empty_array_and_object() {
        assert_eq!(json!([]).to_string(), "[]");
        assert_eq!(json!({}).to_string(), "{}");
    }

    #[test]
    fn objects() {
        let mut expected = Value::object();
        expected.insert("name", "fract");
        expected.insert("count", 3);
        expected.insert("ok", true);
        assert_eq!(json!({"name": "fract", "count": 3, "ok": true}), expected);
        assert_eq!(
            json!({"name": "fract", "count": 3, "ok": true}).to_string(),
            "{\"name\":\"fract\",\"count\":3,\"ok\":true}"
        );
    }

    #[test]
    fn object_identifier_keys() {
        let v = json!({ name: "fract", count: 3 });
        assert_eq!(v.to_string(), "{\"name\":\"fract\",\"count\":3}");
    }

    #[test]
    fn nested() {
        let v = json!({
            "outer": {
                "inner": [1, 2, 3]
            }
        });
        assert_eq!(v.to_string(), "{\"outer\":{\"inner\":[1,2,3]}}");
    }

    #[test]
    fn from_option() {
        let some: Option<i32> = Some(5);
        let none: Option<i32> = None;
        assert_eq!(Value::from(some), Value::Number(5.0));
        assert_eq!(Value::from(none), Value::Null);
    }

    #[test]
    fn from_vec() {
        let v = Value::from(["a", "b"]);
        assert_eq!(
            v,
            Value::Array(vec![Value::String("a".into()), Value::String("b".into()),])
        );
    }

    #[test]
    fn expression_values() {
        let count = 42;
        let label = "items";
        let v = json!({ "count": count, "label": label });
        assert_eq!(v.to_string(), "{\"count\":42,\"label\":\"items\"}");
    }

    #[test]
    #[allow(clippy::useless_vec)]
    fn multi_token_expression_values() {
        let items = vec!["a", "b"];
        let v = json!({ "count": items.len(), "first": items.first().unwrap() });
        assert_eq!(v.to_string(), "{\"count\":2,\"first\":\"a\"}");
    }

    #[test]
    fn mixed_expression_and_literal_values() {
        let count = 7;
        let v = json!({ "count": count, "nested": { "x": 1 } });
        assert_eq!(v.to_string(), "{\"count\":7,\"nested\":{\"x\":1}}");
    }

    #[test]
    fn serialize_derived_struct() {
        let m = TestModule {
            path: "/src/main.rs".into(),
            lines: 120,
            score: 0.95,
        };
        let v = json!({ "module": m });
        assert_eq!(
            v.to_string(),
            "{\"module\":{\"path\":\"/src/main.rs\",\"lines\":120,\"score\":0.95}}"
        );
    }

    #[test]
    fn serialize_vec_of_structs() {
        let modules = vec![
            TestModule {
                path: "a.rs".into(),
                lines: 10,
                score: 0.1,
            },
            TestModule {
                path: "b.rs".into(),
                lines: 20,
                score: 0.2,
            },
        ];
        let v = json!({ "modules": modules });
        assert_eq!(
            v.to_string(),
            "{\"modules\":[{\"path\":\"a.rs\",\"lines\":10,\"score\":0.1},{\"path\":\"b.rs\",\"lines\":20,\"score\":0.2}]}"
        );
    }

    #[test]
    fn serialize_internally_tagged_enum() {
        let e = TestEvent::GitCommit { sha: "abc".into() };
        let v = to_value(e);
        assert_eq!(
            v.to_string(),
            "{\"type\":\"GitCommit\",\"data\":{\"sha\":\"abc\"}}"
        );

        let e = TestEvent::FileSaved;
        let v = to_value(e);
        assert_eq!(v.to_string(), "{\"type\":\"FileSaved\"}");
    }

    #[test]
    fn display_matches_to_string() {
        let v = json!({"a": [1, 2]});
        assert_eq!(format!("{}", v), v.to_string());
    }

    #[test]
    fn special_floats_serialize_as_null() {
        assert_eq!(Value::Number(f64::NAN).to_string(), "null");
        assert_eq!(Value::Number(f64::INFINITY).to_string(), "null");
        assert_eq!(Value::Number(f64::NEG_INFINITY).to_string(), "null");
    }

    #[test]
    fn json_wrapper_roundtrip() {
        let v = json!({"status": "ok"});
        let response = Json(v.clone()).into_response();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = response.into_body();
        // Body is opaque; at least confirm the wrapper builds and responds.
        drop(body);
    }

    #[test]
    fn re_exported_macro_path() {
        let v = crate::json::json!({ "ok": true });
        assert_eq!(v.to_string(), "{\"ok\":true}");
    }
}
