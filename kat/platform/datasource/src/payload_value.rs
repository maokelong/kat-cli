use std::{fmt, str};

use anyhow::{Context, Result};
use serde::{
    Serialize,
    ser::{
        self, Impossible, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
        SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    },
};

#[derive(Clone, Debug)]
pub(crate) enum PayloadValue {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<PayloadValue>),
    Object(Vec<PayloadField>),
}

#[derive(Clone, Debug)]
pub(crate) struct PayloadField {
    name: PayloadFieldName,
    value: PayloadValue,
}

#[derive(Clone, Debug)]
enum PayloadFieldName {
    Static(&'static str),
    Owned(String),
}

pub(crate) fn to_payload_value<T>(value: &T) -> Result<PayloadValue>
where
    T: Serialize,
{
    value
        .serialize(PayloadValueSerializer)
        .context("failed to serialize decoded payload into value tree")
}

impl PayloadValue {
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub(crate) fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub(crate) fn as_object(&self) -> Option<&[PayloadField]> {
        match self {
            Self::Object(fields) => Some(fields),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[PayloadValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            Self::U64(value) => i64::try_from(*value).ok(),
            _ => None,
        }
    }

    pub(crate) fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(value) => Some(*value),
            Self::I64(value) => u64::try_from(*value).ok(),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Self::Binary(value) => Some(value),
            _ => None,
        }
    }
}

impl PayloadField {
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    pub(crate) fn value(&self) -> &PayloadValue {
        &self.value
    }
}

impl PayloadFieldName {
    fn as_str(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Owned(value) => value,
        }
    }
}

struct PayloadValueSerializer;

#[derive(Debug)]
struct PayloadValueError(String);

impl fmt::Display for PayloadValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PayloadValueError {}

impl ser::Error for PayloadValueError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self(message.to_string())
    }
}

impl ser::Serializer for PayloadValueSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    type SerializeSeq = PayloadSeqSerializer;
    type SerializeTuple = PayloadSeqSerializer;
    type SerializeTupleStruct = PayloadSeqSerializer;
    type SerializeTupleVariant = PayloadTupleVariantSerializer;
    type SerializeMap = PayloadMapSerializer;
    type SerializeStruct = PayloadStructSerializer;
    type SerializeStructVariant = PayloadStructVariantSerializer;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Bool(value))
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::I64(i64::from(value)))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::I64(i64::from(value)))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::I64(i64::from(value)))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::I64(value))
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::U64(u64::from(value)))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::U64(u64::from(value)))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::U64(u64::from(value)))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::U64(value))
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::F64(f64::from(value)))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::F64(value))
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(value.to_string()))
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(value.to_string()))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Binary(value.to_vec()))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Null)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(variant.to_string()))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Ok(PayloadValue::Object(vec![PayloadField {
            name: PayloadFieldName::Static(variant),
            value: value.serialize(PayloadValueSerializer)?,
        }]))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(PayloadSeqSerializer {
            values: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(PayloadTupleVariantSerializer {
            variant,
            values: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(PayloadMapSerializer {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(PayloadStructSerializer {
            fields: Vec::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(PayloadStructVariantSerializer {
            variant,
            fields: Vec::with_capacity(len),
        })
    }

    fn collect_str<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + fmt::Display,
    {
        Ok(PayloadValue::String(value.to_string()))
    }
}

struct PayloadSeqSerializer {
    values: Vec<PayloadValue>,
}

impl SerializeSeq for PayloadSeqSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.values.push(value.serialize(PayloadValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Array(self.values))
    }
}

impl SerializeTuple for PayloadSeqSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for PayloadSeqSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

struct PayloadTupleVariantSerializer {
    variant: &'static str,
    values: Vec<PayloadValue>,
}

impl SerializeTupleVariant for PayloadTupleVariantSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.values.push(value.serialize(PayloadValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Object(vec![PayloadField {
            name: PayloadFieldName::Static(self.variant),
            value: PayloadValue::Array(self.values),
        }]))
    }
}

struct PayloadMapSerializer {
    entries: Vec<PayloadField>,
    next_key: Option<PayloadFieldName>,
}

impl SerializeMap for PayloadMapSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.next_key = Some(key.serialize(PayloadKeySerializer)?);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let Some(name) = self.next_key.take() else {
            return Err(PayloadValueError(
                "map value serialized before key".to_string(),
            ));
        };
        self.entries.push(PayloadField {
            name,
            value: value.serialize(PayloadValueSerializer)?,
        });
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Object(self.entries))
    }
}

struct PayloadStructSerializer {
    fields: Vec<PayloadField>,
}

impl SerializeStruct for PayloadStructSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.fields.push(PayloadField {
            name: PayloadFieldName::Static(key),
            value: value.serialize(PayloadValueSerializer)?,
        });
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Object(self.fields))
    }
}

struct PayloadStructVariantSerializer {
    variant: &'static str,
    fields: Vec<PayloadField>,
}

impl SerializeStructVariant for PayloadStructVariantSerializer {
    type Ok = PayloadValue;
    type Error = PayloadValueError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.fields.push(PayloadField {
            name: PayloadFieldName::Static(key),
            value: value.serialize(PayloadValueSerializer)?,
        });
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Object(vec![PayloadField {
            name: PayloadFieldName::Static(self.variant),
            value: PayloadValue::Object(self.fields),
        }]))
    }
}

struct PayloadKeySerializer;

impl ser::Serializer for PayloadKeySerializer {
    type Ok = PayloadFieldName;
    type Error = PayloadValueError;

    type SerializeSeq = Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeMap = Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(
            str::from_utf8(value)
                .map_err(|error| PayloadValueError(error.to_string()))?
                .to_string(),
        ))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(String::new()))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Owned(String::new()))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Static(name))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadFieldName::Static(variant))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Ok(PayloadFieldName::Static(variant))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(PayloadValueError(
            "sequence values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(PayloadValueError(
            "tuple values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(PayloadValueError(
            "tuple struct values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(PayloadValueError(
            "tuple variant values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(PayloadValueError(
            "map values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(PayloadValueError(
            "struct values cannot be used as map keys".to_string(),
        ))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(PayloadValueError(
            "struct variant values cannot be used as map keys".to_string(),
        ))
    }

    fn collect_str<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + fmt::Display,
    {
        Ok(PayloadFieldName::Owned(value.to_string()))
    }
}
