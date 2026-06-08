use prost_reflect::prost::Message;
use prost_reflect::prost_types::{
    field_descriptor_proto::{Label, Type},
    DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
};
use prost_reflect::{DescriptorPool, DynamicMessage, Value};
use trace_arrow::{build_table, ArrowType, FieldSpec, TableSpec};

const PROCESS_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "timestamp_ns",
        source: "timestamp_ns",
        arrow_type: ArrowType::UInt64,
        nullable: false,
        repeated: false,
    },
    FieldSpec {
        name: "pid",
        source: "pid",
        arrow_type: ArrowType::UInt32,
        nullable: false,
        repeated: false,
    },
    FieldSpec {
        name: "process_name",
        source: "process_name",
        arrow_type: ArrowType::Utf8,
        nullable: false,
        repeated: false,
    },
];

const PROCESS_TABLE: TableSpec = TableSpec {
    name: "process_event",
    source: "test.ProcessEvent",
    repeated_field: "process_events",
    fields: PROCESS_FIELDS,
};

#[test]
fn dynamic_messages_are_written_to_arrow_table() {
    let pool = DescriptorPool::decode(test_descriptor_bytes().as_slice()).unwrap();
    let descriptor = pool
        .get_message_by_name("test.ProcessEvent")
        .expect("descriptor exists");
    let mut message = DynamicMessage::new(descriptor);
    message.set_field_by_name("timestamp_ns", Value::U64(7));
    message.set_field_by_name("pid", Value::U32(42));
    message.set_field_by_name("process_name", Value::String("wechat".to_string()));

    let table = build_table(&PROCESS_TABLE, [message])
        .unwrap()
        .expect("table has rows");

    assert_eq!(table.name, "process_event");
    assert_eq!(table.batches.len(), 1);
    assert_eq!(table.batches[0].num_rows(), 1);
}

fn test_descriptor_bytes() -> Vec<u8> {
    FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("process_event.proto".to_string()),
            package: Some("test".to_string()),
            message_type: vec![DescriptorProto {
                name: Some("ProcessEvent".to_string()),
                field: vec![
                    field("timestamp_ns", 1, Type::Uint64),
                    field("pid", 2, Type::Uint32),
                    field("process_name", 3, Type::String),
                ],
                ..Default::default()
            }],
            syntax: Some("proto3".to_string()),
            ..Default::default()
        }],
    }
    .encode_to_vec()
}

fn field(name: &str, number: i32, field_type: Type) -> FieldDescriptorProto {
    FieldDescriptorProto {
        name: Some(name.to_string()),
        number: Some(number),
        label: Some(Label::Optional as i32),
        r#type: Some(field_type as i32),
        ..Default::default()
    }
}
