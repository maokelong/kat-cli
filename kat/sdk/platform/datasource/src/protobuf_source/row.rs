use anyhow::{Context, Result};
use serde::{Serialize, Serializer};

pub(crate) trait EstimatedRow: Serialize {
    fn estimated_bytes(&self) -> Result<usize>;
}

pub(crate) trait EstimatedValue {
    fn estimated_bytes(&self) -> Result<usize>;
    fn estimated_null_bytes() -> Result<usize>;
}

#[derive(Clone, Copy)]
pub(crate) struct BinaryValue<'a>(&'a [u8]);

impl<'a> BinaryValue<'a> {
    pub(crate) const fn new(value: &'a [u8]) -> Self {
        Self(value)
    }
}

impl Serialize for BinaryValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(self.0)
    }
}

impl EstimatedValue for BinaryValue<'_> {
    fn estimated_bytes(&self) -> Result<usize> {
        variable_width_bytes(self.0.len(), "Binary")
    }

    fn estimated_null_bytes() -> Result<usize> {
        Ok(5)
    }
}

impl EstimatedValue for &str {
    fn estimated_bytes(&self) -> Result<usize> {
        variable_width_bytes(self.len(), "Utf8")
    }

    fn estimated_null_bytes() -> Result<usize> {
        Ok(5)
    }
}

impl<T> EstimatedValue for Option<T>
where
    T: EstimatedValue,
{
    fn estimated_bytes(&self) -> Result<usize> {
        match self {
            Some(value) => value.estimated_bytes(),
            None => T::estimated_null_bytes(),
        }
    }

    fn estimated_null_bytes() -> Result<usize> {
        T::estimated_null_bytes()
    }
}

macro_rules! fixed_width_value {
    ($type:ty, $bytes:expr) => {
        impl EstimatedValue for $type {
            fn estimated_bytes(&self) -> Result<usize> {
                Ok($bytes)
            }

            fn estimated_null_bytes() -> Result<usize> {
                Ok($bytes)
            }
        }
    };
}

fixed_width_value!(bool, 2);
fixed_width_value!(i32, 5);
fixed_width_value!(i64, 9);
fixed_width_value!(u32, 5);
fixed_width_value!(u64, 9);
fixed_width_value!(f32, 5);
fixed_width_value!(f64, 9);

pub(crate) fn add_estimated_bytes(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total
        .checked_add(bytes)
        .context("protobuf Source buffered byte estimate overflows")?;
    Ok(())
}

fn variable_width_bytes(len: usize, kind: &str) -> Result<usize> {
    len.checked_add(5)
        .with_context(|| format!("protobuf Source {kind} row-size estimate overflows"))
}
