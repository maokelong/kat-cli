use arrow_array::RecordBatch;
use arrow_json::writer::{JsonArray, WriterBuilder};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use prost::Message;
use prost_types::FileDescriptorSet;
use serde_json::{Value, json};
use tempfile::tempdir;
use url::Url;

use crate as kat_datasource;
use crate::{
    dataset_writer, generated_fixture_emitter, proto, protobuf_source, protobuf_source_codegen,
};

mod roundtrip;

use protobuf_source_codegen::{RootSpec, compile};

const RESERVED_TABLES: &[&str] = &[
    "protobuf_enum_symbol",
    "profiler_payload_occurrence",
    "clock_domain",
    "clock_snapshot",
    "sched_switch",
];

#[tokio::test]
async fn generated_scalar_presence_bytes_and_empty_root_round_trip_through_dataset() {
    use generated_fixture_emitter::{
        append_empty_root_root, append_full_shape_root_root, new_protobuf_source_capture,
    };
    use proto::fixture::protobuf_source::valid::{
        EmptyRoot, FullShapeRoot, Lifecycle, ScalarMatrix,
    };
    use protobuf_source::SpoolOptions;

    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(2, 10))
        .expect("generated capture is valid");
    append_full_shape_root_root(
        &mut capture,
        71,
        &FullShapeRoot {
            scalars: Some(ScalarMatrix {
                double_value: 1.5,
                float_value: 2.5,
                int32_value: -3,
                int64_value: -4,
                uint32_value: 5,
                uint64_value: 6,
                sint32_value: -7,
                sint64_value: -8,
                fixed32_value: 9,
                fixed64_value: 10,
                sfixed32_value: -11,
                sfixed64_value: -12,
                bool_value: true,
                string_value: "scalar-row".to_string(),
                bytes_value: vec![0, 0xff, 0x80],
                lifecycle: Lifecycle::Started as i32,
                optional_count: None,
                optional_label: Some("present".to_string()),
                optional_bytes: Some(vec![0xff, 0x00]),
                optional_lifecycle: None,
            }),
            nullable_outer: None,
            oneof_matrix: None,
            repeated_matrix: None,
            relation_container: None,
        },
    )
    .expect("typed scalar root appends");
    append_empty_root_root(&mut capture, 72, &EmptyRoot {}).expect("empty root appends");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);

    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");
    let scalar_rows = query_json(
        &context,
        "select _kat_parent_row_id, scalars, nullable_outer from full_shape_root",
    )
    .await;
    assert_eq!(
        scalar_rows,
        json!([{
            "_kat_parent_row_id": 71,
            "scalars": {
                "double_value": 1.5,
                "float_value": 2.5,
                "int32_value": -3,
                "int64_value": -4,
                "uint32_value": 5,
                "uint64_value": 6,
                "sint32_value": -7,
                "sint64_value": -8,
                "fixed32_value": 9,
                "fixed64_value": 10,
                "sfixed32_value": -11,
                "sfixed64_value": -12,
                "bool_value": true,
                "string_value": "scalar-row",
                "bytes_value": "00ff80",
                "lifecycle": Lifecycle::Started as i32,
                "optional_count": null,
                "optional_label": "present",
                "optional_bytes": "ff00",
                "optional_lifecycle": null,
            },
            "nullable_outer": null,
        }])
    );

    let empty_rows = query_json(&context, "select _kat_parent_row_id from empty_root").await;
    assert_eq!(empty_rows, json!([{ "_kat_parent_row_id": 72 }]));
    let empty_table = context
        .table("empty_root")
        .await
        .expect("empty root table is registered");
    let empty_schema = empty_table.schema();
    assert_eq!(empty_schema.fields().len(), 1);
    assert_eq!(empty_schema.field(0).name(), "_kat_parent_row_id");
    assert_eq!(
        empty_schema.field(0).data_type(),
        &arrow_schema::DataType::UInt64
    );
    assert!(!empty_schema.field(0).is_nullable());
    assert!(
        !dataset_path
            .join("tables/alpha_shared_root.parquet")
            .exists(),
        "registered but inactive relations stay unpublished"
    );
}

