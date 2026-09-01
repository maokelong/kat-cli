use std::{path::PathBuf, sync::OnceLock};

use prost_types::FileDescriptorSet;

// 构建期 compiler 是被测边界；path include 避免把它扩大为运行时公共 API。
#[path = "../build/protobuf_source_codegen/mod.rs"]
mod protobuf_source_codegen;
#[path = "../src/relation_name.rs"]
mod relation_name;

use protobuf_source_codegen::{RootSpec, compile_for_profiler_capture};

const PACKAGE: &str = "fixture.protobuf_source.contract";
const VALID_ROOT: &str = "fixture.protobuf_source.contract.ValidRoot";
const PROTO2_VALID_ROOT: &str = "fixture.protobuf_source.contract_proto2.ValidRoot";
const MAP_ROOT: &str = "fixture.protobuf_source.contract.MapRoot";
const RECURSIVE_ROOT: &str = "fixture.protobuf_source.contract.RecursiveRoot";
const ALIAS_ROOT: &str = "fixture.protobuf_source.contract.AliasRoot";
const REQUIRED_ROOT: &str = "fixture.protobuf_source.contract_proto2.RequiredRoot";
const GROUP_ROOT: &str = "fixture.protobuf_source.contract_proto2.GroupRoot";
const EXTENSION_ROOT: &str = "fixture.protobuf_source.contract_proto2.ExtensionRoot";
const RESERVED_CHILD_ROOT: &str = "fixture.protobuf_source.contract.ReservedChildRoot";

#[test]
fn valid_registered_closure_compiles_without_validating_unreachable_invalid_shapes() {
    let source =
        compile_for_profiler_capture(descriptors(), &[RootSpec::new(VALID_ROOT, "valid_root")])
            .expect("the valid registered closure compiles")
            .into_source();

    for expected in [
        "\"valid_root\"",
        "\"valid_root_samples\"",
        "\"valid_root_children\"",
        "\"fixture.protobuf_source.contract.Lifecycle\"",
    ] {
        assert!(
            source.contains(expected),
            "generated source must contain {expected:?}"
        );
    }
}

#[test]
fn unsupported_reachable_shapes_report_root_message_field_and_all_failures() {
    let cases = [
        (
            MAP_ROOT,
            "map_root",
            "fixture.protobuf_source.contract.MapRoot",
            "values",
            "protobuf map fields are unsupported",
            None,
        ),
        (
            RECURSIVE_ROOT,
            "recursive_root",
            "fixture.protobuf_source.contract.RecursiveNode",
            "node.next",
            "recursive message edge is unsupported",
            None,
        ),
        (
            ALIAS_ROOT,
            "alias_root",
            "fixture.protobuf_source.contract.AliasRoot",
            "status",
            "uses aliases, which are unsupported",
            None,
        ),
        (
            REQUIRED_ROOT,
            "required_root",
            "fixture.protobuf_source.contract_proto2.RequiredRoot",
            "value",
            "proto2 required fields are unsupported",
            None,
        ),
        (
            GROUP_ROOT,
            "group_root",
            "fixture.protobuf_source.contract_proto2.GroupRoot",
            "legacypayload",
            "protobuf group fields are unsupported",
            None,
        ),
        (
            EXTENSION_ROOT,
            "extension_root",
            "fixture.protobuf_source.contract_proto2.ExtensionContainer",
            "container.subject",
            "reachable protobuf extensions are unsupported",
            Some("fixture.protobuf_source.contract_proto2.ExtensionSubject"),
        ),
    ];
    let roots = cases
        .iter()
        .map(|(root, relation, ..)| RootSpec::new(root, relation))
        .collect::<Vec<_>>();
    let individual = cases
        .iter()
        .map(
            |(root, relation, containing_message, field_path, reason, target_message)| {
                let diagnostic = compile_error(&[RootSpec::new(root, relation)]);
                assert_eq!(diagnostic.lines().count(), 1, "{diagnostic}");
                for expected in [
                    format!("protobuf root {root:?}"),
                    format!("message {containing_message:?}"),
                    format!("field {field_path:?}"),
                    (*reason).to_owned(),
                ] {
                    assert!(
                        diagnostic.contains(&expected),
                        "missing diagnostic fact {expected:?}: {diagnostic}"
                    );
                }
                if let Some(target_message) = target_message {
                    assert!(
                        diagnostic.contains(&format!("target message {target_message:?}")),
                        "missing extension target message: {diagnostic}"
                    );
                }
                diagnostic
            },
        )
        .collect::<Vec<_>>();
    let error = compile_error(&roots);
    assert_eq!(
        error.lines().collect::<Vec<_>>(),
        individual.iter().map(String::as_str).collect::<Vec<_>>()
    );

    let map_entry_error = compile_error(&[RootSpec::new(
        &format!("{PACKAGE}.MapRoot.ValuesEntry"),
        "map_entry",
    )]);
    assert!(map_entry_error.contains(
        "synthetic protobuf map-entry messages cannot be published as roots or relations"
    ));
}

