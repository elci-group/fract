//! Serde `Serializer` that produces a [`Value`].

use super::value::Value;
use serde::ser::{self, Serialize, Serializer};

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