#[tokio::test]
async fn canonical_fqns_and_prost_naming_round_trip_through_typed_emitters() {
    use generated_fixture_emitter::{
        append_alpha_shared_root_root, append_beta_shared_root_root,
        append_keyword_acronym_root_root, append_nested_field_name_root_root,
        append_nested_oneof_root_root, append_oneof_nested_name_root_root,
        append_uppercase_field_root_root, new_protobuf_source_capture,
    };
    use proto::fixture::protobuf_source::{alpha, beta, illegal_field_names, naming};
    use protobuf_source::SpoolOptions;

    let mut capture = new_protobuf_source_capture(SpoolOptions::with_limits(2, 10))
        .expect("generated capture is valid");
    append_alpha_shared_root_root(&mut capture, 101, &alpha::SharedRoot { alpha_value: -7 })
        .expect("alpha SharedRoot appends");
    append_beta_shared_root_root(
        &mut capture,
        102,
        &beta::SharedRoot {
            beta_value: "beta-value".to_string(),
        },
    )
    .expect("beta SharedRoot appends");
    append_keyword_acronym_root_root(
        &mut capture,
        103,
        &naming::KeywordAcronymRoot {
            r#type: 3,
            r#match: "matched".to_string(),
            r#async: true,
            r#gen: "generated".to_string(),
            self_: "self-value".to_string(),
            http_url_payload: Some(naming::HttpurlPayload {
                endpoint_url: "https://fixture.invalid".to_string(),
            }),
            gpu_cpu_stats: vec![
                naming::GpucpuStats {
                    gpu_cycles: 11,
                    cpu_cycles: 12,
                },
                naming::GpucpuStats {
                    gpu_cycles: 21,
                    cpu_cycles: 22,
                },
                naming::GpucpuStats {
                    gpu_cycles: 31,
                    cpu_cycles: 32,
                },
            ],
            http2_url_stats: Some(naming::Http2urlStats { request_count: 4 }),
            field_0_name6: "field-zero-six".to_string(),
            field_name18: "field-eighteen".to_string(),
        },
    )
    .expect("keyword and acronym-heavy root appends");
    append_nested_oneof_root_root(
        &mut capture,
        104,
        &naming::NestedOneofRoot {
            selected: Some(naming::nested_oneof_root::Selected::Payload(
                naming::nested_oneof_root::Payload {
                    nested_value: "nested-payload".to_string(),
                },
            )),
        },
    )
    .expect("nested same-name oneof root appends");
    append_oneof_nested_name_root_root(
        &mut capture,
        105,
        &naming::OneofNestedNameRoot {
            choice: Some(naming::oneof_nested_name_root::ChoiceOneOf::NestedChoice(
                naming::oneof_nested_name_root::Choice {
                    value: "choice-value".to_string(),
                },
            )),
        },
    )
    .expect("oneof/nested type-name collision root appends");
    append_uppercase_field_root_root(
        &mut capture,
        106,
        &illegal_field_names::UppercaseFieldRoot {
            camel_case: "case-preserved".to_string(),
        },
    )
    .expect("legal non-snake inline field appends through its prost binding");
    append_nested_field_name_root_root(
        &mut capture,
        107,
        &illegal_field_names::NestedFieldNameRoot {
            nested_fields: Some(illegal_field_names::NestedFieldNames {
                camel_case: "nested-case-preserved".to_string(),
                kat_parent_row_id: "nested-data-not-relationship".to_string(),
            }),
        },
    )
    .expect("nested data fields may use names reserved only at relation level");

    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_capture(capture, &dataset_path);
    let resolved = kat_datasource::resolve_dataset(&dataset_path)
        .expect("formal Dataset resolver sees naming fixture tables");
    assert!(
        resolved
            .tables()
            .iter()
            .all(|table| table.name() != "protobuf_enum_symbol"),
        "active relations without enum origins must not publish enum definitions"
    );
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("formal Dataset resolver tables register in DataFusion");

    let shared_roots = query_json(
        &context,
        "select a.alpha_value, b.beta_value \
         from alpha_shared_root a cross join beta_shared_root b",
    )
    .await;
    assert_eq!(
        shared_roots,
        json!([{ "alpha_value": -7, "beta_value": "beta-value" }])
    );

    let naming_root = query_json(
        &context,
        "select _kat_parent_row_id, \"type\" as type_value, \"match\" as match_value, \
         \"async\" as async_value, \"gen\" as gen_value, \"self\" as self_value, \
         http_url_payload, http2_url_stats, field_0_name6, field_name18 \
         from keyword_acronym_root",
    )
    .await;
    assert_eq!(
        naming_root,
        json!([{
            "_kat_parent_row_id": 103,
            "type_value": 3,
            "match_value": "matched",
            "async_value": true,
            "gen_value": "generated",
            "self_value": "self-value",
            "http_url_payload": { "endpoint_url": "https://fixture.invalid" },
            "http2_url_stats": { "request_count": 4 },
            "field_0_name6": "field-zero-six",
            "field_name18": "field-eighteen",
        }])
    );
    let acronym_children = query_json(
        &context,
        "select _kat_repeated_index, gpu_cycles, cpu_cycles \
         from keyword_acronym_root_gpu_cpu_stats order by _kat_repeated_index",
    )
    .await;
    assert_eq!(
        acronym_children,
        json!([
            { "_kat_repeated_index": 0, "gpu_cycles": 11, "cpu_cycles": 12 },
            { "_kat_repeated_index": 1, "gpu_cycles": 21, "cpu_cycles": 22 },
            { "_kat_repeated_index": 2, "gpu_cycles": 31, "cpu_cycles": 32 },
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select nested_value from nested_oneof_root_payload"
        )
        .await,
        json!([{ "nested_value": "nested-payload" }])
    );
    assert_eq!(
        query_json(
            &context,
            "select value from oneof_nested_name_root_nested_choice",
        )
        .await,
        json!([{ "value": "choice-value" }])
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, \"CamelCase\" from uppercase_field_root",
        )
        .await,
        json!([{ "_kat_parent_row_id": 106, "CamelCase": "case-preserved" }])
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, nested_fields from nested_field_name_root",
        )
        .await,
        json!([{
            "_kat_parent_row_id": 107,
            "nested_fields": {
                "CamelCase": "nested-case-preserved",
                "_kat_parent_row_id": "nested-data-not-relationship",
            },
        }])
    );
}

