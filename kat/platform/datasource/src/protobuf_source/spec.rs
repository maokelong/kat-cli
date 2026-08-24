use std::collections::HashSet;

use anyhow::{Result, bail};
use arrow_schema::{DataType, SchemaRef};

pub(crate) const PROTOBUF_ENUM_SYMBOL_TABLE: &str = "protobuf_enum_symbol";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RelationSlot(usize);

impl RelationSlot {
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    pub(super) const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone)]
pub(crate) struct RelationSpec {
    pub(super) name: &'static str,
    pub(super) schema: SchemaRef,
}

impl RelationSpec {
    pub(crate) fn new(name: &'static str, schema: SchemaRef) -> Self {
        Self { name, schema }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumSymbol {
    pub(super) number: i32,
    pub(super) symbol: &'static str,
}

impl EnumSymbol {
    pub(crate) const fn new(number: i32, symbol: &'static str) -> Self {
        Self { number, symbol }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumOriginSpec {
    pub(super) relation: RelationSlot,
    pub(super) field_path: &'static str,
    pub(super) enum_type_name: &'static str,
    pub(super) symbols: &'static [EnumSymbol],
}

impl EnumOriginSpec {
    pub(crate) const fn new(
        relation: RelationSlot,
        field_path: &'static str,
        enum_type_name: &'static str,
        symbols: &'static [EnumSymbol],
    ) -> Self {
        Self {
            relation,
            field_path,
            enum_type_name,
            symbols,
        }
    }
}

/// 字节门限按行值的逻辑大小估算；单行超过门限时仍写成一个独立批次。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpoolOptions {
    pub(super) max_buffered_rows: usize,
    pub(super) max_buffered_bytes: usize,
}

impl SpoolOptions {
    pub(crate) const DEFAULT_MAX_BUFFERED_ROWS: usize = 8_192;
    pub(crate) const DEFAULT_MAX_BUFFERED_BYTES: usize = 64 * 1024 * 1024;

    pub(crate) const fn new(max_buffered_rows: usize) -> Self {
        Self::with_limits(max_buffered_rows, Self::DEFAULT_MAX_BUFFERED_BYTES)
    }

    pub(crate) const fn with_limits(max_buffered_rows: usize, max_buffered_bytes: usize) -> Self {
        Self {
            max_buffered_rows,
            max_buffered_bytes,
        }
    }

    pub(super) fn validate(self) -> Result<()> {
        if self.max_buffered_rows == 0 {
            bail!("protobuf Source-table spool row limit must be greater than zero");
        }
        if i64::try_from(self.max_buffered_rows).is_err() {
            bail!("protobuf Source-table spool row limit exceeds Parquet Int64 metadata");
        }
        if self.max_buffered_bytes == 0 {
            bail!("protobuf Source-table spool byte limit must be greater than zero");
        }
        Ok(())
    }
}

impl Default for SpoolOptions {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_BUFFERED_ROWS)
    }
}

pub(super) fn validate_specs(
    relations: &[RelationSpec],
    enum_origins: &[EnumOriginSpec],
) -> Result<()> {
    let mut relation_names = HashSet::new();
    for (slot, relation) in relations.iter().enumerate() {
        if !crate::valid_table_name(relation.name) {
            bail!(
                "protobuf Source relation slot {slot} has invalid Dataset table name {:?}",
                relation.name
            );
        }
        if relation.name == PROTOBUF_ENUM_SYMBOL_TABLE {
            bail!(
                "protobuf Source relation slot {slot} uses reserved table name {:?}",
                relation.name
            );
        }
        if !relation_names.insert(relation.name) {
            bail!(
                "duplicate protobuf Source relation name {:?}",
                relation.name
            );
        }

        let mut columns = HashSet::new();
        for field in relation.schema.fields() {
            if !columns.insert(field.name()) {
                bail!(
                    "protobuf Source relation {:?} has duplicate top-level column {:?}",
                    relation.name,
                    field.name()
                );
            }
            validate_data_type(field.data_type()).map_err(|source| {
                anyhow::anyhow!(
                    "protobuf Source relation {:?} column {:?} is unsupported: {source}",
                    relation.name,
                    field.name()
                )
            })?;
        }
    }

    let mut origins = HashSet::new();
    for origin in enum_origins {
        let Some(relation) = relations.get(origin.relation.index()) else {
            bail!(
                "enum origin refers to missing relation slot {}",
                origin.relation.index()
            );
        };
        if origin.field_path.is_empty() {
            bail!(
                "enum origin in relation {:?} has an empty field path",
                relation.name
            );
        }
        if origin.enum_type_name.is_empty() {
            bail!(
                "enum origin {:?}.{:?} has an empty enum type name",
                relation.name,
                origin.field_path
            );
        }
        if origin.symbols.is_empty() {
            bail!(
                "enum origin {:?}.{:?} has no descriptor symbols",
                relation.name,
                origin.field_path
            );
        }
        if !origins.insert((origin.relation, origin.field_path)) {
            bail!(
                "duplicate enum origin {:?}.{:?}",
                relation.name,
                origin.field_path
            );
        }

        let mut numbers = HashSet::new();
        for symbol in origin.symbols {
            if symbol.symbol.is_empty() {
                bail!(
                    "enum origin {:?}.{:?} has an empty symbol for number {}",
                    relation.name,
                    origin.field_path,
                    symbol.number
                );
            }
            if !numbers.insert(symbol.number) {
                bail!(
                    "enum origin {:?}.{:?} repeats enum number {}",
                    relation.name,
                    origin.field_path,
                    symbol.number
                );
            }
        }
    }

    Ok(())
}

fn validate_data_type(data_type: &DataType) -> Result<()> {
    match data_type {
        DataType::Boolean
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float32
        | DataType::Float64
        | DataType::Utf8
        | DataType::Binary => Ok(()),
        DataType::Struct(fields) => {
            let mut names = HashSet::new();
            for field in fields {
                if !names.insert(field.name()) {
                    bail!("Struct repeats child field name {:?}", field.name());
                }
                validate_data_type(field.data_type())?;
            }
            Ok(())
        }
        other => bail!("Arrow type {other} is outside the fixed protobuf mapping"),
    }
}
