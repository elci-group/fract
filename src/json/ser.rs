//! Serde `Serializer` that produces a [`Value`].

use super::value::Value;
use serde::ser::{self, Serialize, Serializer};

/// Convert any serde-serializable value into a [`Value`].
///
/// # Panics
/// Panics if the value's `Serialize` implementation fails (the built-in
/// serializer itself cannot fail, but a custom `Serialize` impl may error).
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
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, SerError> {
        Ok(Value::Number(v as f64))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, SerError> {
        Ok(Value::Number(f64::from(v)))
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
            v.iter().map(|&b| Value::Number(f64::from(b))).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    // Explicit import: the `Serialize` *derive* lives at `serde::Serialize`,
    // while `super::*` only re-exports the `serde::ser::Serialize` trait.
    use serde::Serialize;

    #[test]
    fn integers_of_all_widths_become_numbers() {
        assert_eq!(to_value(5i8), Value::Number(5.0));
        assert_eq!(to_value(5i16), Value::Number(5.0));
        assert_eq!(to_value(5i32), Value::Number(5.0));
        assert_eq!(to_value(5i64), Value::Number(5.0));
        assert_eq!(to_value(5i128), Value::Number(5.0));
        assert_eq!(to_value(5u8), Value::Number(5.0));
        assert_eq!(to_value(5u16), Value::Number(5.0));
        assert_eq!(to_value(5u32), Value::Number(5.0));
        assert_eq!(to_value(5u64), Value::Number(5.0));
        assert_eq!(to_value(5u128), Value::Number(5.0));
    }

    #[test]
    fn floats_become_numbers() {
        assert_eq!(to_value(0.5f32), Value::Number(0.5));
        assert_eq!(to_value(-1.25f64), Value::Number(-1.25));
        assert_eq!(to_value(1.5f64).to_string(), "1.5");
    }

    #[test]
    fn char_and_str_become_strings() {
        assert_eq!(to_value('x'), Value::String("x".to_string()));
        assert_eq!(to_value("hello"), Value::String("hello".to_string()));
    }

    #[test]
    fn bytes_become_number_array() {
        struct Raw(&'static [u8]);
        impl Serialize for Raw {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }
        assert_eq!(
            to_value(Raw(&[1, 2, 255])),
            Value::Array(vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(255.0),
            ])
        );
    }

    #[test]
    fn unit_like_values_become_null() {
        #[derive(Serialize)]
        struct Marker;
        assert_eq!(to_value(()), Value::Null);
        assert_eq!(to_value(Marker), Value::Null);
    }

    #[test]
    fn unit_variant_becomes_variant_name_string() {
        #[derive(Serialize)]
        enum Kind {
            Alpha,
            Beta,
        }
        assert_eq!(to_value(Kind::Alpha), Value::String("Alpha".to_string()));
        assert_eq!(to_value(Kind::Beta).to_string(), "\"Beta\"");
    }

    #[test]
    fn newtype_struct_is_transparent() {
        #[derive(Serialize)]
        struct Wrapper(u32);
        assert_eq!(to_value(Wrapper(7)), Value::Number(7.0));
    }

    #[test]
    fn newtype_variant_wraps_value_in_named_object() {
        #[derive(Serialize)]
        enum Kind {
            One(u32),
        }
        let mut expected = Value::object();
        expected.insert("One", Value::Number(3.0));
        assert_eq!(to_value(Kind::One(3)), expected);
    }

    #[test]
    fn tuple_and_tuple_struct_become_arrays() {
        #[derive(Serialize)]
        struct Pair(u32, u32);
        assert_eq!(
            to_value(Pair(1, 2)),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
        );
        assert_eq!(
            to_value((1u32, "two")),
            Value::Array(vec![Value::Number(1.0), Value::String("two".to_string()),])
        );
    }

    #[test]
    fn tuple_variant_becomes_named_array() {
        #[derive(Serialize)]
        enum Kind {
            Pair(u32, u32),
        }
        let mut expected = Value::object();
        expected.insert(
            "Pair",
            Value::Array(vec![Value::Number(4.0), Value::Number(5.0)]),
        );
        assert_eq!(to_value(Kind::Pair(4, 5)), expected);
    }

    #[test]
    fn struct_variant_becomes_named_object() {
        #[derive(Serialize)]
        enum Kind {
            Point { x: u32, y: u32 },
        }
        let mut inner = Value::object();
        inner.insert("x", Value::Number(1.0));
        inner.insert("y", Value::Number(2.0));
        let mut expected = Value::object();
        expected.insert("Point", inner);
        assert_eq!(to_value(Kind::Point { x: 1, y: 2 }), expected);
    }

    #[test]
    fn struct_fields_preserve_declaration_order() {
        #[derive(Serialize)]
        struct Record {
            first: u32,
            second: bool,
        }
        assert_eq!(
            to_value(Record {
                first: 1,
                second: true,
            })
            .to_string(),
            "{\"first\":1,\"second\":true}"
        );
    }

    #[test]
    fn nested_structs_and_sequences_render_as_expected() {
        #[derive(Serialize)]
        struct Inner {
            name: String,
        }
        #[derive(Serialize)]
        struct Outer {
            inner: Inner,
            items: Vec<u32>,
            maybe: Option<u32>,
        }
        let value = to_value(Outer {
            inner: Inner {
                name: "a".to_string(),
            },
            items: vec![1, 2],
            maybe: None,
        });
        assert_eq!(
            value.to_string(),
            "{\"inner\":{\"name\":\"a\"},\"items\":[1,2],\"maybe\":null}"
        );
    }

    #[test]
    fn map_with_string_keys_becomes_object() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("a".to_string(), 1u32);
        map.insert("b".to_string(), 2u32);
        assert_eq!(to_value(map).to_string(), "{\"a\":1,\"b\":2}");
    }

    #[test]
    fn map_with_non_string_keys_is_an_error() {
        let mut map = std::collections::HashMap::new();
        map.insert(1u32, 2u32);
        // `to_value` would panic on this; call the serializer directly to
        // observe the error instead.
        let result = map.serialize(ValueSerializer);
        assert!(result.is_err());
    }

    #[test]
    fn escapes_and_number_formatting_hold_through_to_value() {
        let value = to_value("quote:\" newline:\n tab:\t");
        assert_eq!(value.to_string(), "\"quote:\\\" newline:\\n tab:\\t\"");
        assert_eq!(to_value(42i64).to_string(), "42");
        assert_eq!(to_value(-7i32).to_string(), "-7");
    }
}
