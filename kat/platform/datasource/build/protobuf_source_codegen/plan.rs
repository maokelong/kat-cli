use std::collections::{BTreeMap, BTreeSet};

use prost_types::{
    FieldDescriptorProto,
    field_descriptor_proto::{Label, Type},
};

use super::{
    RootSpec,
    descriptor::{Catalog, EnumDef, MessageDef, Syntax},
    diagnostic::Diagnostic,
    names,
};

const RESERVED_TABLES: &[&str] = &[
    "protobuf_enum_symbol",
    "profiler_payload_occurrence",
    "clock_domain",
    "clock_snapshot",
    "sched_switch",
];

#[derive(Clone, Debug)]
pub(super) struct RelationalPlan {
    pub(super) roots: Vec<RootPlan>,
    pub(super) relations: Vec<RelationPlan>,
    pub(super) enum_origins: Vec<EnumOriginPlan>,
}

#[derive(Clone, Debug)]
pub(super) struct RootPlan {
    pub(super) spec_index: usize,
    pub(super) protobuf_fqn: String,
    pub(super) root_table_name: String,
    pub(super) relation_slot: usize,
}

#[derive(Clone, Debug)]
pub(super) struct RelationPlan {
    pub(super) slot: usize,
    pub(super) name: String,
    pub(super) message_fqn: String,
    pub(super) source: RelationSource,
    pub(super) columns: Vec<ColumnPlan>,
}

#[derive(Clone, Debug)]
pub(super) enum RelationSource {
    Root { root_index: usize },
    RepeatedMessage { parent: usize, field: ProtoField },
    OptionalMessage { parent: usize, field: ProtoField },
    OneofMessage { parent: usize, field: ProtoField },
    RepeatedValue { parent: usize, field: ProtoField },
}

impl RelationSource {
    pub(super) fn parent(&self) -> Option<usize> {
        match self {
            Self::Root { .. } => None,
            Self::RepeatedMessage { parent, .. }
            | Self::OptionalMessage { parent, .. }
            | Self::OneofMessage { parent, .. }
            | Self::RepeatedValue { parent, .. } => Some(*parent),
        }
    }

