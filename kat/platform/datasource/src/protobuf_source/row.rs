use anyhow::{Context, Result, anyhow, bail};
use arrow_array::builder::{
    ArrayBuilder, BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int32Builder,
    Int64Builder, StringBuilder, StructBuilder, UInt32Builder, UInt64Builder,
};
use arrow_schema::{DataType, FieldRef};

use super::{ColumnSlot, RelationSlot};

/// 单行的强类型写入视图。生成的写入器只按构建期槽位调用这些方法。
pub(crate) struct RowWriter<'a> {
    relation: RelationSlot,
    fields: &'a [FieldRef],
    builders: &'a mut [Box<dyn ArrayBuilder>],
    estimated_bytes: &'a mut usize,
    written: Vec<bool>,
    depth: usize,
}

impl<'a> RowWriter<'a> {
    pub(super) fn new(
        relation: RelationSlot,
        fields: &'a [FieldRef],
        builders: &'a mut [Box<dyn ArrayBuilder>],
        estimated_bytes: &'a mut usize,
    ) -> Result<Self> {
        Self::nested(relation, fields, builders, estimated_bytes, 0)
    }

    fn nested(
        relation: RelationSlot,
        fields: &'a [FieldRef],
        builders: &'a mut [Box<dyn ArrayBuilder>],
        estimated_bytes: &'a mut usize,
        depth: usize,
    ) -> Result<Self> {
        if fields.len() != builders.len() {
            bail!(
                "protobuf Source relation slot {} Struct depth {depth} has {} fields but {} builders",
                relation.index(),
                fields.len(),
                builders.len()
            );
        }
        Ok(Self {
            relation,
            fields,
            builders,
            estimated_bytes,
            written: vec![false; fields.len()],
            depth,
        })
    }

    pub(crate) fn null(&mut self, slot: ColumnSlot) -> Result<()> {
        let index = self.claim(slot)?;
        let field = &self.fields[index];
        if !field.is_nullable() {
            bail!(
                "protobuf Source relation slot {} Struct depth {} column slot {} is not nullable",
                self.relation.index(),
                self.depth,
                slot.index()
            );
        }
        let data_type = field.data_type().clone();
        self.account(estimated_null_bytes(&data_type)?)?;
        append_null_value(
            &data_type,
            self.builders[index].as_mut(),
            self.relation,
            self.depth,
            slot,
        )
    }

