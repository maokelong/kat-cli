use super::*;

#[tokio::test]
async fn standalone_proto3_and_proto2_optional_fields_distinguish_absent_from_default() {
    use generated_fixture_emitter::{
        append_proto2_optional_root_root, append_scalar_matrix_root, new_protobuf_source_capture,
    };
    use proto::fixture::protobuf_source::valid::{Proto2OptionalRoot, ScalarMatrix};
    use protobuf_source::SpoolOptions;

    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(1)).expect("generated capture is valid");
    append_scalar_matrix_root(&mut capture, 7_000, &ScalarMatrix::default())
        .expect("absent proto3 optional fields append");
    append_scalar_matrix_root(
        &mut capture,
        7_001,
        &ScalarMatrix {
            optional_count: Some(0),
            optional_label: Some(String::new()),
            optional_bytes: Some(Vec::new()),
            optional_lifecycle: Some(0),
            ..Default::default()
        },
    )
    .expect("present-default proto3 optional fields append");
    append_proto2_optional_root_root(&mut capture, 7_100, &Proto2OptionalRoot::default())
        .expect("absent proto2 optional fields append");
    append_proto2_optional_root_root(
        &mut capture,
        7_101,
        &Proto2OptionalRoot {
            count: Some(0),
            label: Some(String::new()),
        },
    )
    .expect("present-default proto2 optional fields append");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let scalar_schema = parquet_arrow_schema(&dataset_path, "scalar_matrix");
    assert_eq!(scalar_schema.fields().len(), 21);
    for field_name in [
        "_kat_parent_row_id",
        "double_value",
        "float_value",
        "int32_value",
        "int64_value",
        "uint32_value",
        "uint64_value",
        "sint32_value",
        "sint64_value",
        "fixed32_value",
        "fixed64_value",
        "sfixed32_value",
        "sfixed64_value",
        "bool_value",
        "string_value",
        "bytes_value",
        "lifecycle",
    ] {
        assert!(
            !scalar_schema
                .field_with_name(field_name)
                .expect("implicit scalar field exists")
                .is_nullable(),
            "implicit scalar field {field_name:?} must be non-null"
        );
    }
    for field_name in [
        "optional_count",
        "optional_label",
        "optional_bytes",
        "optional_lifecycle",
    ] {
        assert!(
            scalar_schema
                .field_with_name(field_name)
                .expect("proto3 optional field exists")
                .is_nullable(),
            "proto3 optional field {field_name:?} must be nullable"
        );
    }
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, int32_value, string_value, bytes_value, lifecycle, \
             optional_count, optional_label, optional_bytes, optional_lifecycle \
             from scalar_matrix order by _kat_parent_row_id",
        )
        .await,
        json!([
            {
                "_kat_parent_row_id": 7_000,
                "int32_value": 0,
                "string_value": "",
                "bytes_value": "",
                "lifecycle": 0,
                "optional_count": null,
                "optional_label": null,
                "optional_bytes": null,
                "optional_lifecycle": null,
            },
            {
                "_kat_parent_row_id": 7_001,
                "int32_value": 0,
                "string_value": "",
                "bytes_value": "",
                "lifecycle": 0,
                "optional_count": 0,
                "optional_label": "",
                "optional_bytes": "",
                "optional_lifecycle": 0,
            },
        ])
    );

    let proto2_schema = parquet_arrow_schema(&dataset_path, "proto2_optional_root");
    assert_flat_schema(
        proto2_schema.as_ref(),
        &[
            ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
            ("count", arrow_schema::DataType::Int32, true),
            ("label", arrow_schema::DataType::Utf8, true),
        ],
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, count, label \
             from proto2_optional_root order by _kat_parent_row_id",
        )
        .await,
        json!([
            { "_kat_parent_row_id": 7_100, "count": null, "label": null },
            { "_kat_parent_row_id": 7_101, "count": 0, "label": "" },
        ])
    );
}