    pub(super) fn is_repeated(&self) -> bool {
        matches!(
            self,
            Self::RepeatedMessage { .. } | Self::RepeatedValue { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct ColumnPlan {
    pub(super) name: String,
    pub(super) nullable: bool,
    pub(super) source: ColumnSource,
}

#[derive(Clone, Debug)]
pub(super) enum ColumnSource {
    RowId,
    ParentRowId,
    RepeatedIndex,
    Scalar {
        scalar: ScalarType,
        value: ScalarValue,
    },
    Struct {
        field: ProtoField,
        fields: Vec<ColumnPlan>,
    },
}

#[derive(Clone, Debug)]
pub(super) enum ScalarValue {
    Field(ProtoField),
    RelationValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScalarType {
    Boolean,
    Int32,
    Int64,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Utf8,
    Binary,
}

#[derive(Clone, Debug)]
pub(super) struct ProtoField {
    pub(super) containing_message_fqn: String,
    pub(super) name: String,
    pub(super) number: i32,
    pub(super) presence: Presence,
}

#[derive(Clone, Debug)]
pub(super) enum Presence {
    Implicit,
    Explicit,
    Oneof { group_name: String },
}

#[derive(Clone, Debug)]
pub(super) struct EnumOriginPlan {
    pub(super) relation_slot: usize,
    pub(super) field_path: String,
    pub(super) enum_fqn: String,
    pub(super) symbols: Vec<EnumSymbolPlan>,
}

#[derive(Clone, Debug)]
pub(super) struct EnumSymbolPlan {
    pub(super) number: i32,
    pub(super) symbol: String,
}

pub(super) fn build(
    catalog: &Catalog,
    roots: &[RootSpec<'_>],
) -> Result<RelationalPlan, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut builder = Builder {
        catalog,
        relations: Vec::new(),
        enum_origins: Vec::new(),
        relation_names: BTreeMap::new(),
        relation_memo: BTreeMap::new(),
    };
    let mut root_plans = Vec::new();

    for (root_index, spec) in roots.iter().enumerate() {
        match builder.build_root(root_index, spec) {
            Ok(root) => root_plans.push(root),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    builder.add_relationship_columns();
    Ok(RelationalPlan {
        roots: root_plans,
        relations: builder.relations,
        enum_origins: builder.enum_origins,
    })
}

struct Builder<'a> {
    catalog: &'a Catalog,
    relations: Vec<RelationPlan>,
    enum_origins: Vec<EnumOriginPlan>,
    relation_names: BTreeMap<String, String>,
    relation_memo: BTreeMap<String, bool>,
}

impl Builder<'_> {
    fn build_root(
        &mut self,
        root_index: usize,
        spec: &RootSpec<'_>,
    ) -> Result<RootPlan, Diagnostic> {
        if spec.protobuf_fqn.starts_with('.') || spec.protobuf_fqn.is_empty() {
            return Err(Diagnostic::root(
                spec.protobuf_fqn,
                "root type must be a canonical protobuf FQN without a leading dot",
            ));
        }
        if self.catalog.message_is_ambiguous(spec.protobuf_fqn) {
            return Err(Diagnostic::root(
                spec.protobuf_fqn,
                "canonical root FQN is defined more than once",
            ));
        }
        let message = self
            .catalog
            .message(spec.protobuf_fqn)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::root(
                    spec.protobuf_fqn,
                    "canonical root FQN does not identify a message in the descriptor set",
                )
            })?;

        self.validate_closure(
            spec.protobuf_fqn,
            &message,
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &[],
            None,
        )?;
        let relation_slot = self.plan_message_relation(
            spec.protobuf_fqn,
            spec.root_table_name,
            &message,
            Vec::new(),
            RelationSource::Root { root_index },
        )?;
        Ok(RootPlan {
            spec_index: root_index,
            protobuf_fqn: spec.protobuf_fqn.to_string(),
            root_table_name: spec.root_table_name.to_string(),
            relation_slot,
        })
    }

    fn validate_closure(
        &self,
        root_fqn: &str,
        message: &MessageDef,
        stack: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
        path_prefix: &[String],
        entered_from_containing_fqn: Option<&str>,
    ) -> Result<(), Diagnostic> {
        if let Some(cycle_start) = stack.iter().position(|fqn| fqn == &message.fqn) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(message.fqn.clone());
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                format!(
                    "recursive message graph is unsupported: {}",
                    cycle.join(" -> ")
                ),
            ));
        }
        if visited.contains(&message.fqn) {
            return Ok(());
        }
        if message.syntax == Syntax::Other {
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                "only proto2 and proto3 descriptor syntax is supported",
            ));
        }
        if message
            .descriptor
            .options
            .as_ref()
            .and_then(|options| options.map_entry)
            .unwrap_or(false)
        {
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                "synthetic protobuf map-entry messages cannot be published as roots or relations",
            ));
        }
        if !message.descriptor.extension.is_empty()
            || !message.descriptor.extension_range.is_empty()
            || !self.catalog.extensions_for(&message.fqn).is_empty()
        {
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                "reachable protobuf extensions are unsupported",
            ));
        }

        stack.push(message.fqn.clone());
        let mut fields = message.descriptor.field.iter().collect::<Vec<_>>();
        fields.sort_by_key(|field| field.number.unwrap_or_default());
        let mut numbers = BTreeSet::new();
        let mut names_seen = BTreeSet::new();
        for field in fields {
            let field_name = field.name.as_deref().unwrap_or("<unnamed>");
            let mut full_path = path_prefix.to_vec();
            full_path.push(field_name.to_string());
            let field_path = full_path.join(".");
            if !numbers.insert(field.number.unwrap_or_default()) {
                return Err(Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "duplicate protobuf field number",
                ));
            }
            if !names_seen.insert(field_name) {
                return Err(Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "duplicate protobuf field name",
                ));
            }
            let label = field_label(field).ok_or_else(|| {
                Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "field has an invalid or missing label",
                )
            })?;
            if label == Label::Required {
                return Err(Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "proto2 required fields are unsupported",
                ));
            }
            let field_type = field_type(field).ok_or_else(|| {
                Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "field has an invalid or missing protobuf type",
                )
            })?;
            if field_type == Type::Group {
                return Err(Diagnostic::field(
                    root_fqn,
                    &message.fqn,
                    &field_path,
                    "protobuf group fields are unsupported",
                ));
            }
            self.validate_oneof(root_fqn, message, field, &field_path)?;

            match field_type {
                Type::Message => {
                    let target = self.message_target(root_fqn, message, field, &field_path)?;
                    if target
                        .descriptor
                        .options
                        .as_ref()
                        .and_then(|options| options.map_entry)
                        .unwrap_or(false)
                    {
                        return Err(Diagnostic::field(
                            root_fqn,
                            &message.fqn,
                            &field_path,
                            "protobuf map fields are unsupported",
                        ));
                    }
                    if let Some(cycle_start) = stack.iter().position(|fqn| fqn == &target.fqn) {
                        let mut cycle = stack[cycle_start..].to_vec();
                        cycle.push(target.fqn.clone());
                        return Err(Diagnostic::field(
                            root_fqn,
                            &message.fqn,
                            &field_path,
                            format!(
                                "recursive message edge is unsupported: {}",
                                cycle.join(" -> ")
                            ),
                        ));
                    }
                    self.validate_closure(
                        root_fqn,
                        target,
                        stack,
                        visited,
                        &full_path,
                        Some(&message.fqn),
                    )?;
                }
                Type::Enum => {
                    let enum_def = self.enum_target(root_fqn, message, field, &field_path)?;
                    self.validate_enum(root_fqn, message, &field_path, enum_def)?;
                }
                _ => {}
            }
        }
        stack.pop();
        visited.insert(message.fqn.clone());
        Ok(())
    }

    fn validate_oneof(
        &self,
        root_fqn: &str,
        message: &MessageDef,
        field: &FieldDescriptorProto,
        field_path: &str,
    ) -> Result<(), Diagnostic> {
        let Some(index) = field.oneof_index else {
            return Ok(());
        };
        let index = usize::try_from(index).ok();
        if index
            .and_then(|index| message.descriptor.oneof_decl.get(index))
            .is_none()
        {
            return Err(Diagnostic::field(
                root_fqn,
                &message.fqn,
                field_path,
                "field refers to a missing oneof declaration",
            ));
        }
        Ok(())
    }

    fn validate_enum(
        &self,
        root_fqn: &str,
        message: &MessageDef,
        field_path: &str,
        enum_def: &EnumDef,
    ) -> Result<(), Diagnostic> {
        let alias_enabled = enum_def
            .descriptor
            .options
            .as_ref()
            .and_then(|options| options.allow_alias)
            .unwrap_or(false);
        let mut numbers = BTreeSet::new();
        let duplicated_number = enum_def
            .descriptor
            .value
            .iter()
            .any(|value| !numbers.insert(value.number.unwrap_or_default()));
        if alias_enabled || duplicated_number {
            return Err(Diagnostic::field(
                root_fqn,
                &message.fqn,
                field_path,
                format!(
                    "enum {:?} uses aliases, which are unsupported",
                    enum_def.fqn
                ),
            ));
        }
        Ok(())
    }

    fn plan_message_relation(
        &mut self,
        root_fqn: &str,
        root_table_name: &str,
        message: &MessageDef,
        relation_path: Vec<String>,
        source: RelationSource,
    ) -> Result<usize, Diagnostic> {
        let relation_name = names::relation_name(root_table_name, &relation_path);
        let origin = if relation_path.is_empty() {
            format!("root {root_fqn}")
        } else {
            format!("root {root_fqn} path {}", relation_path.join("."))
        };
        self.register_relation_name(root_fqn, message, &relation_name, &origin)?;

        let slot = self.relations.len();
        self.relations.push(RelationPlan {
            slot,
            name: relation_name,
            message_fqn: message.fqn.clone(),
            source,
            columns: Vec::new(),
        });
        let columns = self.plan_message_columns(
            root_fqn,
            root_table_name,
            slot,
            message,
            &relation_path,
            false,
            Vec::new(),
        )?;
        self.relations[slot].columns = columns;
        Ok(slot)
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_message_columns(
        &mut self,
        root_fqn: &str,
        root_table_name: &str,
        relation_slot: usize,
        message: &MessageDef,
        relation_path: &[String],
        nullable_ancestor: bool,
        arrow_path: Vec<String>,
    ) -> Result<Vec<ColumnPlan>, Diagnostic> {
        let mut fields = message.descriptor.field.iter().collect::<Vec<_>>();
        fields.sort_by_key(|field| field.number.unwrap_or_default());
        let mut columns = Vec::new();
        let mut column_names = BTreeSet::new();
        let relation_top_level = arrow_path.is_empty();

        for descriptor in fields {
            let field = self.proto_field(message, descriptor)?;
            let field_type = field_type(descriptor).expect("closure validation checked field type");
            let repeated = field_label(descriptor) == Some(Label::Repeated);
            let user_oneof = matches!(field.presence, Presence::Oneof { .. });
            if repeated {
                let mut child_path = relation_path.to_vec();
                child_path.push(field.name.clone());
                if field_type == Type::Message {
                    let target = self
                        .catalog
                        .resolve_message(
                            &message.fqn,
                            descriptor.type_name.as_deref().unwrap_or_default(),
                        )
                        .cloned()
                        .expect("closure validation resolved message");
                    self.plan_message_relation(
                        root_fqn,
                        root_table_name,
                        &target,
                        child_path,
                        RelationSource::RepeatedMessage {
                            parent: relation_slot,
                            field,
                        },
                    )?;
                } else {
                    let scalar = scalar_type(field_type).expect("validated scalar type");
                    let relation_name = names::relation_name(root_table_name, &child_path);
                    let origin = format!("root {root_fqn} path {}", child_path.join("."));
                    self.register_relation_name(root_fqn, message, &relation_name, &origin)?;
                    let child_slot = self.relations.len();
                    self.relations.push(RelationPlan {
                        slot: child_slot,
                        name: relation_name,
                        message_fqn: message.fqn.clone(),
                        source: RelationSource::RepeatedValue {
                            parent: relation_slot,
                            field: field.clone(),
                        },
                        columns: vec![ColumnPlan {
                            name: "value".to_string(),
                            nullable: false,
                            source: ColumnSource::Scalar {
                                scalar,
                                value: ScalarValue::RelationValue,
                            },
                        }],
                    });
                    if field_type == Type::Enum {
                        let enum_def = self
                            .catalog
                            .resolve_enum(
                                &message.fqn,
                                descriptor.type_name.as_deref().unwrap_or_default(),
                            )
                            .cloned()
                            .expect("closure validation resolved enum");
                        self.register_enum_origin(child_slot, "value", &enum_def);
                    }
                }
                continue;
            }

            if field_type == Type::Message {
                let target = self
                    .catalog
                    .resolve_message(
                        &message.fqn,
                        descriptor.type_name.as_deref().unwrap_or_default(),
                    )
                    .cloned()
                    .expect("closure validation resolved message");
                if user_oneof || self.message_has_relations(&target) {
                    let mut child_path = relation_path.to_vec();
                    child_path.push(field.name.clone());
                    let source = if user_oneof {
                        RelationSource::OneofMessage {
                            parent: relation_slot,
                            field,
                        }
                    } else {
                        RelationSource::OptionalMessage {
                            parent: relation_slot,
                            field,
                        }
                    };
                    self.plan_message_relation(
                        root_fqn,
                        root_table_name,
                        &target,
                        child_path,
                        source,
                    )?;
                } else {
                    ensure_unique_column(
                        root_fqn,
                        message,
                        &mut column_names,
                        &field.name,
                        relation_top_level,
                    )?;
                    let mut nested_arrow_path = arrow_path.clone();
                    nested_arrow_path.push(field.name.clone());
                    let nested = self.plan_message_columns(
                        root_fqn,
                        root_table_name,
                        relation_slot,
                        &target,
                        relation_path,
                        true,
                        nested_arrow_path,
                    )?;
                    columns.push(ColumnPlan {
                        name: field.name.clone(),
                        nullable: true,
                        source: ColumnSource::Struct {
                            field,
                            fields: nested,
                        },
                    });
                }
                continue;
            }

            ensure_unique_column(
                root_fqn,
                message,
                &mut column_names,
                &field.name,
                relation_top_level,
            )?;
            let scalar = scalar_type(field_type).expect("validated scalar type");
            let nullable = nullable_ancestor || !matches!(field.presence, Presence::Implicit);
            let mut origin_path = arrow_path.clone();
            origin_path.push(field.name.clone());
            columns.push(ColumnPlan {
                name: field.name.clone(),
                nullable,
                source: ColumnSource::Scalar {
                    scalar,
                    value: ScalarValue::Field(field.clone()),
                },
            });
            if field_type == Type::Enum {
                let enum_def = self
                    .catalog
                    .resolve_enum(
                        &message.fqn,
                        descriptor.type_name.as_deref().unwrap_or_default(),
                    )
                    .cloned()
                    .expect("closure validation resolved enum");
                self.register_enum_origin(relation_slot, &origin_path.join("."), &enum_def);
            }
        }
        Ok(columns)
    }

    fn message_has_relations(&mut self, message: &MessageDef) -> bool {
        if let Some(result) = self.relation_memo.get(&message.fqn) {
            return *result;
        }
        // 递归已在此前拒绝；临时写入 false 只用于让 memo 在本次构建期递归中保持完备。
        self.relation_memo.insert(message.fqn.clone(), false);
        let result = message.descriptor.field.iter().any(|field| {
            if field_label(field) == Some(Label::Repeated) {
                return true;
            }
            if field_type(field) != Some(Type::Message) {
                return false;
            }
            if self.user_oneof(message, field).is_some() {
                return true;
            }
            let Some(target) = field
                .type_name
                .as_deref()
                .and_then(|type_name| self.catalog.resolve_message(&message.fqn, type_name))
                .cloned()
            else {
                return false;
            };
            self.message_has_relations(&target)
        });
        self.relation_memo.insert(message.fqn.clone(), result);
        result
    }

    fn proto_field(
        &self,
        message: &MessageDef,
        descriptor: &FieldDescriptorProto,
    ) -> Result<ProtoField, Diagnostic> {
        let name = descriptor.name.clone().unwrap_or_default();
        let presence = if let Some(group_name) = self.user_oneof(message, descriptor) {
            Presence::Oneof { group_name }
        } else if descriptor.proto3_optional.unwrap_or(false)
            || field_type(descriptor) == Some(Type::Message)
            || (message.syntax == Syntax::Proto2
                && field_label(descriptor) == Some(Label::Optional))
        {
            Presence::Explicit
        } else {
            Presence::Implicit
        };
        Ok(ProtoField {
            containing_message_fqn: message.fqn.clone(),
            name,
            number: descriptor.number.unwrap_or_default(),
            presence,
        })
    }

    fn user_oneof(&self, message: &MessageDef, field: &FieldDescriptorProto) -> Option<String> {
        if field.proto3_optional.unwrap_or(false) {
            return None;
        }
        let index = usize::try_from(field.oneof_index?).ok()?;
        message.descriptor.oneof_decl.get(index)?.name.clone()
    }

    fn message_target<'a>(
        &'a self,
        root_fqn: &str,
        message: &MessageDef,
        field: &FieldDescriptorProto,
        field_path: &str,
    ) -> Result<&'a MessageDef, Diagnostic> {
        let type_name = field.type_name.as_deref().unwrap_or_default();
        let canonical = self.catalog.canonical_reference(&message.fqn, type_name);
        self.catalog
            .resolve_message(&message.fqn, type_name)
            .ok_or_else(|| {
                let detail = if self.catalog.message_is_ambiguous(&canonical) {
                    format!("message type {canonical:?} is defined more than once")
                } else {
                    format!("message type {canonical:?} is missing from the descriptor set")
                };
                Diagnostic::field(root_fqn, &message.fqn, field_path, detail)
            })
    }

    fn enum_target<'a>(
        &'a self,
        root_fqn: &str,
        message: &MessageDef,
        field: &FieldDescriptorProto,
        field_path: &str,
    ) -> Result<&'a EnumDef, Diagnostic> {
        let type_name = field.type_name.as_deref().unwrap_or_default();
        let canonical = self.catalog.canonical_reference(&message.fqn, type_name);
        self.catalog
            .resolve_enum(&message.fqn, type_name)
            .ok_or_else(|| {
                let detail = if self.catalog.enum_is_ambiguous(&canonical) {
                    format!("enum type {canonical:?} is defined more than once")
                } else {
                    format!("enum type {canonical:?} is missing from the descriptor set")
                };
                Diagnostic::field(root_fqn, &message.fqn, field_path, detail)
            })
    }

    fn register_enum_origin(&mut self, relation_slot: usize, field_path: &str, enum_def: &EnumDef) {
        self.enum_origins.push(EnumOriginPlan {
            relation_slot,
            field_path: field_path.to_string(),
            enum_fqn: enum_def.fqn.clone(),
            symbols: enum_def
                .descriptor
                .value
                .iter()
                .map(|value| EnumSymbolPlan {
                    number: value.number.unwrap_or_default(),
                    symbol: value.name.clone().unwrap_or_default(),
                })
                .collect(),
        });
    }

    fn add_relationship_columns(&mut self) {
        let mut referenced = vec![false; self.relations.len()];
        for relation in &self.relations {
            if let Some(parent) = relation.source.parent() {
                referenced[parent] = true;
            }
        }
        for relation in &mut self.relations {
            let mut columns = Vec::new();
            if referenced[relation.slot] {
                columns.push(ColumnPlan {
                    name: "_kat_row_id".to_string(),
                    nullable: false,
                    source: ColumnSource::RowId,
                });
            }
            columns.push(ColumnPlan {
                name: "_kat_parent_row_id".to_string(),
                nullable: false,
                source: ColumnSource::ParentRowId,
            });
            if relation.source.is_repeated() {
                columns.push(ColumnPlan {
                    name: "_kat_repeated_index".to_string(),
                    nullable: false,
                    source: ColumnSource::RepeatedIndex,
                });
            }
            columns.append(&mut relation.columns);
            relation.columns = columns;
        }
    }

    fn register_relation_name(
        &mut self,
        root_fqn: &str,
        message: &MessageDef,
        relation_name: &str,
        origin: &str,
    ) -> Result<(), Diagnostic> {
        if !names::valid_dataset_name(relation_name) {
            return Err(Diagnostic::message(
                root_fqn,
                &message.fqn,
                format!("generated relation name {relation_name:?} is illegal"),
            ));
        }
        if RESERVED_TABLES.contains(&relation_name) {
            return Err(Diagnostic::message(
                root_fqn,
                &message.fqn,
                format!("generated relation name {relation_name:?} is reserved"),
            ));
        }
        if let Some(previous) = self
            .relation_names
            .insert(relation_name.to_string(), origin.to_string())
        {
            return Err(Diagnostic::message(
                root_fqn,
                &message.fqn,
                format!(
                    "generated relation name {relation_name:?} collides between {previous} and {origin}"
                ),
            ));
        }
        Ok(())
    }
}

