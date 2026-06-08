//! htrace 文件读取和 protobuf payload 解码。

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use prost_reflect::{DescriptorPool, DynamicMessage, Value};
use trace_arrow::{build_table, TraceDataset};

use crate::table_specs::table_specs;

const ROOT_MESSAGE: &str = "kat.htrace.HtraceTrace";

/// 从磁盘读取 htrace 文件，并解析为 TraceDataset。
pub fn parse(path: impl AsRef<Path>) -> Result<TraceDataset> {
    let bytes = fs::read(path.as_ref())
        .with_context(|| format!("failed to read htrace file: {}", path.as_ref().display()))?;
    parse_bytes(&bytes)
}

/// 从内存 bytes 解析 htrace protobuf，并构建 TraceDataset。
pub fn parse_bytes(bytes: &[u8]) -> Result<TraceDataset> {
    let pool = DescriptorPool::decode(htrace_proto::FILE_DESCRIPTOR_SET)
        .context("failed to decode htrace descriptor set")?;
    let root_descriptor = pool
        .get_message_by_name(ROOT_MESSAGE)
        .with_context(|| format!("protobuf message not found: {ROOT_MESSAGE}"))?;
    let root = DynamicMessage::decode(root_descriptor.clone(), bytes)
        .context("failed to decode htrace root message")?;

    let mut tables = Vec::new();
    for table_spec in table_specs() {
        let repeated_field = root_descriptor
            .get_field_by_name(table_spec.repeated_field)
            .with_context(|| {
                format!(
                    "root field `{}` not found for table `{}`",
                    table_spec.repeated_field, table_spec.name
                )
            })?;
        let value = root.get_field(&repeated_field);
        let records = repeated_messages(value.as_ref(), table_spec.name)?;
        if let Some(table) = build_table(table_spec, records)? {
            tables.push(table);
        }
    }

    TraceDataset::from_tables(tables)
}

/// 将 root repeated message 字段转换为 DynamicMessage 列表。
fn repeated_messages(value: &Value, table_name: &str) -> Result<Vec<DynamicMessage>> {
    let Value::List(values) = value else {
        bail!("table `{table_name}` source field is not repeated");
    };

    values
        .iter()
        .map(|value| match value {
            Value::Message(message) => Ok(message.clone()),
            other => bail!("table `{table_name}` contains non-message value: {other:?}"),
        })
        .collect()
}