#[tokio::test]
async fn explicit_defaults_and_nullable_struct_ancestors_survive_round_trip() {
    use generated_fixture_emitter::{
        append_alpha_shared_root_root, append_full_shape_root_root, new_protobuf_source_capture,
    };
    use proto::fixture::protobuf_source::alpha::SharedRoot;
    use proto::fixture::protobuf_source::valid::{
        FullShapeRoot, LeafValue, NullableInner, NullableOuter, ScalarMatrix,
    };
    use protobuf_source::SpoolOptions;

    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(1)).expect("generated capture is valid");
    append_full_shape_root_root(&mut capture, 1_000, &FullShapeRoot::default())
        .expect("fully absent root appends");
    append_full_shape_root_root(
        &mut capture,
        1_001,
        &FullShapeRoot {
            scalars: Some(ScalarMatrix::default()),
            nullable_outer: Some(NullableOuter::default()),
            ..Default::default()
        },
    )
    .expect("present messages with absent optional scalars append");
    append_full_shape_root_root(
        &mut capture,
        1_002,
        &FullShapeRoot {
            scalars: Some(ScalarMatrix {
                optional_count: Some(0),
                optional_label: Some(String::new()),
                optional_bytes: Some(Vec::new()),
                optional_lifecycle: Some(0),
                ..Default::default()
            }),
            nullable_outer: Some(NullableOuter {
                inner: Some(NullableInner::default()),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .expect("present-default optional values and nested message append");
    append_full_shape_root_root(
        &mut capture,
        1_003,
        &FullShapeRoot {
            nullable_outer: Some(NullableOuter {
                inner: Some(NullableInner {
                    leaf: Some(LeafValue::default()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .expect("present-default leaf message appends");
    append_alpha_shared_root_root(&mut capture, 1_004, &SharedRoot { alpha_value: 0 })
        .expect("direct implicit scalar root appends as a nullability counterexample");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let rows = query_json(
        &context,
        "select _kat_parent_row_id, scalars, nullable_outer \
         from full_shape_root order by _kat_parent_row_id",
    )
    .await;
    assert_eq!(
        rows,
        json!([
            {
                "_kat_parent_row_id": 1_000,
                "scalars": null,
                "nullable_outer": null,
            },
            {
                "_kat_parent_row_id": 1_001,
                "scalars": {
                    "double_value": 0.0,
                    "float_value": 0.0,
                    "int32_value": 0,
                    "int64_value": 0,
                    "uint32_value": 0,
                    "uint64_value": 0,
                    "sint32_value": 0,
                    "sint64_value": 0,
                    "fixed32_value": 0,
                    "fixed64_value": 0,
                    "sfixed32_value": 0,
                    "sfixed64_value": 0,
                    "bool_value": false,
                    "string_value": "",
                    "bytes_value": "",
                    "lifecycle": 0,
                    "optional_count": null,
                    "optional_label": null,
                    "optional_bytes": null,
                    "optional_lifecycle": null,
                },
                "nullable_outer": { "inner": null, "outer_value": 0 },
            },
            {
                "_kat_parent_row_id": 1_002,
                "scalars": {
                    "double_value": 0.0,
                    "float_value": 0.0,
                    "int32_value": 0,
                    "int64_value": 0,
                    "uint32_value": 0,
                    "uint64_value": 0,
                    "sint32_value": 0,
                    "sint64_value": 0,
                    "fixed32_value": 0,
                    "fixed64_value": 0,
                    "sfixed32_value": 0,
                    "sfixed64_value": 0,
                    "bool_value": false,
                    "string_value": "",
                    "bytes_value": "",
                    "lifecycle": 0,
                    "optional_count": 0,
                    "optional_label": "",
                    "optional_bytes": "",
                    "optional_lifecycle": 0,
                },
                "nullable_outer": {
                    "inner": { "leaf": null, "inner_value": 0 },
                    "outer_value": 0,
                },
            },
            {
                "_kat_parent_row_id": 1_003,
                "scalars": null,
                "nullable_outer": {
                    "inner": {
                        "leaf": { "code": 0, "label": "", "payload": "" },
                        "inner_value": 0,
                    },
                    "outer_value": 0,
                },
            },
        ])
    );

    let full_shape_schema = parquet_arrow_schema(&dataset_path, "full_shape_root");
    for key in ["_kat_row_id", "_kat_parent_row_id"] {
        let field = full_shape_schema
            .field_with_name(key)
            .expect("relationship key exists");
        assert!(
            !field.is_nullable(),
            "relationship key {key:?} must be non-null"
        );
        assert_eq!(
            field.data_type(),
            &arrow_schema::DataType::UInt64,
            "relationship key {key:?} must use UInt64"
        );
    }
    for struct_name in ["scalars", "nullable_outer"] {
        let field = full_shape_schema
            .field_with_name(struct_name)
            .expect("nullable inline Struct exists");
        assert!(field.is_nullable(), "{struct_name:?} must be nullable");
        assert_all_struct_descendants_nullable(field.data_type(), struct_name);
    }
    let scalar_field = full_shape_schema
        .field_with_name("scalars")
        .expect("scalar Struct exists");
    let arrow_schema::DataType::Struct(scalar_children) = scalar_field.data_type() else {
        panic!("scalars must be an Arrow Struct");
    };
    let expected_scalar_children = [
        ("double_value", arrow_schema::DataType::Float64),
        ("float_value", arrow_schema::DataType::Float32),
        ("int32_value", arrow_schema::DataType::Int32),
        ("int64_value", arrow_schema::DataType::Int64),
        ("uint32_value", arrow_schema::DataType::UInt32),
        ("uint64_value", arrow_schema::DataType::UInt64),
        ("sint32_value", arrow_schema::DataType::Int32),
        ("sint64_value", arrow_schema::DataType::Int64),
        ("fixed32_value", arrow_schema::DataType::UInt32),
        ("fixed64_value", arrow_schema::DataType::UInt64),
        ("sfixed32_value", arrow_schema::DataType::Int32),
        ("sfixed64_value", arrow_schema::DataType::Int64),
        ("bool_value", arrow_schema::DataType::Boolean),
        ("string_value", arrow_schema::DataType::Utf8),
        ("bytes_value", arrow_schema::DataType::Binary),
        ("lifecycle", arrow_schema::DataType::Int32),
        ("optional_count", arrow_schema::DataType::Int32),
        ("optional_label", arrow_schema::DataType::Utf8),
        ("optional_bytes", arrow_schema::DataType::Binary),
        ("optional_lifecycle", arrow_schema::DataType::Int32),
    ];
    assert_eq!(scalar_children.len(), expected_scalar_children.len());
    for (actual, (expected_name, expected_type)) in
        scalar_children.iter().zip(&expected_scalar_children)
    {
        assert_eq!(actual.name(), expected_name);
        assert_eq!(actual.data_type(), expected_type);
        assert!(
            actual.is_nullable(),
            "scalar child {expected_name:?} inherits nullable message presence"
        );
    }
    let alpha_schema = parquet_arrow_schema(&dataset_path, "alpha_shared_root");
    let alpha_value = alpha_schema
        .field_with_name("alpha_value")
        .expect("direct implicit scalar exists");
    assert!(
        !alpha_value.is_nullable(),
        "a direct implicit scalar without a nullable ancestor stays non-null"
    );
    assert_eq!(alpha_value.data_type(), &arrow_schema::DataType::Int32);

    let resolved = kat_datasource::resolve_dataset(&dataset_path)
        .expect("published sparse fixture Dataset resolves");
    let published_tables = resolved
        .tables()
        .iter()
        .map(|table| table.name())
        .collect::<std::collections::HashSet<_>>();
    for absent_table in [
        "full_shape_root_oneof_matrix",
        "full_shape_root_oneof_matrix_message_value",
        "full_shape_root_repeated_matrix",
        "full_shape_root_repeated_matrix_scalar_values",
        "full_shape_root_repeated_matrix_bytes_values",
        "full_shape_root_repeated_matrix_enum_values",
        "full_shape_root_repeated_matrix_message_values",
        "full_shape_root_relation_container",
        "full_shape_root_relation_container_children",
    ] {
        assert!(
            !published_tables.contains(absent_table),
            "inactive relation {absent_table:?} must not be published"
        );
    }
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, count(*) as symbol_count \
             from protobuf_enum_symbol \
             group by origin_table, origin_field_path \
             order by origin_table, origin_field_path",
        )
        .await,
        json!([
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.lifecycle",
                "symbol_count": 3,
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.optional_lifecycle",
                "symbol_count": 3,
            },
        ])
    );
}

fn assert_all_struct_descendants_nullable(data_type: &arrow_schema::DataType, path: &str) {
    let arrow_schema::DataType::Struct(fields) = data_type else {
        panic!("{path:?} must be an Arrow Struct, got {data_type:?}");
    };
    for field in fields {
        let child_path = format!("{path}.{}", field.name());
        assert!(
            field.is_nullable(),
            "inline descendant {child_path:?} must inherit ancestor nullability"
        );
        if matches!(field.data_type(), arrow_schema::DataType::Struct(_)) {
            assert_all_struct_descendants_nullable(field.data_type(), &child_path);
        }
    }
}

fn assert_flat_schema(
    schema: &arrow_schema::Schema,
    expected: &[(&str, arrow_schema::DataType, bool)],
) {
    assert_eq!(schema.fields().len(), expected.len());
    for (actual, (expected_name, expected_type, expected_nullable)) in
        schema.fields().iter().zip(expected)
    {
        assert_eq!(actual.name(), expected_name);
        assert_eq!(actual.data_type(), expected_type);
        assert_eq!(
            actual.is_nullable(),
            *expected_nullable,
            "unexpected nullability for column {expected_name:?}"
        );
    }
}

#[tokio::test]
async fn oneof_membership_preserves_default_values_and_message_parentage() {
    use generated_fixture_emitter::{append_full_shape_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{
        FullShapeRoot, LeafValue, OneofMatrix, oneof_matrix::Selected,
    };
    use protobuf_source::SpoolOptions;

    let cases = [
        (2_000, Some(Selected::ScalarValue(0))),
        (2_001, Some(Selected::BytesValue(vec![0x00, 0xff, 0x80]))),
        (2_002, Some(Selected::EnumValue(0))),
        (2_003, Some(Selected::EnumValue(77))),
        (
            2_004,
            Some(Selected::MessageValue(LeafValue {
                code: 41,
                label: "message-variant".to_string(),
                payload: vec![0, 0xff, 0x80],
            })),
        ),
        (2_005, None),
    ];
    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(2, 8))
        .expect("generated capture is valid");
    for (parent_row_id, selected) in cases {
        append_full_shape_root_root(
            &mut capture,
            parent_row_id,
            &FullShapeRoot {
                oneof_matrix: Some(OneofMatrix { selected }),
                ..Default::default()
            },
        )
        .expect("oneof fixture root appends");
    }

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let oneof_parent = parquet_arrow_schema(&dataset_path, "full_shape_root_oneof_matrix");
    assert_flat_schema(
        oneof_parent.as_ref(),
        &[
            ("_kat_row_id", arrow_schema::DataType::UInt64, false),
            ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
            ("scalar_value", arrow_schema::DataType::Int64, true),
            ("bytes_value", arrow_schema::DataType::Binary, true),
            ("enum_value", arrow_schema::DataType::Int32, true),
        ],
    );
    let message_variant =
        parquet_arrow_schema(&dataset_path, "full_shape_root_oneof_matrix_message_value");
    assert_flat_schema(
        message_variant.as_ref(),
        &[
            ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
            ("code", arrow_schema::DataType::Int32, false),
            ("label", arrow_schema::DataType::Utf8, false),
            ("payload", arrow_schema::DataType::Binary, false),
        ],
    );

    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id, oneof_row._kat_row_id, \
             oneof_row.scalar_value, oneof_row.bytes_value, oneof_row.enum_value \
             from full_shape_root root \
             join full_shape_root_oneof_matrix oneof_row \
               on oneof_row._kat_parent_row_id = root._kat_row_id \
             order by root._kat_parent_row_id",
        )
        .await,
        json!([
            {
                "_kat_parent_row_id": 2_000,
                "_kat_row_id": 0,
                "scalar_value": 0,
                "bytes_value": null,
                "enum_value": null,
            },
            {
                "_kat_parent_row_id": 2_001,
                "_kat_row_id": 1,
                "scalar_value": null,
                "bytes_value": "00ff80",
                "enum_value": null,
            },
            {
                "_kat_parent_row_id": 2_002,
                "_kat_row_id": 2,
                "scalar_value": null,
                "bytes_value": null,
                "enum_value": 0,
            },
            {
                "_kat_parent_row_id": 2_003,
                "_kat_row_id": 3,
                "scalar_value": null,
                "bytes_value": null,
                "enum_value": 77,
            },
            {
                "_kat_parent_row_id": 2_004,
                "_kat_row_id": 4,
                "scalar_value": null,
                "bytes_value": null,
                "enum_value": null,
            },
            {
                "_kat_parent_row_id": 2_005,
                "_kat_row_id": 5,
                "scalar_value": null,
                "bytes_value": null,
                "enum_value": null,
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             child._kat_parent_row_id as message_parent, \
             child.code, child.label, child.payload \
             from full_shape_root root \
             join full_shape_root_oneof_matrix oneof_row \
               on oneof_row._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_oneof_matrix_message_value child \
               on child._kat_parent_row_id = oneof_row._kat_row_id",
        )
        .await,
        json!([{
            "root_parent": 2_004,
            "message_parent": 4,
            "code": 41,
            "label": "message-variant",
            "payload": "00ff80",
        }])
    );
}

#[tokio::test]
async fn scalar_only_oneof_in_singular_message_stays_inline() {
    use generated_fixture_emitter::{append_inline_oneof_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{
        InlineOneofRoot, ScalarOneofOnly, scalar_oneof_only::Selected,
    };
    use protobuf_source::SpoolOptions;

    let cases = [
        (2_100, None),
        (
            2_101,
            Some(ScalarOneofOnly {
                selected: Some(Selected::Scalar(0)),
            }),
        ),
        (
            2_102,
            Some(ScalarOneofOnly {
                selected: Some(Selected::Payload(vec![0x00, 0xff, 0x80])),
            }),
        ),
        (
            2_103,
            Some(ScalarOneofOnly {
                selected: Some(Selected::Lifecycle(77)),
            }),
        ),
    ];
    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(2, 8))
        .expect("generated capture is valid");
    for (parent_row_id, nested) in cases {
        append_inline_oneof_root_root(&mut capture, parent_row_id, &InlineOneofRoot { nested })
            .expect("inline oneof fixture root appends");
    }

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let root_schema = parquet_arrow_schema(&dataset_path, "inline_oneof_root");
    assert_eq!(root_schema.fields().len(), 2);
    let nested = root_schema
        .field_with_name("nested")
        .expect("relation-free singular message stays inline");
    assert!(nested.is_nullable());
    let arrow_schema::DataType::Struct(fields) = nested.data_type() else {
        panic!("inline oneof wrapper must be an Arrow Struct");
    };
    assert_eq!(fields.len(), 3);
    for (actual, (expected_name, expected_type)) in fields.iter().zip([
        ("scalar", arrow_schema::DataType::Int64),
        ("payload", arrow_schema::DataType::Binary),
        ("lifecycle", arrow_schema::DataType::Int32),
    ]) {
        assert_eq!(actual.name(), expected_name);
        assert_eq!(actual.data_type(), &expected_type);
        assert!(actual.is_nullable());
    }

    let resolved = kat_datasource::resolve_dataset(&dataset_path)
        .expect("published inline oneof Dataset resolves");
    assert!(
        resolved
            .tables()
            .iter()
            .all(|table| table.name() != "inline_oneof_root_nested"),
        "scalar-only oneof wrapper must not produce a child relation"
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, nested from inline_oneof_root \
             order by _kat_parent_row_id",
        )
        .await,
        json!([
            { "_kat_parent_row_id": 2_100, "nested": null },
            {
                "_kat_parent_row_id": 2_101,
                "nested": { "scalar": 0, "payload": null, "lifecycle": null },
            },
            {
                "_kat_parent_row_id": 2_102,
                "nested": { "scalar": null, "payload": "00ff80", "lifecycle": null },
            },
            {
                "_kat_parent_row_id": 2_103,
                "nested": { "scalar": null, "payload": null, "lifecycle": 77 },
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, enum_number, enum_symbol \
             from protobuf_enum_symbol \
             order by enum_number",
        )
        .await,
        json!([
            {
                "origin_table": "inline_oneof_root",
                "origin_field_path": "nested.lifecycle",
                "enum_number": 0,
                "enum_symbol": "LIFECYCLE_UNSPECIFIED",
            },
            {
                "origin_table": "inline_oneof_root",
                "origin_field_path": "nested.lifecycle",
                "enum_number": 1,
                "enum_symbol": "LIFECYCLE_STARTED",
            },
            {
                "origin_table": "inline_oneof_root",
                "origin_field_path": "nested.lifecycle",
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
        ])
    );
}

fn parquet_arrow_schema(
    dataset_path: &std::path::Path,
    table_name: &str,
) -> arrow_schema::SchemaRef {
    parquet_arrow_metadata(dataset_path, table_name)
        .schema()
        .clone()
}

fn parquet_arrow_metadata(
    dataset_path: &std::path::Path,
    table_name: &str,
) -> parquet::arrow::arrow_reader::ArrowReaderMetadata {
    use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};

    let resolved = kat_datasource::resolve_dataset(dataset_path)
        .expect("published fixture Dataset resolves for Parquet metadata inspection");
    let table = resolved
        .tables()
        .iter()
        .find(|table| table.name() == table_name)
        .unwrap_or_else(|| panic!("published fixture has no table {table_name:?}"));
    let file = std::fs::File::open(table.path())
        .unwrap_or_else(|error| panic!("fixture table {table_name:?} opens: {error}"));
    ArrowReaderMetadata::load(&file, ArrowReaderOptions::new())
        .expect("fixture Parquet metadata loads")
}

#[tokio::test]
async fn byte_limit_alone_flushes_complete_variable_width_values() {
    use generated_fixture_emitter::{append_full_shape_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{FullShapeRoot, ScalarMatrix};
    use protobuf_source::SpoolOptions;

    let fixtures = [
        (5_000, "byte-threshold-first", vec![0x00, 0xff, 0x80, 0x01]),
        (5_001, "byte-threshold-second", vec![0x10, 0x20, 0x30, 0xfe]),
        (5_002, "byte-threshold-third", vec![0xaa, 0xbb, 0xcc, 0xdd]),
    ];
    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(100, 1))
        .expect("generated capture is valid");
    for (parent_row_id, label, payload) in &fixtures {
        append_full_shape_root_root(
            &mut capture,
            *parent_row_id,
            &FullShapeRoot {
                scalars: Some(ScalarMatrix {
                    string_value: (*label).to_string(),
                    bytes_value: payload.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .expect("variable-width fixture root appends");
    }

    let prepared = prepare_capture(capture);
    assert_eq!(
        prepared.preflighted_row_group_count("full_shape_root"),
        Some(3),
        "the byte threshold alone must preflight three bounded spool row groups"
    );
    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_prepared(prepared, &dataset_path);

    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");
    let rows = query_json(
        &context,
        "select _kat_parent_row_id, scalars \
         from full_shape_root order by _kat_parent_row_id",
    )
    .await;
    let variable_values = rows
        .as_array()
        .expect("fixture query returns an array")
        .iter()
        .map(|row| {
            json!({
                "_kat_parent_row_id": row["_kat_parent_row_id"],
                "string_value": row["scalars"]["string_value"],
                "bytes_value": row["scalars"]["bytes_value"],
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        Value::Array(variable_values),
        json!([
            {
                "_kat_parent_row_id": 5_000,
                "string_value": "byte-threshold-first",
                "bytes_value": "00ff8001",
            },
            {
                "_kat_parent_row_id": 5_001,
                "string_value": "byte-threshold-second",
                "bytes_value": "102030fe",
            },
            {
                "_kat_parent_row_id": 5_002,
                "string_value": "byte-threshold-third",
                "bytes_value": "aabbccdd",
            },
        ])
    );
}

#[tokio::test]
async fn repeated_and_singular_relations_preserve_identity_and_order_across_flushes() {
    use generated_fixture_emitter::{append_full_shape_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{
        FullShapeRoot, LeafValue, Lifecycle, RelationChild, RelationContainer, RepeatedMatrix,
    };
    use protobuf_source::SpoolOptions;

    let roots = [
        (
            3_000,
            FullShapeRoot {
                repeated_matrix: Some(RepeatedMatrix {
                    scalar_values: vec![11, 12, 13],
                    bytes_values: vec![vec![0, 0xff], vec![0x80]],
                    enum_values: vec![Lifecycle::Unspecified as i32, Lifecycle::Stopped as i32, 88],
                    message_values: vec![
                        LeafValue {
                            code: 31,
                            label: "first".to_string(),
                            payload: vec![0x31],
                        },
                        LeafValue {
                            code: 32,
                            label: "second".to_string(),
                            payload: vec![0x32, 0xff],
                        },
                    ],
                }),
                relation_container: Some(RelationContainer {
                    label: "container".to_string(),
                    children: vec![
                        RelationChild {
                            id: 7,
                            name: "child-a".to_string(),
                        },
                        RelationChild {
                            id: 8,
                            name: "child-b".to_string(),
                        },
                    ],
                }),
                ..Default::default()
            },
        ),
        (
            3_001,
            FullShapeRoot {
                repeated_matrix: Some(RepeatedMatrix {
                    scalar_values: vec![21],
                    bytes_values: vec![vec![0x21]],
                    enum_values: vec![Lifecycle::Started as i32],
                    message_values: vec![LeafValue {
                        code: 33,
                        label: "third".to_string(),
                        payload: vec![],
                    }],
                }),
                relation_container: Some(RelationContainer::default()),
                ..Default::default()
            },
        ),
        (3_002, FullShapeRoot::default()),
    ];
    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(1)).expect("generated capture is valid");
    for (parent_row_id, root) in &roots {
        append_full_shape_root_root(&mut capture, *parent_row_id, root)
            .expect("multi-parent fixture root appends");
    }

    let prepared = prepare_capture(capture);
    assert_eq!(
        prepared.preflighted_row_group_count("full_shape_root"),
        Some(3),
        "the one-row threshold must preflight one spool row group per root"
    );
    assert_eq!(
        prepared.preflighted_row_group_count("full_shape_root_repeated_matrix"),
        Some(2),
        "the one-row threshold must also bound a singular child relation"
    );
    assert_eq!(
        prepared.preflighted_row_group_count("full_shape_root_repeated_matrix_scalar_values"),
        Some(4),
        "the one-row threshold must also bound repeated descendants"
    );
    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_prepared(prepared, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    assert_eq!(
        query_json(
            &context,
            "select _kat_row_id, _kat_parent_row_id \
             from full_shape_root order by _kat_row_id",
        )
        .await,
        json!([
            { "_kat_row_id": 0, "_kat_parent_row_id": 3_000 },
            { "_kat_row_id": 1, "_kat_parent_row_id": 3_001 },
            { "_kat_row_id": 2, "_kat_parent_row_id": 3_002 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             matrix._kat_row_id as matrix_row_id, \
             matrix._kat_parent_row_id as matrix_parent \
             from full_shape_root root \
             join full_shape_root_repeated_matrix matrix \
               on matrix._kat_parent_row_id = root._kat_row_id \
             order by root._kat_parent_row_id",
        )
        .await,
        json!([
            { "root_parent": 3_000, "matrix_row_id": 0, "matrix_parent": 0 },
            { "root_parent": 3_001, "matrix_row_id": 1, "matrix_parent": 1 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             element._kat_parent_row_id as element_parent, \
             element._kat_repeated_index, element.value \
             from full_shape_root root \
             join full_shape_root_repeated_matrix matrix \
               on matrix._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_repeated_matrix_scalar_values element \
               on element._kat_parent_row_id = matrix._kat_row_id \
             order by root._kat_parent_row_id, element._kat_repeated_index",
        )
        .await,
        json!([
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 0, "value": 11 },
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 1, "value": 12 },
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 2, "value": 13 },
            { "root_parent": 3_001, "element_parent": 1, "_kat_repeated_index": 0, "value": 21 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             element._kat_parent_row_id as element_parent, \
             element._kat_repeated_index, element.value \
             from full_shape_root root \
             join full_shape_root_repeated_matrix matrix \
               on matrix._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_repeated_matrix_bytes_values element \
               on element._kat_parent_row_id = matrix._kat_row_id \
             order by root._kat_parent_row_id, element._kat_repeated_index",
        )
        .await,
        json!([
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 0, "value": "00ff" },
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 1, "value": "80" },
            { "root_parent": 3_001, "element_parent": 1, "_kat_repeated_index": 0, "value": "21" },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             element._kat_parent_row_id as element_parent, \
             element._kat_repeated_index, element.value \
             from full_shape_root root \
             join full_shape_root_repeated_matrix matrix \
               on matrix._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_repeated_matrix_enum_values element \
               on element._kat_parent_row_id = matrix._kat_row_id \
             order by root._kat_parent_row_id, element._kat_repeated_index",
        )
        .await,
        json!([
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 0, "value": 0 },
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 1, "value": 2 },
            { "root_parent": 3_000, "element_parent": 0, "_kat_repeated_index": 2, "value": 88 },
            { "root_parent": 3_001, "element_parent": 1, "_kat_repeated_index": 0, "value": 1 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             element._kat_parent_row_id as element_parent, \
             element._kat_repeated_index, element.code, element.label, element.payload \
             from full_shape_root root \
             join full_shape_root_repeated_matrix matrix \
               on matrix._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_repeated_matrix_message_values element \
               on element._kat_parent_row_id = matrix._kat_row_id \
             order by root._kat_parent_row_id, element._kat_repeated_index",
        )
        .await,
        json!([
            {
                "root_parent": 3_000,
                "element_parent": 0,
                "_kat_repeated_index": 0,
                "code": 31,
                "label": "first",
                "payload": "31",
            },
            {
                "root_parent": 3_000,
                "element_parent": 0,
                "_kat_repeated_index": 1,
                "code": 32,
                "label": "second",
                "payload": "32ff",
            },
            {
                "root_parent": 3_001,
                "element_parent": 1,
                "_kat_repeated_index": 0,
                "code": 33,
                "label": "third",
                "payload": "",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             container._kat_row_id as container_row_id, \
             container._kat_parent_row_id as container_parent, container.label \
             from full_shape_root root \
             join full_shape_root_relation_container container \
               on container._kat_parent_row_id = root._kat_row_id \
             order by root._kat_parent_row_id",
        )
        .await,
        json!([
            {
                "root_parent": 3_000,
                "container_row_id": 0,
                "container_parent": 0,
                "label": "container",
            },
            {
                "root_parent": 3_001,
                "container_row_id": 1,
                "container_parent": 1,
                "label": "",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             child._kat_parent_row_id as child_parent, \
             child._kat_repeated_index, child.id, child.name \
             from full_shape_root root \
             join full_shape_root_relation_container container \
               on container._kat_parent_row_id = root._kat_row_id \
             join full_shape_root_relation_container_children child \
               on child._kat_parent_row_id = container._kat_row_id \
             order by root._kat_parent_row_id, child._kat_repeated_index",
        )
        .await,
        json!([
            {
                "root_parent": 3_000,
                "child_parent": 0,
                "_kat_repeated_index": 0,
                "id": 7,
                "name": "child-a",
            },
            {
                "root_parent": 3_000,
                "child_parent": 0,
                "_kat_repeated_index": 1,
                "id": 8,
                "name": "child-b",
            },
        ])
    );

    for (table_name, expected) in [
        (
            "full_shape_root_repeated_matrix",
            vec![
                ("_kat_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
            ],
        ),
        (
            "full_shape_root_repeated_matrix_scalar_values",
            vec![
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
                ("value", arrow_schema::DataType::UInt64, false),
            ],
        ),
        (
            "full_shape_root_repeated_matrix_bytes_values",
            vec![
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
                ("value", arrow_schema::DataType::Binary, false),
            ],
        ),
        (
            "full_shape_root_repeated_matrix_enum_values",
            vec![
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
                ("value", arrow_schema::DataType::Int32, false),
            ],
        ),
        (
            "full_shape_root_repeated_matrix_message_values",
            vec![
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
                ("code", arrow_schema::DataType::Int32, false),
                ("label", arrow_schema::DataType::Utf8, false),
                ("payload", arrow_schema::DataType::Binary, false),
            ],
        ),
        (
            "full_shape_root_relation_container",
            vec![
                ("_kat_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("label", arrow_schema::DataType::Utf8, false),
            ],
        ),
        (
            "full_shape_root_relation_container_children",
            vec![
                ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
                ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
                ("id", arrow_schema::DataType::UInt32, false),
                ("name", arrow_schema::DataType::Utf8, false),
            ],
        ),
    ] {
        let table_schema = parquet_arrow_schema(&dataset_path, table_name);
        assert_flat_schema(table_schema.as_ref(), &expected);
    }
}

#[tokio::test]
async fn nested_repeated_relations_link_each_generation_to_its_direct_parent() {
    use generated_fixture_emitter::{append_deep_repeated_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{
        DeepRepeatedRoot, RelationChild, RelationContainer,
    };
    use protobuf_source::SpoolOptions;

    let roots = [
        (
            6_000,
            DeepRepeatedRoot {
                containers: vec![
                    RelationContainer {
                        label: "first".to_string(),
                        children: vec![
                            RelationChild {
                                id: 11,
                                name: "first-a".to_string(),
                            },
                            RelationChild {
                                id: 12,
                                name: "first-b".to_string(),
                            },
                        ],
                    },
                    RelationContainer {
                        label: "second".to_string(),
                        children: vec![RelationChild {
                            id: 21,
                            name: "second-a".to_string(),
                        }],
                    },
                ],
            },
        ),
        (
            6_001,
            DeepRepeatedRoot {
                containers: vec![RelationContainer {
                    label: "third".to_string(),
                    children: vec![
                        RelationChild {
                            id: 31,
                            name: "third-a".to_string(),
                        },
                        RelationChild {
                            id: 32,
                            name: "third-b".to_string(),
                        },
                    ],
                }],
            },
        ),
    ];
    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(1)).expect("generated capture is valid");
    for (parent_row_id, root) in &roots {
        append_deep_repeated_root_root(&mut capture, *parent_row_id, root)
            .expect("nested repeated fixture root appends");
    }

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    assert_flat_schema(
        parquet_arrow_schema(&dataset_path, "deep_repeated_root_containers").as_ref(),
        &[
            ("_kat_row_id", arrow_schema::DataType::UInt64, false),
            ("_kat_parent_row_id", arrow_schema::DataType::UInt64, false),
            ("_kat_repeated_index", arrow_schema::DataType::UInt64, false),
            ("label", arrow_schema::DataType::Utf8, false),
        ],
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             root._kat_row_id as root_row_id, \
             container._kat_row_id as container_row_id, \
             container._kat_parent_row_id as container_parent, \
             container._kat_repeated_index, container.label \
             from deep_repeated_root root \
             join deep_repeated_root_containers container \
               on container._kat_parent_row_id = root._kat_row_id \
             order by root._kat_parent_row_id, container._kat_repeated_index",
        )
        .await,
        json!([
            {
                "root_parent": 6_000,
                "root_row_id": 0,
                "container_row_id": 0,
                "container_parent": 0,
                "_kat_repeated_index": 0,
                "label": "first",
            },
            {
                "root_parent": 6_000,
                "root_row_id": 0,
                "container_row_id": 1,
                "container_parent": 0,
                "_kat_repeated_index": 1,
                "label": "second",
            },
            {
                "root_parent": 6_001,
                "root_row_id": 1,
                "container_row_id": 2,
                "container_parent": 1,
                "_kat_repeated_index": 0,
                "label": "third",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select root._kat_parent_row_id as root_parent, \
             root._kat_row_id as root_row_id, \
             container._kat_row_id as container_row_id, \
             child._kat_parent_row_id as child_parent, \
             child._kat_repeated_index, child.id, child.name \
             from deep_repeated_root root \
             join deep_repeated_root_containers container \
               on container._kat_parent_row_id = root._kat_row_id \
             join deep_repeated_root_containers_children child \
               on child._kat_parent_row_id = container._kat_row_id \
             order by root._kat_parent_row_id, container._kat_repeated_index, \
                      child._kat_repeated_index",
        )
        .await,
        json!([
            {
                "root_parent": 6_000,
                "root_row_id": 0,
                "container_row_id": 0,
                "child_parent": 0,
                "_kat_repeated_index": 0,
                "id": 11,
                "name": "first-a",
            },
            {
                "root_parent": 6_000,
                "root_row_id": 0,
                "container_row_id": 0,
                "child_parent": 0,
                "_kat_repeated_index": 1,
                "id": 12,
                "name": "first-b",
            },
            {
                "root_parent": 6_000,
                "root_row_id": 0,
                "container_row_id": 1,
                "child_parent": 1,
                "_kat_repeated_index": 0,
                "id": 21,
                "name": "second-a",
            },
            {
                "root_parent": 6_001,
                "root_row_id": 1,
                "container_row_id": 2,
                "child_parent": 2,
                "_kat_repeated_index": 0,
                "id": 31,
                "name": "third-a",
            },
            {
                "root_parent": 6_001,
                "root_row_id": 1,
                "container_row_id": 2,
                "child_parent": 2,
                "_kat_repeated_index": 1,
                "id": 32,
                "name": "third-b",
            },
        ])
    );
}

#[tokio::test]
async fn generated_incremental_relation_appenders_preserve_the_same_parent_child_contract() {
    use generated_fixture_emitter::{
        append_deep_repeated_root_containers_children_subtree,
        append_deep_repeated_root_containers_subtree, append_deep_repeated_root_incremental_root,
        new_protobuf_source_capture,
    };
    use proto::fixture::protobuf_source::valid::{
        DeepRepeatedRoot, RelationChild, RelationContainer,
    };
    use protobuf_source::SpoolOptions;

    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(1)).expect("generated capture is valid");
    let root_row_id = append_deep_repeated_root_incremental_root(
        &mut capture,
        7_000,
        &DeepRepeatedRoot { containers: vec![] },
    )
    .expect("incremental fixture root appends");
    let container_row_id = append_deep_repeated_root_containers_subtree(
        &mut capture,
        root_row_id,
        0,
        &RelationContainer {
            label: "incremental".to_string(),
            children: vec![],
        },
    )
    .expect("incremental container appends");
    append_deep_repeated_root_containers_children_subtree(
        &mut capture,
        container_row_id,
        0,
        &RelationChild {
            id: 42,
            name: "child".to_string(),
        },
    )
    .expect("incremental child appends");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("incremental fixture Dataset resolves");
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, _kat_repeated_index, id, name from deep_repeated_root_containers_children",
        )
        .await,
        json!([{
            "_kat_parent_row_id": container_row_id,
            "_kat_repeated_index": 0,
            "id": 42,
            "name": "child",
        }])
    );
}

#[tokio::test]
async fn enum_definitions_are_complete_and_unknown_numbers_remain_unmatched() {
    use generated_fixture_emitter::{append_full_shape_root_root, new_protobuf_source_capture};
    use proto::fixture::protobuf_source::valid::{
        FullShapeRoot, Lifecycle, OneofMatrix, RepeatedMatrix, ScalarMatrix, oneof_matrix::Selected,
    };
    use protobuf_source::SpoolOptions;

    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(1, 1))
        .expect("generated capture is valid");
    append_full_shape_root_root(
        &mut capture,
        4_000,
        &FullShapeRoot {
            scalars: Some(ScalarMatrix {
                lifecycle: Lifecycle::Started as i32,
                optional_lifecycle: Some(123),
                ..Default::default()
            }),
            oneof_matrix: Some(OneofMatrix {
                selected: Some(Selected::EnumValue(77)),
            }),
            repeated_matrix: Some(RepeatedMatrix {
                enum_values: vec![Lifecycle::Stopped as i32, 88],
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .expect("enum fixture root appends");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let definitions = parquet_arrow_schema(&dataset_path, "protobuf_enum_symbol");
    let expected_definition_fields = [
        ("origin_table", arrow_schema::DataType::Utf8),
        ("origin_field_path", arrow_schema::DataType::Utf8),
        ("enum_type_name", arrow_schema::DataType::Utf8),
        ("enum_number", arrow_schema::DataType::Int32),
        ("enum_symbol", arrow_schema::DataType::Utf8),
    ];
    assert_eq!(definitions.fields().len(), expected_definition_fields.len());
    for (actual, (expected_name, expected_type)) in
        definitions.fields().iter().zip(&expected_definition_fields)
    {
        assert_eq!(actual.name(), expected_name);
        assert_eq!(actual.data_type(), expected_type);
        assert!(
            !actual.is_nullable(),
            "definition column {expected_name:?} must be non-null"
        );
    }
    assert_eq!(
        query_json(
            &context,
            "select * from protobuf_enum_symbol \
             order by origin_table, origin_field_path, enum_number",
        )
        .await,
        json!([
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 0,
                "enum_symbol": "LIFECYCLE_UNSPECIFIED",
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 1,
                "enum_symbol": "LIFECYCLE_STARTED",
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.optional_lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 0,
                "enum_symbol": "LIFECYCLE_UNSPECIFIED",
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.optional_lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 1,
                "enum_symbol": "LIFECYCLE_STARTED",
            },
            {
                "origin_table": "full_shape_root",
                "origin_field_path": "scalars.optional_lifecycle",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
            {
                "origin_table": "full_shape_root_oneof_matrix",
                "origin_field_path": "enum_value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 0,
                "enum_symbol": "LIFECYCLE_UNSPECIFIED",
            },
            {
                "origin_table": "full_shape_root_oneof_matrix",
                "origin_field_path": "enum_value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 1,
                "enum_symbol": "LIFECYCLE_STARTED",
            },
            {
                "origin_table": "full_shape_root_oneof_matrix",
                "origin_field_path": "enum_value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
            {
                "origin_table": "full_shape_root_repeated_matrix_enum_values",
                "origin_field_path": "value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 0,
                "enum_symbol": "LIFECYCLE_UNSPECIFIED",
            },
            {
                "origin_table": "full_shape_root_repeated_matrix_enum_values",
                "origin_field_path": "value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 1,
                "enum_symbol": "LIFECYCLE_STARTED",
            },
            {
                "origin_table": "full_shape_root_repeated_matrix_enum_values",
                "origin_field_path": "value",
                "enum_type_name": "fixture.protobuf_source.valid.Lifecycle",
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select origin_table, origin_field_path, enum_number, count(*) as copies \
             from protobuf_enum_symbol \
             group by origin_table, origin_field_path, enum_number \
             having count(*) <> 1",
        )
        .await,
        json!([])
    );
    assert_eq!(
        query_json(
            &context,
            "select occurrence.enum_value as enum_number, definition.enum_symbol \
             from full_shape_root_oneof_matrix occurrence \
             left join protobuf_enum_symbol definition \
               on definition.origin_table = 'full_shape_root_oneof_matrix' \
              and definition.origin_field_path = 'enum_value' \
              and definition.enum_number = occurrence.enum_value",
        )
        .await,
        json!([{ "enum_number": 77, "enum_symbol": null }])
    );
    assert_eq!(
        query_json(
            &context,
            "select occurrence._kat_repeated_index, occurrence.value as enum_number, \
             definition.enum_symbol \
             from full_shape_root_repeated_matrix_enum_values occurrence \
             left join protobuf_enum_symbol definition \
               on definition.origin_table = \
                    'full_shape_root_repeated_matrix_enum_values' \
              and definition.origin_field_path = 'value' \
              and definition.enum_number = occurrence.value \
             order by occurrence._kat_repeated_index",
        )
        .await,
        json!([
            {
                "_kat_repeated_index": 0,
                "enum_number": 2,
                "enum_symbol": "LIFECYCLE_STOPPED",
            },
            {
                "_kat_repeated_index": 1,
                "enum_number": 88,
                "enum_symbol": null,
            },
        ])
    );
}
