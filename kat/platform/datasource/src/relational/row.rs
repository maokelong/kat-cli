#[derive(Clone, Debug, Eq, PartialEq)]
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
    Struct(Vec<ColumnSpec>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ColumnProjection {
    Direct,
    EnumName(&'static [super::descriptor::EnumValueDescriptor]),
    OneofName(Vec<OneofVariantName>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OneofVariantName {
    pub(crate) field_name: &'static str,
    pub(crate) serialized_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ColumnSpec {
    pub(crate) name: String,
    pub(crate) source_name: String,
    pub(crate) column_type: ColumnType,
    pub(crate) projection: ColumnProjection,
}

impl ColumnSpec {
    pub(crate) fn new(name: impl Into<String>, column_type: ColumnType) -> Self {
        let name = name.into();
        Self {
            source_name: name.clone(),
            name,
            column_type,
            projection: ColumnProjection::Direct,
        }
    }

    pub(crate) fn projected(
        name: impl Into<String>,
        source_name: impl Into<String>,
        column_type: ColumnType,
        projection: ColumnProjection,
    ) -> Self {
        Self {
            name: name.into(),
            source_name: source_name.into(),
            column_type,
            projection,
        }
    }
}
