//! Lightweight `serde_json` replacement for simple dashboard payloads.
//!
//! Split into focused submodules: `value` (the `Value` type, writer, and the
//! `Json` response wrapper), `ser` (serde `Serializer` + `to_value`), and
//! `de` (parser). Every public name is re-exported here so the external
//! surface `crate::json::{Value, Json, json!, to_value, parse}` is unchanged.

mod de;
mod ser;
mod value;

pub use de::parse;
pub use ser::to_value;
pub(crate) use value::{as_array, as_object, as_str, get, get_str};
pub use value::{Json, Value};

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
    use axum::{http::header, response::IntoResponse};
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
        assert_eq!(format!("{v}"), v.to_string());
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

    #[test]
    fn parse_roundtrips_scalars() {
        for (input, expected) in [
            ("null", Value::Null),
            ("true", Value::Bool(true)),
            ("false", Value::Bool(false)),
            ("0", Value::Number(0.0)),
            ("-7", Value::Number(-7.0)),
            ("2.71", Value::Number(2.71)),
            ("1.5e10", Value::Number(1.5e10)),
            ("\"hi\"", Value::String("hi".into())),
        ] {
            assert_eq!(parse(input).unwrap(), expected, "input {input}");
        }
    }

    #[test]
    fn parse_roundtrips_nested_structure() {
        let v = json!({
            "name": "fract",
            "counts": [1, 2, 3],
            "nested": { "ok": true, "nothing": null }
        });
        let s = v.to_string();
        let back = parse(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn parse_handles_unicode_and_surrogates() {
        // \u00e9 = é ; surrogate pair for U+1F600 (😀)
        let v = parse("\"caf\\u00e9 \\uD83D\\uDE00\"").unwrap();
        assert_eq!(v, Value::String("café 😀".into()));
    }

    #[test]
    fn parse_decodes_standard_escapes() {
        let v = parse("\"a\\nb\\t\\\\c\\\"\"").unwrap();
        assert_eq!(v, Value::String("a\nb\t\\c\"".into()));
    }

    #[test]
    fn parse_rejects_unescaped_control_char() {
        assert!(parse("\"bad\x01\"").is_err());
    }

    #[test]
    fn parse_rejects_trailing_data() {
        assert!(parse("true false").is_err());
        assert!(parse("[1] x").is_err());
    }

    #[test]
    fn parse_rejects_non_finite_number() {
        assert!(parse("1e9999").is_err());
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{\"a\"}").is_err());
        assert!(parse("nope").is_err());
        assert!(parse("\"\\u12GH\"").is_err());
    }

    #[test]
    fn write_string_escapes_control_chars() {
        let v = Value::String("a\x00b\x1fc".into());
        assert_eq!(v.to_string(), "\"a\\u0000b\\u001fc\"");
    }

    #[test]
    fn large_integer_round_trips_as_number() {
        // 2^53 + 1 is not exactly representable as f64; it rounds to 2^53.
        // The point of the test is that integers beyond i64 still parse as
        // finite f64 instead of being rejected.
        let v = parse("9007199254740993").unwrap();
        assert!(matches!(
            v,
            Value::Number(n) if (n - 9_007_199_254_740_992.0).abs() < 2.0
        ));
    }

    fn lcg_next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state
    }

    fn gen_string(state: &mut u64) -> String {
        const ALPHA: &[char] = &['a', 'b', 'c', ' ', '"', '\\', '/', '\n', '\t', 'é'];
        let len = (lcg_next(state) % 8) as usize;
        let mut s = String::new();
        for _ in 0..len {
            let idx = usize::try_from(lcg_next(state) % ALPHA.len() as u64).unwrap();
            s.push(ALPHA[idx]);
        }
        s
    }

    fn gen_value(state: &mut u64, depth: usize) -> Value {
        let branches = if depth == 0 { 4 } else { 6 };
        match u8::try_from(lcg_next(state) % branches).unwrap() {
            0 => Value::Null,
            1 => Value::Bool(lcg_next(state) & 1 == 1),
            2 => {
                let n = i64::try_from(lcg_next(state) % 2000).unwrap() - 1000;
                Value::Number(n as f64)
            }
            3 => Value::String(gen_string(state)),
            4 => {
                let len = (lcg_next(state) % 4) as usize;
                Value::Array((0..len).map(|_| gen_value(state, depth - 1)).collect())
            }
            _ => {
                let len = (lcg_next(state) % 4) as usize;
                let mut obj = Value::object();
                for i in 0..len {
                    obj.insert(format!("k{i}"), gen_value(state, depth - 1));
                }
                obj
            }
        }
    }

    #[test]
    fn property_generated_values_roundtrip() {
        let mut state: u64 = 0x1234_5678_9abc_def0;
        for _ in 0..500 {
            let v = gen_value(&mut state, 3);
            let s = v.to_string();
            let back = parse(&s).unwrap_or_else(|e| panic!("parse failed for {s}: {e}"));
            assert_eq!(back, v, "roundtrip mismatch for {s}");
        }
    }
}
