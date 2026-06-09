use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use syn::{Fields, Item, Type};

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");

    let proto_file = "proto/hitrace.proto";
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&[proto_file], &["proto"])
        .expect("hitrace proto compiles");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));
    generate_hitrace_event_arrow_builder(
        &out_dir.join("kat.hitrace.rs"),
        &out_dir.join("hitrace_event_arrow_generated.rs"),
    );

    println!("cargo:rerun-if-changed={proto_file}");
}

#[derive(Clone, Debug)]
struct ProtoField {
    name: String,
    rust_type: RustType,
}

#[derive(Clone, Debug)]
enum RustType {
    Bool,
    I32,
    I64,
    String,
    U32,
    U64,
}

impl RustType {
    fn arrow_data_type(&self) -> &'static str {
        match self {
            Self::Bool => "Boolean",
            Self::I32 => "Int32",
            Self::I64 => "Int64",
            Self::String => "Utf8",
            Self::U32 => "UInt32",
            Self::U64 => "UInt64",
        }
    }

    fn builder_type(&self) -> &'static str {
        match self {
            Self::Bool => "BooleanBuilder",
            Self::I32 => "Int32Builder",
            Self::I64 => "Int64Builder",
            Self::String => "StringBuilder",
            Self::U32 => "UInt32Builder",
            Self::U64 => "UInt64Builder",
        }
    }

    fn builder_init(&self, capacity: &str) -> String {
        match self {
            Self::String => "StringBuilder::new()".to_string(),
            _ => format!("{}::with_capacity({capacity})", self.builder_type()),
        }
    }

    fn append_value(&self, field_name: &str) -> String {
        match self {
            Self::String => format!("self.{field_name}.append_value(&row.{field_name});"),
            _ => format!("self.{field_name}.append_value(row.{field_name});"),
        }
    }
}

fn generate_hitrace_event_arrow_builder(prost_file: &Path, output_file: &Path) {
    let fields = parse_struct_fields(prost_file, "HitraceEvent");
    let source = render_arrow_builder_source(&fields);
    fs::write(output_file, source).expect("hitrace event arrow builder is written");
}

fn parse_struct_fields(prost_file: &Path, struct_name: &str) -> Vec<ProtoField> {
    let source = fs::read_to_string(prost_file).expect("prost generated source is readable");
    let syntax = syn::parse_file(&source).expect("prost generated source parses as Rust AST");

    for item in syntax.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_name {
            continue;
        }

        let Fields::Named(named_fields) = item_struct.fields else {
            panic!("{struct_name} must use named fields");
        };

        return named_fields
            .named
            .into_iter()
            .map(|field| {
                let name = field
                    .ident
                    .expect("prost generated struct field has a name")
                    .to_string();
                let rust_type = parse_rust_type(&field.ty)
                    .unwrap_or_else(|| panic!("unsupported protobuf field type for {name}"));

                ProtoField { name, rust_type }
            })
            .collect();
    }

    panic!("{struct_name} not found in {}", prost_file.display());
}

fn parse_rust_type(field_type: &Type) -> Option<RustType> {
    let Type::Path(type_path) = field_type else {
        return None;
    };
    let ident = type_path.path.segments.last()?.ident.to_string();

    match ident.as_str() {
        "bool" => Some(RustType::Bool),
        "i32" => Some(RustType::I32),
        "i64" => Some(RustType::I64),
        "String" => Some(RustType::String),
        "u32" => Some(RustType::U32),
        "u64" => Some(RustType::U64),
        _ => None,
    }
}

fn render_arrow_builder_source(fields: &[ProtoField]) -> String {
    let builder_types = fields
        .iter()
        .map(|field| field.rust_type.builder_type())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");

    let struct_fields = fields
        .iter()
        .map(|field| format!("    {}: {},", field.name, field.rust_type.builder_type()))
        .collect::<Vec<_>>()
        .join("\n");
    let builder_inits = fields
        .iter()
        .map(|field| {
            format!(
                "            {}: {},",
                field.name,
                field.rust_type.builder_init("capacity")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let schema_fields = fields
        .iter()
        .map(|field| {
            format!(
                "        Field::new(\"{}\", DataType::{}, false),",
                field.name,
                field.rust_type.arrow_data_type()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let append_values = fields
        .iter()
        .map(|field| format!("        {}", field.rust_type.append_value(&field.name)))
        .collect::<Vec<_>>()
        .join("\n");
    let arrays = fields
        .iter()
        .map(|field| format!("            Arc::new(self.{}.finish()),", field.name))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"// This file is generated by crates/kat-rs-datasource/build.rs.

use std::sync::Arc;

use anyhow::Result;
use arrow_array::{{
    RecordBatch,
    builder::{{{builder_types}}},
}};
use arrow_schema::{{DataType, Field, Schema}};

use crate::proto::HitraceEvent;

pub(crate) struct HitraceEventArrowBuilder {{
{struct_fields}
}}

impl HitraceEventArrowBuilder {{
    pub(crate) fn with_capacity(capacity: usize) -> Self {{
        Self {{
{builder_inits}
        }}
    }}

    pub(crate) fn append(&mut self, row: &HitraceEvent) {{
{append_values}
    }}

    pub(crate) fn finish(mut self) -> Result<RecordBatch> {{
        Ok(RecordBatch::try_new(
            hitrace_event_schema(),
            vec![
{arrays}
            ],
        )?)
    }}
}}

fn hitrace_event_schema() -> Arc<Schema> {{
    Arc::new(Schema::new(vec![
{schema_fields}
    ]))
}}
"#
    )
}
