#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColumnType {
    Binary,
    Bool,
    I32,
    I64,
    U32,
    U64,
    F32,
    F64,
    String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ColumnSpec {
    pub(crate) name: String,
    pub(crate) column_type: ColumnType,
}

impl ColumnSpec {
    pub(crate) fn new(name: impl Into<String>, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CellValue {
    Null,
    Binary(Vec<u8>),
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
}