#[test]
fn capture_preflight_failure_does_not_create_or_publish_the_dataset_target() {
    use generated_fixture_emitter::new_protobuf_source_capture;
    use protobuf_source::{EstimatedRow, RelationSlot, SpoolOptions};

    #[derive(serde::Serialize)]
    struct IncompleteRow;

    impl EstimatedRow for IncompleteRow {
        fn estimated_bytes(&self) -> anyhow::Result<usize> {
            Ok(0)
        }
    }

    let directory = tempdir().expect("temporary parent directory is created");
    let dataset_path = directory.path().join("must_not_exist");
    let mut capture =
        new_protobuf_source_capture(SpoolOptions::new(2)).expect("generated capture is valid");
    capture
        .append_row(RelationSlot::new(0), &IncompleteRow)
        .expect_err("incomplete row poisons preflight capture");
    try_publish_capture(capture, &dataset_path)
        .expect_err("poisoned capture fails before Dataset begin");

    assert!(
        !dataset_path.exists(),
        "preflight failure must not create the Dataset target"
    );
    assert!(
        !dataset_path.join(".kat-dataset").exists(),
        "preflight failure must not publish a Dataset marker"
    );
}

#[test]
fn planner_scopes_unsupported_shapes_to_the_registered_root_closure() {
    let descriptors = fixture_descriptors();
    let cases = [
        (
            "fixture.protobuf_source.unsupported.extension_shape.ExtensionReachableRoot",
            "extension_reachable_root",
            "fixture.protobuf_source.unsupported.extension_shape.ExtensionUnreachableRoot",
            "extension_unreachable_root",
            "extension",
            "container.subject",
            "fixture.protobuf_source.unsupported.extension_shape.ExtensionContainer",
        ),
        (
            "fixture.protobuf_source.unsupported.group_shape.GroupReachableRoot",
            "group_reachable_root",
            "fixture.protobuf_source.unsupported.group_shape.GroupUnreachableRoot",
            "group_unreachable_root",
            "group",
            "legacypayload",
            "fixture.protobuf_source.unsupported.group_shape.GroupReachableRoot",
        ),
        (
            "fixture.protobuf_source.unsupported.required_shape.RequiredReachableRoot",
            "required_reachable_root",
            "fixture.protobuf_source.unsupported.required_shape.RequiredUnreachableRoot",
            "required_unreachable_root",
            "required",
            "value",
            "fixture.protobuf_source.unsupported.required_shape.RequiredReachableRoot",
        ),
        (
            "fixture.protobuf_source.unsupported.enum_alias_shape.EnumAliasReachableRoot",
            "enum_alias_reachable_root",
            "fixture.protobuf_source.unsupported.enum_alias_shape.EnumAliasUnreachableRoot",
            "enum_alias_unreachable_root",
            "alias",
            "status",
            "fixture.protobuf_source.unsupported.enum_alias_shape.EnumAliasReachableRoot",
        ),
        (
            "fixture.protobuf_source.unsupported.map_shape.MapReachableRoot",
            "map_reachable_root",
            "fixture.protobuf_source.unsupported.map_shape.MapUnreachableRoot",
            "map_unreachable_root",
            "map",
            "values",
            "fixture.protobuf_source.unsupported.map_shape.MapReachableRoot",
        ),
        (
            "fixture.protobuf_source.unsupported.recursive_shape.RecursiveReachableRoot",
            "recursive_reachable_root",
            "fixture.protobuf_source.unsupported.recursive_shape.RecursiveUnreachableRoot",
            "recursive_unreachable_root",
            "recursive",
            "node.next",
            "fixture.protobuf_source.unsupported.recursive_shape.RecursiveNode",
        ),
    ];

    for (
        reachable,
        reachable_table,
        unreachable,
        unreachable_table,
        reason,
        field_path,
        containing_message,
    ) in cases
    {
        let message = compile_error(&descriptors, RootSpec::new(reachable, reachable_table));
        assert!(
            message.contains(reachable),
            "missing root context: {message}"
        );
        assert!(
            message.to_ascii_lowercase().contains(reason),
            "missing unsupported-shape reason {reason:?}: {message}"
        );
        assert!(
            message.contains(field_path),
            "missing traversal field path {field_path:?}: {message}"
        );
        assert!(
            message.contains(containing_message),
            "missing containing message {containing_message:?}: {message}"
        );
        if reason == "extension" {
            assert!(
                message.contains(
                    "fixture.protobuf_source.unsupported.extension_shape.ExtensionSubject"
                ),
                "missing unsupported target message: {message}"
            );
        }
        compile(
            &descriptors,
            &[RootSpec::new(unreachable, unreachable_table)],
        )
        .expect("unsupported shapes outside the registered root closure do not block planning");
    }
}