#[test]
fn canonical_fqns_and_relation_names_fail_closed() {
    for root_fqn in [
        format!(".{PACKAGE}.ValidRoot"),
        "ValidRoot".to_owned(),
        format!("{PACKAGE}.MissingRoot"),
    ] {
        let error = compile_error(&[RootSpec::new(&root_fqn, "valid_root")]);
        assert!(
            error.contains(&format!("protobuf root {root_fqn:?}")),
            "diagnostic must identify {root_fqn:?}"
        );
        assert!(
            error.contains("canonical") || error.contains("without a leading dot"),
            "diagnostic must explain canonical FQN failure: {error}"
        );
    }

    let source = compile_for_profiler_capture(
        descriptors(),
        &[
            RootSpec::new(VALID_ROOT, "proto3_valid_root"),
            RootSpec::new(PROTO2_VALID_ROOT, "proto2_valid_root"),
        ],
    )
    .expect("canonical FQNs distinguish equal short names and accept proto2 optional fields")
    .into_source();
    for expected in [
        "value: &crate::proto::fixture::protobuf_source::contract::ValidRoot",
        "value: &crate::proto::fixture::protobuf_source::contract_proto2::ValidRoot",
    ] {
        assert!(
            source.contains(expected),
            "generated binding must contain {expected:?}"
        );
    }

    let valid_root = format!("{PACKAGE}.ValidRoot");
    for relation_name in ["", "Uppercase", "two__segments", "nul"] {
        let error = compile_error(&[RootSpec::new(&valid_root, relation_name)]);
        assert!(error.contains("generated relation name"));
        assert!(error.contains("is illegal"));
    }
    for relation_name in [
        "protobuf_enum_symbol",
        "profiler_payload_occurrence",
        "clock_domain",
        "clock_snapshot",
    ] {
        let error = compile_error(&[RootSpec::new(&valid_root, relation_name)]);
        assert!(error.contains(&format!(
            "generated relation name {relation_name:?} is reserved"
        )));
    }
    compile_for_profiler_capture(descriptors(), &[RootSpec::new(&valid_root, "sched_switch")])
        .expect("retired root-level sched_switch relation is no longer reserved");

    let reserved_child_error = compile_error(&[RootSpec::new(RESERVED_CHILD_ROOT, "clock")]);
    for expected in [
        format!("protobuf root {RESERVED_CHILD_ROOT:?}"),
        format!("message {RESERVED_CHILD_ROOT:?}"),
        "generated relation name \"clock_domain\" is reserved".to_owned(),
    ] {
        assert!(
            reserved_child_error.contains(&expected),
            "missing derived reserved-relation fact {expected:?}: {reserved_child_error}"
        );
    }

    let collision_error = compile_error(&[RootSpec::new(
        &format!("{PACKAGE}.NameCollisionRoot"),
        "collision_root",
    )]);
    assert!(collision_error.contains("generated relation name \"collision_root_foo_bar\""));
    assert!(collision_error.contains("collides between"));
    assert!(collision_error.contains("path foo_bar"));
    assert!(collision_error.contains("path foo.bar"));

    let reserved_column_error = compile_error(&[RootSpec::new(
        &format!("{PACKAGE}.ReservedColumnRoot"),
        "reserved_column_root",
    )]);
    assert!(reserved_column_error.contains(&format!(
        "message \"{PACKAGE}.ReservedColumnRoot\", field \"_kat_parent_row_id\""
    )));
    assert!(reserved_column_error.contains("is illegal or reserved at relation scope"));
}

#[test]
fn enum_symbol_accessor_reuses_descriptor_validation_and_safe_names() {
    let generated =
        compile_for_profiler_capture(descriptors(), &[RootSpec::new(VALID_ROOT, "valid_root")])
            .expect("the valid root compiles");
    let source = generated
        .clone()
        .with_enum_symbol_accessor(
            descriptors(),
            &format!("{PACKAGE}.Lifecycle"),
            "lifecycle_symbols",
        )
        .expect("the canonical enum and safe accessor compile")
        .into_source();
    assert!(source.contains("lifecycle_symbols"));

    for (enum_fqn, accessor, expected) in [
        (
            format!("{PACKAGE}.Lifecycle"),
            "LifecycleSymbols",
            "safe lower_snake Rust identifier",
        ),
        (
            format!(".{PACKAGE}.Lifecycle"),
            "lifecycle_symbols",
            "without a leading dot",
        ),
        (
            format!("{PACKAGE}.MissingEnum"),
            "lifecycle_symbols",
            "does not identify an enum",
        ),
        (
            format!("{PACKAGE}.AliasedStatus"),
            "lifecycle_symbols",
            "enum aliases are unsupported",
        ),
    ] {
        let error = generated
            .clone()
            .with_enum_symbol_accessor(descriptors(), &enum_fqn, accessor)
            .expect_err("invalid enum accessor contract must fail")
            .to_string();
        assert!(error.contains(&format!("protobuf enum {enum_fqn:?}")));
        assert!(error.contains(expected), "unexpected diagnostic: {error}");
    }
}

fn compile_error(roots: &[RootSpec<'_>]) -> String {
    compile_for_profiler_capture(descriptors(), roots)
        .expect_err("the descriptor roots must be rejected")
        .to_string()
}

fn descriptors() -> &'static FileDescriptorSet {
    static DESCRIPTORS: OnceLock<FileDescriptorSet> = OnceLock::new();
    DESCRIPTORS.get_or_init(load_descriptors)
}

fn load_descriptors() -> FileDescriptorSet {
    let fixture_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/protobuf_source_codegen");
    let files = [
        fixture_directory.join("contract.proto"),
        fixture_directory.join("contract_proto2.proto"),
    ];
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .load_fds(&files, &[fixture_directory])
        .expect("protobuf compiler contract descriptors load")
}