    pub(crate) fn bool(&mut self, slot: ColumnSlot, value: bool) -> Result<()> {
        self.typed::<BooleanBuilder>(slot, &DataType::Boolean, 2, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn i32(&mut self, slot: ColumnSlot, value: i32) -> Result<()> {
        self.typed::<Int32Builder>(slot, &DataType::Int32, 5, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn i64(&mut self, slot: ColumnSlot, value: i64) -> Result<()> {
        self.typed::<Int64Builder>(slot, &DataType::Int64, 9, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn u32(&mut self, slot: ColumnSlot, value: u32) -> Result<()> {
        self.typed::<UInt32Builder>(slot, &DataType::UInt32, 5, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn u64(&mut self, slot: ColumnSlot, value: u64) -> Result<()> {
        self.typed::<UInt64Builder>(slot, &DataType::UInt64, 9, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn f32(&mut self, slot: ColumnSlot, value: f32) -> Result<()> {
        self.typed::<Float32Builder>(slot, &DataType::Float32, 5, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn f64(&mut self, slot: ColumnSlot, value: f64) -> Result<()> {
        self.typed::<Float64Builder>(slot, &DataType::Float64, 9, |builder| {
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn utf8(&mut self, slot: ColumnSlot, value: &str) -> Result<()> {
        let estimated_bytes = value
            .len()
            .checked_add(5)
            .context("protobuf Source Utf8 row-size estimate overflows")?;
        self.typed::<StringBuilder>(slot, &DataType::Utf8, estimated_bytes, |builder| {
            checked_i32_offset(builder.values_slice().len(), value.len(), "Utf8")?;
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn binary(&mut self, slot: ColumnSlot, value: &[u8]) -> Result<()> {
        let estimated_bytes = value
            .len()
            .checked_add(5)
            .context("protobuf Source Binary row-size estimate overflows")?;
        self.typed::<BinaryBuilder>(slot, &DataType::Binary, estimated_bytes, |builder| {
            checked_i32_offset(builder.values_slice().len(), value.len(), "Binary")?;
            builder.append_value(value);
            Ok(())
        })
    }

    pub(crate) fn struct_<F>(&mut self, slot: ColumnSlot, present: bool, append: F) -> Result<()>
    where
        F: FnOnce(&mut RowWriter<'_>) -> Result<()>,
    {
        if !present {
            return self.null(slot);
        }

        let index = self.claim(slot)?;
        let data_type = self.fields[index].data_type().clone();
        let DataType::Struct(fields) = data_type else {
            bail!(
                "protobuf Source relation slot {} Struct depth {} column slot {} expected Struct but schema is {}",
                self.relation.index(),
                self.depth,
                slot.index(),
                data_type
            );
        };
        let relation_index = self.relation.index();
        let depth = self.depth;
        let slot_index = slot.index();
        let expected = DataType::Struct(fields.clone());
        self.account(1)?;
        let builder = self.builders[index]
            .as_any_mut()
            .downcast_mut::<StructBuilder>()
            .ok_or_else(|| {
                anyhow!(
                    "protobuf Source relation slot {relation_index} Struct depth {depth} column slot {slot_index} has no {expected} builder"
                )
            })?;
        {
            let mut nested = RowWriter::nested(
                self.relation,
                fields.as_ref(),
                builder.field_builders_mut(),
                self.estimated_bytes,
                self.depth + 1,
            )?;
            append(&mut nested)?;
            nested.finish()?;
        }
        builder.append(true);
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<()> {
        if let Some(index) = self.written.iter().position(|written| !written) {
            bail!(
                "protobuf Source relation slot {} Struct depth {} did not append column slot {index}",
                self.relation.index(),
                self.depth
            );
        }
        Ok(())
    }

    fn typed<B>(
        &mut self,
        slot: ColumnSlot,
        expected: &DataType,
        estimated_bytes: usize,
        append: impl FnOnce(&mut B) -> Result<()>,
    ) -> Result<()>
    where
        B: ArrayBuilder + 'static,
    {
        let index = self.claim(slot)?;
        let actual = self.fields[index].data_type();
        if actual != expected {
            bail!(
                "protobuf Source relation slot {} Struct depth {} column slot {} expected {expected} but schema is {actual}",
                self.relation.index(),
                self.depth,
                slot.index()
            );
        }
        self.account(estimated_bytes)?;
        let relation_index = self.relation.index();
        let depth = self.depth;
        let slot_index = slot.index();
        let builder = self.builders[index]
            .as_any_mut()
            .downcast_mut::<B>()
            .ok_or_else(|| {
                anyhow!(
                    "protobuf Source relation slot {relation_index} Struct depth {depth} column slot {slot_index} has no {expected} builder"
                )
            })?;
        append(builder)
    }

    fn claim(&mut self, slot: ColumnSlot) -> Result<usize> {
        let index = slot.index();
        let Some(written) = self.written.get_mut(index) else {
            bail!(
                "protobuf Source relation slot {} Struct depth {} has no column slot {index}",
                self.relation.index(),
                self.depth
            );
        };
        if *written {
            bail!(
                "protobuf Source relation slot {} Struct depth {} appended column slot {index} twice",
                self.relation.index(),
                self.depth
            );
        }
        *written = true;
        Ok(index)
    }

    fn account(&mut self, bytes: usize) -> Result<()> {
        *self.estimated_bytes = self
            .estimated_bytes
            .checked_add(bytes)
            .context("protobuf Source buffered byte estimate overflows")?;
        Ok(())
    }
}

pub(super) fn checked_i32_offset(current: usize, additional: usize, kind: &str) -> Result<()> {
    let total = current
        .checked_add(additional)
        .with_context(|| format!("protobuf Source {kind} Arrow value offset overflows usize"))?;
    if total > i32::MAX as usize {
        bail!(
            "protobuf Source {kind} Arrow value offset {total} exceeds the Int32 offset limit {}",
            i32::MAX
        );
    }
    Ok(())
}

fn estimated_null_bytes(data_type: &DataType) -> Result<usize> {
    match data_type {
        DataType::Boolean => Ok(2),
        DataType::Int32 | DataType::UInt32 | DataType::Float32 => Ok(5),
        DataType::Int64 | DataType::UInt64 | DataType::Float64 => Ok(9),
        DataType::Utf8 | DataType::Binary => Ok(5),
        DataType::Struct(fields) => fields.iter().try_fold(1_usize, |total, field| {
            total
                .checked_add(estimated_null_bytes(field.data_type())?)
                .context("protobuf Source Struct null-size estimate overflows")
        }),
        other => bail!("cannot estimate null size for unsupported Arrow type {other}"),
    }
}

fn append_null_value(
    data_type: &DataType,
    builder: &mut dyn ArrayBuilder,
    relation: RelationSlot,
    depth: usize,
    slot: ColumnSlot,
) -> Result<()> {
    macro_rules! append_null {
        ($builder:ty) => {{
            builder
                .as_any_mut()
                .downcast_mut::<$builder>()
                .ok_or_else(|| {
                    anyhow!(
                        "protobuf Source relation slot {} Struct depth {depth} column slot {} has no {} builder",
                        relation.index(),
                        slot.index(),
                        data_type
                    )
                })?
                .append_null();
            Ok(())
        }};
    }

    match data_type {
        DataType::Boolean => append_null!(BooleanBuilder),
        DataType::Int32 => append_null!(Int32Builder),
        DataType::Int64 => append_null!(Int64Builder),
        DataType::UInt32 => append_null!(UInt32Builder),
        DataType::UInt64 => append_null!(UInt64Builder),
        DataType::Float32 => append_null!(Float32Builder),
        DataType::Float64 => append_null!(Float64Builder),
        DataType::Utf8 => append_null!(StringBuilder),
        DataType::Binary => append_null!(BinaryBuilder),
        DataType::Struct(fields) => {
            let struct_builder = builder
                .as_any_mut()
                .downcast_mut::<StructBuilder>()
                .ok_or_else(|| {
                    anyhow!(
                        "protobuf Source relation slot {} Struct depth {depth} column slot {} has no Struct builder",
                        relation.index(),
                        slot.index()
                    )
                })?;
            if fields.len() != struct_builder.field_builders().len() {
                bail!(
                    "protobuf Source relation slot {} Struct depth {depth} column slot {} has inconsistent Struct builders",
                    relation.index(),
                    slot.index()
                );
            }
            for (field, child) in fields.iter().zip(struct_builder.field_builders_mut()) {
                append_null_value(field.data_type(), child.as_mut(), relation, depth + 1, slot)?;
            }
            struct_builder.append(false);
            Ok(())
        }
        other => bail!(
            "protobuf Source relation slot {} Struct depth {depth} column slot {} cannot append null for unsupported Arrow type {other}",
            relation.index(),
            slot.index()
        ),
    }
}