fn ensure_unique_column(
    root_fqn: &str,
    message: &MessageDef,
    names: &mut BTreeSet<String>,
    name: &str,
    relation_top_level: bool,
) -> Result<(), Diagnostic> {
    if relation_top_level && names::reserved_relationship_column(name) {
        return Err(Diagnostic::field(
            root_fqn,
            &message.fqn,
            name,
            format!(
                "column name {name:?} is illegal or reserved at relation scope for protobuf relationships"
            ),
        ));
    }
    if names.insert(name.to_string()) {
        Ok(())
    } else {
        Err(Diagnostic::field(
            root_fqn,
            &message.fqn,
            name,
            format!("column name {name:?} collides after relational mapping"),
        ))
    }
}

fn message_shape_diagnostic(
    root_fqn: &str,
    message: &MessageDef,
    path_prefix: &[String],
    entered_from_containing_fqn: Option<&str>,
    detail: impl Into<String>,
) -> Diagnostic {
    let detail = detail.into();
    match entered_from_containing_fqn {
        Some(containing_message_fqn) => Diagnostic::field(
            root_fqn,
            containing_message_fqn,
            path_prefix.join("."),
            format!("{detail}; target message {:?}", message.fqn),
        ),
        None => Diagnostic::message(root_fqn, &message.fqn, detail),
    }
}

fn field_type(field: &FieldDescriptorProto) -> Option<Type> {
    Type::try_from(field.r#type?).ok()
}

fn field_label(field: &FieldDescriptorProto) -> Option<Label> {
    Label::try_from(field.label?).ok()
}

fn scalar_type(field_type: Type) -> Option<ScalarType> {
    match field_type {
        Type::Double => Some(ScalarType::Float64),
        Type::Float => Some(ScalarType::Float32),
        Type::Int64 | Type::Sint64 | Type::Sfixed64 => Some(ScalarType::Int64),
        Type::Uint64 | Type::Fixed64 => Some(ScalarType::UInt64),
        Type::Int32 | Type::Sint32 | Type::Sfixed32 | Type::Enum => Some(ScalarType::Int32),
        Type::Uint32 | Type::Fixed32 => Some(ScalarType::UInt32),
        Type::Bool => Some(ScalarType::Boolean),
        Type::String => Some(ScalarType::Utf8),
        Type::Bytes => Some(ScalarType::Binary),
        Type::Message | Type::Group => None,
    }
}