#[test]
fn planner_rejects_a_synthetic_map_entry_bound_directly_as_a_root() {
    let descriptors = fixture_descriptors();
    let root_fqn = "fixture.protobuf_source.unsupported.map_shape.MapReachableRoot.ValuesEntry";

    let message = compile_error(&descriptors, RootSpec::new(root_fqn, "map_entry_root"));

    assert!(
        message.contains(root_fqn),
        "missing root context: {message}"
    );
    assert!(
        message.contains("ValuesEntry"),
        "missing message context: {message}"
    );
    assert!(
        message.to_ascii_lowercase().contains("map entry")
            || message.to_ascii_lowercase().contains("map-entry"),
        "missing synthetic map-entry reason: {message}"
    );
}

#[test]
fn planner_rejects_every_reserved_root_table_name() {
    let descriptors = fixture_descriptors();
    let root_fqn = "fixture.protobuf_source.valid.EmptyRoot";

    for table_name in RESERVED_TABLES {
        let message = compile_error(&descriptors, RootSpec::new(root_fqn, table_name));
        assert!(
            message.contains(root_fqn),
            "missing root context: {message}"
        );
        assert!(
            message.contains(table_name) && message.contains("reserved"),
            "reserved root table {table_name:?} should be rejected: {message}"
        );
    }
}

