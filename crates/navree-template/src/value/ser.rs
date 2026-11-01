//! Serializing Rust values into [`Value`]s.

use serde::ser::{self, Serialize};

use super::{Value, ValueMap};
use crate::error::Error;

/// Serializes into a [`Value`].
pub(crate) struct ValueSerializer;

/// Serializes `value`, failing like the whole value would.
fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
    value.serialize(ValueSerializer)
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = SerializeSeq;
    type SerializeTuple = SerializeSeq;
    type SerializeTupleStruct = SerializeSeq;
    type SerializeTupleVariant = SerializeVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeStruct;
    type SerializeStructVariant = SerializeStructVariant;

    fn serialize_bool(self, v: bool) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_str(self, v: &str) -> Result<Value, Error> {
        Ok(Value::from(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, Error> {
        Ok(Value::from(String::from_utf8_lossy(v)))
    }

    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::NONE)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
        to_value(value)
    }

    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::NONE)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, Error> {
        Ok(Value::NONE)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        Ok(Value::from(variant))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        to_value(value)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        let mut map = ValueMap::new();
        map.insert(Value::from(variant), to_value(value)?);
        Value::map(map).check_depth()
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<SerializeSeq, Error> {
        Ok(SerializeSeq {
            items: Vec::new(),
            tuple: false,
        })
    }

    fn serialize_tuple(self, _len: usize) -> Result<SerializeSeq, Error> {
        Ok(SerializeSeq {
            items: Vec::new(),
            tuple: true,
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<SerializeSeq, Error> {
        Ok(SerializeSeq {
            items: Vec::new(),
            tuple: false,
        })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<SerializeVariant, Error> {
        Ok(SerializeVariant {
            variant,
            items: Vec::new(),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<SerializeMap, Error> {
        Ok(SerializeMap {
            map: ValueMap::new(),
            key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<SerializeStruct, Error> {
        Ok(SerializeStruct { fields: Vec::new() })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<SerializeStructVariant, Error> {
        Ok(SerializeStructVariant {
            variant,
            map: ValueMap::new(),
        })
    }
}

pub(crate) struct SerializeSeq {
    items: Vec<Value>,
    tuple: bool,
}

impl SerializeSeq {
    fn finish(self) -> Result<Value, Error> {
        if self.tuple {
            Value::tuple(self.items)
        } else {
            Value::list(self.items)
        }
        .check_depth()
    }
}

impl ser::SerializeSeq for SerializeSeq {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}

impl ser::SerializeTuple for SerializeSeq {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for SerializeSeq {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}

pub(crate) struct SerializeVariant {
    variant: &'static str,
    items: Vec<Value>,
}

impl ser::SerializeTupleVariant for SerializeVariant {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        let mut map = ValueMap::new();
        map.insert(Value::from(self.variant), Value::list(self.items));
        Value::map(map).check_depth()
    }
}

pub(crate) struct SerializeMap {
    map: ValueMap,
    key: Option<Value>,
}

impl ser::SerializeMap for SerializeMap {
    type Ok = Value;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(to_value(key)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("a map value came without its key"))?;
        self.map.insert(key, to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Value::map(self.map).check_depth()
    }
}

pub(crate) struct SerializeStruct {
    fields: Vec<(Value, Value)>,
}

impl ser::SerializeStruct for SerializeStruct {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.fields.push((Value::from(key), to_value(value)?));
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        Value::map(ValueMap::Fields(self.fields)).check_depth()
    }
}

pub(crate) struct SerializeStructVariant {
    variant: &'static str,
    map: ValueMap,
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.map.insert(Value::from(key), to_value(value)?);
        Ok(())
    }

    fn end(self) -> Result<Value, Error> {
        let mut outer = ValueMap::new();
        outer.insert(Value::from(self.variant), Value::map(self.map));
        Value::map(outer).check_depth()
    }
}
