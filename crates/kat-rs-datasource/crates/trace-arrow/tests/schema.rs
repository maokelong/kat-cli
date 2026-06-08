use arrow_schema::DataType;
use trace_arrow::{schema_for_table, ArrowType, FieldSpec, TableSpec};

const PROCESS_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "timestamp_ns",
        source: "timestamp_ns",
        arrow_type: ArrowType::UInt64,
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
    source: "kat.htrace.ProcessEvent",
    repeated_field: "process_events",
    fields: PROCESS_FIELDS,
};

#[test]
fn schema_is_built_from_table_spec() {
    let schema = schema_for_table(&PROCESS_TABLE);

    assert_eq!(schema.field(0).name(), "timestamp_ns");
    assert_eq!(schema.field(0).data_type(), &DataType::UInt64);
    assert_eq!(schema.field(1).name(), "process_name");
    assert_eq!(schema.field(1).data_type(), &DataType::Utf8);
}