#[test]
fn planner_rejects_repeated_value_relations_that_form_reserved_table_names() {
    let descriptors = fixture_descriptors();
    let cases = [
        (
            "fixture.protobuf_source.reserved_relation_names.EnumSymbolChildRoot",
            "protobuf_enum",
            "protobuf_enum_symbol",
            "symbol",
        ),
        (
            "fixture.protobuf_source.reserved_relation_names.ProfilerOccurrenceChildRoot",
            "profiler",
            "profiler_payload_occurrence",
            "payload_occurrence",
        ),
        (
            "fixture.protobuf_source.reserved_relation_names.ClockDomainChildRoot",
            "clock",
            "clock_domain",
            "domain",
        ),
        (
            "fixture.protobuf_source.reserved_relation_names.ClockSnapshotChildRoot",
            "clock",
            "clock_snapshot",
            "snapshot",
        ),
        (
            "fixture.protobuf_source.reserved_relation_names.SchedSwitchChildRoot",
            "sched",
            "sched_switch",
            "switch",
        ),
    ];

    for (root_fqn, root_table, reserved_relation, field) in cases {
        let message = compile_error(&descriptors, RootSpec::new(root_fqn, root_table));
        assert!(
            message.contains(root_fqn),
            "missing root context: {message}"
        );
        assert!(
            message.contains(reserved_relation)
                && message.contains(field)
                && message.contains("reserved"),
            "repeated value path should not form {reserved_relation:?}: {message}"
        );
    }
}

#[test]
fn planner_recursive_error_identifies_root_containing_message_and_field_path() {
    let descriptors = fixture_descriptors();
    let root_fqn = "fixture.protobuf_source.unsupported.recursive_shape.RecursiveReachableRoot";
    let containing_message = "fixture.protobuf_source.unsupported.recursive_shape.RecursiveNode";

    let message = compile_error(
        &descriptors,
        RootSpec::new(root_fqn, "recursive_reachable_root"),
    );

    assert!(
        message.contains(root_fqn),
        "missing root context: {message}"
    );
    assert!(
        message.contains(containing_message),
        "missing containing-message context: {message}"
    );
    assert!(
        message.contains("node.next"),
        "missing field path: {message}"
    );
    assert!(
        message.contains("recursive"),
        "missing recursive-shape reason: {message}"
    );

    compile(
        &descriptors,
        &[RootSpec::new(
            "fixture.protobuf_source.unsupported.recursive_shape.RecursiveUnreachableRoot",
            "non_recursive_root",
        )],
    )
    .expect("recursion outside the registered root closure must not block compilation");
}

#[test]
fn planner_rejects_normalized_relation_name_collisions() {
    let descriptors = fixture_descriptors();
    let root_fqn = "fixture.protobuf_source.name_collision.NameCollisionRoot";

    let message = compile_error(&descriptors, RootSpec::new(root_fqn, "collision_root"));

    assert!(
        message.contains(root_fqn),
        "missing root context: {message}"
    );
    assert!(
        message.contains("collision_root_foo_bar") && message.contains("collides"),
        "missing deterministic normalized-name collision: {message}"
    );
}

