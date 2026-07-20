#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtoFieldLabel {
    Optional,
    Repeated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ProtoScalarType {
    Bool,
    I32,
    I64,
    U32,
    U64,
    F32,
    F64,
    String,
    Bytes,
    Enum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtoFieldType {
    Scalar(ProtoScalarType),
    Message(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EnumValueDescriptor {
    pub(crate) number: i32,
    pub(crate) name: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FieldDescriptor {
    pub(crate) name: &'static str,
    pub(crate) label: ProtoFieldLabel,
    pub(crate) field_type: ProtoFieldType,
    pub(crate) oneof_name: Option<&'static str>,
    pub(crate) enum_values: &'static [EnumValueDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MessageDescriptor {
    pub(crate) package: &'static str,
    pub(crate) name: &'static str,
    pub(crate) fields: &'static [FieldDescriptor],
}

mod generated {
    include!(concat!(env!("OUT_DIR"), "/relational_descriptors.rs"));
}

pub(crate) use generated::RELATIONAL_DESCRIPTORS;

pub(crate) fn descriptor_root_names() -> Vec<String> {
    RELATIONAL_DESCRIPTORS
        .iter()
        .map(|message| message.name.to_string())
        .collect()
}
