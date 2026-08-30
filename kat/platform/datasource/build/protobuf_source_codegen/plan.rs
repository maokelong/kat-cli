use std::collections::{BTreeMap, BTreeSet};

use prost_reflect::{
    Cardinality, DescriptorPool, EnumDescriptor, FieldDescriptor, Kind, MessageDescriptor,
};

use super::{RootSpec, diagnostic::Diagnostic, enum_uses_aliases, names};

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
    catalog: &DescriptorPool,
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
    catalog: &'a DescriptorPool,
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
        let message = self
            .catalog
            .get_message_by_name(spec.protobuf_fqn)
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
        message: &MessageDescriptor,
        stack: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
        path_prefix: &[String],
        entered_from_containing_fqn: Option<&str>,
    ) -> Result<(), Diagnostic> {
        if let Some(cycle_start) = stack.iter().position(|fqn| fqn == message.full_name()) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(message.full_name().to_string());
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
        if visited.contains(message.full_name()) {
            return Ok(());
        }
        if message.is_map_entry() {
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                "synthetic protobuf map-entry messages cannot be published as roots or relations",
            ));
        }
        if message.child_extensions().next().is_some()
            || message.extension_ranges().next().is_some()
            || message.extensions().next().is_some()
        {
            return Err(message_shape_diagnostic(
                root_fqn,
                message,
                path_prefix,
                entered_from_containing_fqn,
                "reachable protobuf extensions are unsupported",
            ));
        }

        stack.push(message.full_name().to_string());
        let mut fields = message.fields().collect::<Vec<_>>();
        fields.sort_by_key(FieldDescriptor::number);
        let mut numbers = BTreeSet::new();
        let mut names_seen = BTreeSet::new();
        for field in fields {
            let field_name = field.name();
            let mut full_path = path_prefix.to_vec();
            full_path.push(field_name.to_string());
            let field_path = full_path.join(".");
            if !numbers.insert(field.number()) {
                return Err(Diagnostic::field(
                    root_fqn,
                    message.full_name(),
                    &field_path,
                    "duplicate protobuf field number",
                ));
            }
            if !names_seen.insert(field_name.to_string()) {
                return Err(Diagnostic::field(
                    root_fqn,
                    message.full_name(),
                    &field_path,
                    "duplicate protobuf field name",
                ));
            }
            if field.is_required() {
                return Err(Diagnostic::field(
                    root_fqn,
                    message.full_name(),
                    &field_path,
                    "proto2 required fields are unsupported",
                ));
            }
            if field.is_group() {
                return Err(Diagnostic::field(
                    root_fqn,
                    message.full_name(),
                    &field_path,
                    "protobuf group fields are unsupported",
                ));
            }
            match field.kind() {
                Kind::Message(target) => {
                    if target.is_map_entry() {
                        return Err(Diagnostic::field(
                            root_fqn,
                            message.full_name(),
                            &field_path,
                            "protobuf map fields are unsupported",
                        ));
                    }
                    if let Some(cycle_start) =
                        stack.iter().position(|fqn| fqn == target.full_name())
                    {
                        let mut cycle = stack[cycle_start..].to_vec();
                        cycle.push(target.full_name().to_string());
                        return Err(Diagnostic::field(
                            root_fqn,
                            message.full_name(),
                            &field_path,
                            format!(
                                "recursive message edge is unsupported: {}",
                                cycle.join(" -> ")
                            ),
                        ));
                    }
                    self.validate_closure(
                        root_fqn,
                        &target,
                        stack,
                        visited,
                        &full_path,
                        Some(message.full_name()),
                    )?;
                }
                Kind::Enum(enum_def) => {
                    self.validate_enum(root_fqn, message, &field_path, &enum_def)?;
                }
                _ => {}
            }
        }
        stack.pop();
        visited.insert(message.full_name().to_string());
        Ok(())
    }

    fn validate_enum(
        &self,
        root_fqn: &str,
        message: &MessageDescriptor,
        field_path: &str,
        enum_def: &EnumDescriptor,
    ) -> Result<(), Diagnostic> {
        if enum_uses_aliases(enum_def) {
            return Err(Diagnostic::field(
                root_fqn,
                message.full_name(),
                field_path,
                format!(
                    "enum {:?} uses aliases, which are unsupported",
                    enum_def.full_name()
                ),
            ));
        }
        Ok(())
    }

    fn plan_message_relation(
        &mut self,
        root_fqn: &str,
        root_table_name: &str,
        message: &MessageDescriptor,
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
            message_fqn: message.full_name().to_string(),
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
        message: &MessageDescriptor,
        relation_path: &[String],
        nullable_ancestor: bool,
        arrow_path: Vec<String>,
    ) -> Result<Vec<ColumnPlan>, Diagnostic> {
        let mut fields = message.fields().collect::<Vec<_>>();
        fields.sort_by_key(FieldDescriptor::number);
        let mut columns = Vec::new();
        let mut column_names = BTreeSet::new();
        let relation_top_level = arrow_path.is_empty();

        for descriptor in fields {
            let field = self.proto_field(&descriptor);
            let kind = descriptor.kind();
            let repeated = descriptor.cardinality() == Cardinality::Repeated;
            let user_oneof = matches!(field.presence, Presence::Oneof { .. });
            if repeated {
                let mut child_path = relation_path.to_vec();
                child_path.push(field.name.clone());
                if let Kind::Message(target) = &kind {
                    self.plan_message_relation(
                        root_fqn,
                        root_table_name,
                        target,
                        child_path,
                        RelationSource::RepeatedMessage {
                            parent: relation_slot,
                            field,
                        },
                    )?;
                } else {
                    let scalar = scalar_type(&kind).expect("validated scalar type");
                    let relation_name = names::relation_name(root_table_name, &child_path);
                    let origin = format!("root {root_fqn} path {}", child_path.join("."));
                    self.register_relation_name(root_fqn, message, &relation_name, &origin)?;
                    let child_slot = self.relations.len();
                    self.relations.push(RelationPlan {
                        slot: child_slot,
                        name: relation_name,
                        message_fqn: message.full_name().to_string(),
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
                    if let Kind::Enum(enum_def) = &kind {
                        self.register_enum_origin(child_slot, "value", enum_def);
                    }
                }
                continue;
            }

            if let Kind::Message(target) = &kind {
                if user_oneof || self.message_has_relations(target) {
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
                        target,
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
                        target,
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
            let scalar = scalar_type(&kind).expect("validated scalar type");
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
            if let Kind::Enum(enum_def) = &kind {
                self.register_enum_origin(relation_slot, &origin_path.join("."), enum_def);
            }
        }
        Ok(columns)
    }

    fn message_has_relations(&mut self, message: &MessageDescriptor) -> bool {
        if let Some(result) = self.relation_memo.get(message.full_name()) {
            return *result;
        }
        // 递归已在此前拒绝；临时写入 false 只用于让 memo 在本次构建期递归中保持完备。
        self.relation_memo
            .insert(message.full_name().to_string(), false);
        let result = message.fields().any(|field| {
            if field.cardinality() == Cardinality::Repeated {
                return true;
            }
            match field.kind() {
                Kind::Message(target) => {
                    self.user_oneof(&field).is_some() || self.message_has_relations(&target)
                }
                _ => false,
            }
        });
        self.relation_memo
            .insert(message.full_name().to_string(), result);
        result
    }

    fn proto_field(&self, descriptor: &FieldDescriptor) -> ProtoField {
        let presence = if let Some(group_name) = self.user_oneof(descriptor) {
            Presence::Oneof { group_name }
        } else if descriptor.supports_presence() {
            Presence::Explicit
        } else {
            Presence::Implicit
        };
        ProtoField {
            containing_message_fqn: descriptor.parent_message().full_name().to_string(),
            name: descriptor.name().to_string(),
            number: i32::try_from(descriptor.number()).expect("protobuf field number fits i32"),
            presence,
        }
    }

    fn user_oneof(&self, field: &FieldDescriptor) -> Option<String> {
        field
            .containing_oneof()
            .filter(|oneof| !oneof.is_synthetic())
            .map(|oneof| oneof.name().to_string())
    }

    fn register_enum_origin(
        &mut self,
        relation_slot: usize,
        field_path: &str,
        enum_def: &EnumDescriptor,
    ) {
        self.enum_origins.push(EnumOriginPlan {
            relation_slot,
            field_path: field_path.to_string(),
            enum_fqn: enum_def.full_name().to_string(),
            symbols: enum_def
                .values()
                .map(|value| EnumSymbolPlan {
                    number: value.number(),
                    symbol: value.name().to_string(),
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
        message: &MessageDescriptor,
        relation_name: &str,
        origin: &str,
    ) -> Result<(), Diagnostic> {
        if !names::valid_dataset_name(relation_name) {
            return Err(Diagnostic::message(
                root_fqn,
                message.full_name(),
                format!("generated relation name {relation_name:?} is illegal"),
            ));
        }
        if RESERVED_TABLES.contains(&relation_name) {
            return Err(Diagnostic::message(
                root_fqn,
                message.full_name(),
                format!("generated relation name {relation_name:?} is reserved"),
            ));
        }
        if let Some(previous) = self
            .relation_names
            .insert(relation_name.to_string(), origin.to_string())
        {
            return Err(Diagnostic::message(
                root_fqn,
                message.full_name(),
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
    message: &MessageDescriptor,
    names: &mut BTreeSet<String>,
    name: &str,
    relation_top_level: bool,
) -> Result<(), Diagnostic> {
    if relation_top_level && names::reserved_relationship_column(name) {
        return Err(Diagnostic::field(
            root_fqn,
            message.full_name(),
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
            message.full_name(),
            name,
            format!("column name {name:?} collides after relational mapping"),
        ))
    }
}

fn message_shape_diagnostic(
    root_fqn: &str,
    message: &MessageDescriptor,
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
            format!("{detail}; target message {:?}", message.full_name()),
        ),
        None => Diagnostic::message(root_fqn, message.full_name(), detail),
    }
}

fn scalar_type(kind: &Kind) -> Option<ScalarType> {
    match kind {
        Kind::Double => Some(ScalarType::Float64),
        Kind::Float => Some(ScalarType::Float32),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => Some(ScalarType::Int64),
        Kind::Uint64 | Kind::Fixed64 => Some(ScalarType::UInt64),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 | Kind::Enum(_) => Some(ScalarType::Int32),
        Kind::Uint32 | Kind::Fixed32 => Some(ScalarType::UInt32),
        Kind::Bool => Some(ScalarType::Boolean),
        Kind::String => Some(ScalarType::Utf8),
        Kind::Bytes => Some(ScalarType::Binary),
        Kind::Message(_) => None,
    }
}