#[test]
fn planner_requires_canonical_exact_root_fqns_and_distinguishes_same_short_names() {
    let descriptors = fixture_descriptors();
    let canonical = "fixture.protobuf_source.alpha.SharedRoot";

    let leading_dot = compile_error(
        &descriptors,
        RootSpec::new(
            ".fixture.protobuf_source.alpha.SharedRoot",
            "leading_dot_root",
        ),
    );
    assert!(
        leading_dot.contains("canonical") && leading_dot.contains("leading dot"),
        "leading-dot FQN should be rejected: {leading_dot}"
    );

    let missing = compile_error(
        &descriptors,
        RootSpec::new("fixture.protobuf_source.alpha.MissingRoot", "missing_root"),
    );
    assert!(
        missing.contains("does not identify a message"),
        "missing canonical FQN should be rejected: {missing}"
    );

    let _generated_source = compile(
        &descriptors,
        &[
            RootSpec::new(canonical, "alpha_contract_root"),
            RootSpec::new(
                "fixture.protobuf_source.beta.SharedRoot",
                "beta_contract_root",
            ),
        ],
    )
    .expect("canonical FQNs distinguish equal short message names across packages")
    .into_source();
}

#[test]
fn enum_symbol_accessor_reuses_descriptor_validation_and_requires_a_safe_name() {
    let descriptors = fixture_descriptors();
    let root = RootSpec::new("fixture.protobuf_source.valid.EmptyRoot", "empty_root");
    let enum_fqn = "fixture.protobuf_source.valid.Lifecycle";

    let _generated_source = compile(&descriptors, &[root])
        .expect("fixture root compiles")
        .with_enum_symbol_accessor(&descriptors, enum_fqn, "fixture_lifecycle_symbols")
        .expect("canonical non-aliased enum and safe accessor compile")
        .into_source();

    for invalid_name in ["NotSnake", "two__segments", "fn"] {
        let message = compile(&descriptors, &[root])
            .expect("fixture root compiles")
            .with_enum_symbol_accessor(&descriptors, enum_fqn, invalid_name)
            .expect_err("unsafe generated Rust accessor name is rejected")
            .to_string();
        assert!(
            message.contains(invalid_name) && message.contains("lower_snake"),
            "missing accessor-name diagnostic: {message}"
        );
    }

    let aliased = "fixture.protobuf_source.unsupported.enum_alias_shape.AliasedStatus";
    let message = compile(&descriptors, &[root])
        .expect("fixture root compiles")
        .with_enum_symbol_accessor(&descriptors, aliased, "aliased_status_symbols")
        .expect_err("standalone enum accessor shares root-plan alias rejection")
        .to_string();
    assert!(
        message.contains(aliased) && message.contains("aliases"),
        "missing shared enum-alias diagnostic: {message}"
    );
}

#[test]
fn planner_rejects_illegal_root_and_field_names() {
    let descriptors = fixture_descriptors();
    let valid_root = "fixture.protobuf_source.valid.EmptyRoot";

    for invalid_table in ["", "Uppercase", "two__segments", "nul"] {
        let message = compile_error(&descriptors, RootSpec::new(valid_root, invalid_table));
        assert!(
            message.contains(valid_root)
                && (message.contains("table-name contract") || message.contains("illegal")),
            "illegal root table {invalid_table:?} should fail with context: {message}"
        );
    }

    let root_fqn = "fixture.protobuf_source.illegal_field_names.ReservedRelationshipFieldRoot";
    let field_name = "_kat_parent_row_id";
    let message = compile_error(&descriptors, RootSpec::new(root_fqn, "illegal_field_root"));
    assert!(
        message.contains(root_fqn),
        "missing root context: {message}"
    );
    assert!(
        message.contains(field_name) && message.contains("illegal or reserved"),
        "illegal field {field_name:?} should fail with context: {message}"
    );

    let uppercase_repeated_root =
        "fixture.protobuf_source.illegal_field_names.UppercaseRepeatedFieldRoot";
    let message = compile_error(
        &descriptors,
        RootSpec::new(uppercase_repeated_root, "uppercase_repeated_root"),
    );
    assert!(
        message.contains(uppercase_repeated_root)
            && message.contains("uppercase_repeated_root_CamelCase")
            && message.contains("illegal"),
        "a non-snake field is legal inline but cannot form an illegal relation table: {message}"
    );
}

fn compile_error(descriptors: &FileDescriptorSet, root: RootSpec<'_>) -> String {
    match compile(descriptors, &[root]) {
        Ok(_) => panic!("fixture root should be rejected at compile time"),
        Err(error) => error.to_string(),
    }
}

fn fixture_descriptors() -> FileDescriptorSet {
    FileDescriptorSet::decode(
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "/protobuf_source_fixture/all_descriptors.bin"
        ))
        .as_slice(),
    )
    .expect("build-time synthetic protobuf descriptors decode")
}

fn publish_capture(capture: protobuf_source::SourceTableCapture, dataset_path: &std::path::Path) {
    try_publish_capture(capture, dataset_path)
        .expect("protobuf Source capture preflights and publishes");
}

fn try_publish_capture(
    capture: protobuf_source::SourceTableCapture,
    dataset_path: &std::path::Path,
) -> anyhow::Result<()> {
    let prepared = capture.finish()?;
    try_publish_prepared(prepared, dataset_path)
}

fn prepare_capture(
    capture: protobuf_source::SourceTableCapture,
) -> protobuf_source::PreparedSourceTables {
    capture
        .finish()
        .expect("protobuf Source capture passes preflight")
}

fn publish_prepared(
    prepared: protobuf_source::PreparedSourceTables,
    dataset_path: &std::path::Path,
) {
    try_publish_prepared(prepared, dataset_path).expect("prepared Source tables publish");
}

fn try_publish_prepared(
    prepared: protobuf_source::PreparedSourceTables,
    dataset_path: &std::path::Path,
) -> anyhow::Result<()> {
    use dataset_writer::{DatasetWriteTarget, DatasetWriter};

    // 临时 spool 已完整关闭、读取并校验，才授权创建目标目录。
    let mut writer = DatasetWriter::begin(DatasetWriteTarget::write_to_empty(dataset_path))?;
    prepared.write_into(&mut writer)?;
    writer.finish()?;
    Ok(())
}

async fn register_resolved_dataset(
    dataset_path: &std::path::Path,
) -> anyhow::Result<SessionContext> {
    let resolved = kat_datasource::resolve_dataset(dataset_path)?;
    let context = SessionContext::new();
    for table in resolved.tables() {
        let url = Url::from_file_path(table.path()).map_err(|()| {
            anyhow::anyhow!(
                "fixture table path cannot be converted to a file URL: {}",
                table.path().display()
            )
        })?;
        context
            .register_parquet(table.name(), url.as_str(), ParquetReadOptions::default())
            .await?;
    }
    Ok(context)
}

async fn query_json(context: &SessionContext, sql: &str) -> Value {
    let batches = context
        .sql(sql)
        .await
        .expect("fixture SQL plans")
        .collect()
        .await
        .expect("fixture SQL executes");
    record_batches_to_json(&batches)
}

fn record_batches_to_json(batches: &[RecordBatch]) -> Value {
    let batch_refs = batches.iter().collect::<Vec<_>>();
    let mut buffer = Vec::new();
    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, JsonArray>(&mut buffer);
    writer
        .write_batches(&batch_refs)
        .expect("fixture query batches encode as JSON");
    writer.finish().expect("fixture JSON writer finishes");
    drop(writer);
    serde_json::from_slice(&buffer).expect("fixture query JSON parses")
}
