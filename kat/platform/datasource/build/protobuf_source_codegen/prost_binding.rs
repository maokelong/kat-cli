use std::collections::{BTreeMap, BTreeSet};

use super::{
    descriptor::{Catalog, MessageDef},
    diagnostic::Diagnostic,
    names,
    plan::{ColumnPlan, ColumnSource, Presence, ProtoField, RelationSource, RelationalPlan},
};

#[derive(Clone, Debug)]
pub(super) struct ProstBindings {
    root_types: BTreeMap<usize, String>,
    fields: BTreeMap<FieldKey, FieldBinding>,
}

impl ProstBindings {
    pub(super) fn root_type(&self, root_index: usize) -> &str {
        self.root_types
            .get(&root_index)
            .expect("every relational root has a prost binding")
    }

    pub(super) fn field(&self, field: &ProtoField) -> &FieldBinding {
        self.fields
            .get(&FieldKey::new(field))
            .expect("every planned field has a prost binding")
    }
}

#[derive(Clone, Debug)]
pub(super) struct FieldBinding {
    pub(super) rust_field_ident: String,
    pub(super) oneof: Option<OneofBinding>,
}

#[derive(Clone, Debug)]
pub(super) struct OneofBinding {
    pub(super) rust_group_ident: String,
    pub(super) rust_enum_path: String,
    pub(super) rust_variant_ident: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FieldKey {
    message_fqn: String,
    number: i32,
}

impl FieldKey {
    fn new(field: &ProtoField) -> Self {
        Self {
            message_fqn: field.containing_message_fqn.clone(),
            number: field.number,
        }
    }
}

pub(super) fn bind(
    catalog: &Catalog,
    plan: &RelationalPlan,
) -> Result<ProstBindings, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut root_types = BTreeMap::new();
    for root in &plan.roots {
        let Some(message) = catalog.message(&root.protobuf_fqn) else {
            diagnostics.push(Diagnostic::root(
                &root.protobuf_fqn,
                "root disappeared while creating prost binding",
            ));
            continue;
        };
        root_types.insert(root.spec_index, message_rust_path(message));
    }

    let mut planned_fields = BTreeMap::<FieldKey, ProtoField>::new();
    for relation in &plan.relations {
        if let Some(field) = relation_source_field(&relation.source) {
            planned_fields.insert(FieldKey::new(field), field.clone());
        }
        for column in &relation.columns {
            collect_column_fields(column, &mut planned_fields);
        }
    }

    let mut checked_messages = BTreeSet::new();
    let mut fields = BTreeMap::new();
    for (key, field) in planned_fields {
        let Some(message) = catalog.message(&field.containing_message_fqn) else {
            diagnostics.push(Diagnostic::message(
                plan_root_for_message(plan, &field.containing_message_fqn),
                &field.containing_message_fqn,
                "containing message disappeared while creating prost binding",
            ));
            continue;
        };
        if checked_messages.insert(message.fqn.clone())
            && let Err(detail) = validate_message_identifiers(message)
        {
            diagnostics.push(Diagnostic::message(
                plan_root_for_message(plan, &message.fqn),
                &message.fqn,
                detail,
            ));
        }
        match bind_field(message, &field) {
            Ok(binding) => {
                fields.insert(key, binding);
            }
            Err(detail) => diagnostics.push(Diagnostic::field(
                plan_root_for_message(plan, &message.fqn),
                &message.fqn,
                &field.name,
                detail,
            )),
        }
    }

    if diagnostics.is_empty() {
        Ok(ProstBindings { root_types, fields })
    } else {
        Err(diagnostics)
    }
}

fn collect_column_fields(column: &ColumnPlan, fields: &mut BTreeMap<FieldKey, ProtoField>) {
    match &column.source {
        ColumnSource::Scalar {
            value: super::plan::ScalarValue::Field(field),
            ..
        } => {
            fields.insert(FieldKey::new(field), field.clone());
        }
        ColumnSource::Struct {
            field,
            fields: children,
        } => {
            fields.insert(FieldKey::new(field), field.clone());
            for child in children {
                collect_column_fields(child, fields);
            }
        }
        ColumnSource::RowId
        | ColumnSource::ParentRowId
        | ColumnSource::RepeatedIndex
        | ColumnSource::Scalar { .. } => {}
    }
}

fn relation_source_field(source: &RelationSource) -> Option<&ProtoField> {
    match source {
        RelationSource::Root { .. } => None,
        RelationSource::RepeatedMessage { field, .. }
        | RelationSource::OptionalMessage { field, .. }
        | RelationSource::OneofMessage { field, .. }
        | RelationSource::RepeatedValue { field, .. } => Some(field),
    }
}

fn bind_field(message: &MessageDef, field: &ProtoField) -> Result<FieldBinding, String> {
    let rust_field_ident = names::rust_snake(&field.name);
    let oneof = match &field.presence {
        Presence::Oneof { group_name } => {
            let group_index = message
                .descriptor
                .oneof_decl
                .iter()
                .position(|oneof| oneof.name.as_deref() == Some(group_name))
                .ok_or_else(|| format!("oneof group {group_name:?} is missing"))?;
            let type_name_conflict = message
                .descriptor
                .nested_type
                .iter()
                .filter_map(|nested| nested.name.as_deref())
                .chain(
                    message
                        .descriptor
                        .enum_type
                        .iter()
                        .filter_map(|nested| nested.name.as_deref()),
                )
                .any(|name| names::rust_upper_camel(name) == names::rust_upper_camel(group_name));
            let mut rust_oneof_type = names::rust_upper_camel(group_name);
            if type_name_conflict {
                rust_oneof_type.push_str("OneOf");
            }
            let mut message_path = message_rust_path(message)
                .split("::")
                .map(str::to_string)
                .collect::<Vec<_>>();
            message_path.pop();
            let containing_name = message
                .nesting
                .last()
                .expect("indexed message has a nesting component");
            message_path.push(names::rust_snake(containing_name));
            message_path.push(rust_oneof_type);

            let oneof = &message.descriptor.oneof_decl[group_index];
            let mut variants = BTreeMap::new();
            for descriptor in message.descriptor.field.iter().filter(|descriptor| {
                !descriptor.proto3_optional.unwrap_or(false)
                    && descriptor.oneof_index == Some(group_index as i32)
            }) {
                let proto_name = descriptor.name.as_deref().unwrap_or_default();
                let rust_name = names::rust_upper_camel(proto_name);
                if let Some(previous) = variants.insert(rust_name.clone(), proto_name.to_string()) {
                    return Err(format!(
                        "oneof {:?} variants {:?} and {:?} both bind to Rust variant {:?}",
                        oneof.name.as_deref().unwrap_or_default(),
                        previous,
                        proto_name,
                        rust_name
                    ));
                }
            }
            Some(OneofBinding {
                rust_group_ident: names::rust_snake(group_name),
                rust_enum_path: message_path.join("::"),
                rust_variant_ident: names::rust_upper_camel(&field.name),
            })
        }
        Presence::Implicit | Presence::Explicit => None,
    };
    Ok(FieldBinding {
        rust_field_ident,
        oneof,
    })
}

fn validate_message_identifiers(message: &MessageDef) -> Result<(), String> {
    let mut struct_fields = BTreeMap::new();
    for field in &message.descriptor.field {
        let synthetic = field.proto3_optional.unwrap_or(false);
        if field.oneof_index.is_some() && !synthetic {
            continue;
        }
        let proto_name = field.name.as_deref().unwrap_or_default();
        let rust_name = names::rust_snake(proto_name);
        if let Some(previous) = struct_fields.insert(rust_name.clone(), proto_name.to_string()) {
            return Err(format!(
                "protobuf fields {previous:?} and {proto_name:?} both bind to Rust field {rust_name:?}"
            ));
        }
    }
    for (oneof_index, oneof) in message.descriptor.oneof_decl.iter().enumerate() {
        let synthetic = message.descriptor.field.iter().any(|field| {
            field.proto3_optional.unwrap_or(false) && field.oneof_index == Some(oneof_index as i32)
        });
        if synthetic {
            continue;
        }
        let proto_name = oneof.name.as_deref().unwrap_or_default();
        let rust_name = names::rust_snake(proto_name);
        if let Some(previous) = struct_fields.insert(rust_name.clone(), proto_name.to_string()) {
            return Err(format!(
                "protobuf member {previous:?} and oneof {proto_name:?} both bind to Rust field {rust_name:?}"
            ));
        }
    }
    Ok(())
}

fn message_rust_path(message: &MessageDef) -> String {
    let mut parts = vec!["crate".to_string(), "proto".to_string()];
    parts.extend(
        message
            .package
            .split('.')
            .filter(|part| !part.is_empty())
            .map(names::rust_snake),
    );
    for enclosing in message
        .nesting
        .iter()
        .take(message.nesting.len().saturating_sub(1))
    {
        parts.push(names::rust_snake(enclosing));
    }
    parts.push(names::rust_upper_camel(
        message
            .nesting
            .last()
            .expect("indexed message has a nesting component"),
    ));
    parts.join("::")
}

fn plan_root_for_message<'a>(plan: &'a RelationalPlan, message_fqn: &str) -> &'a str {
    plan.relations
        .iter()
        .find(|relation| relation.message_fqn == message_fqn)
        .and_then(|relation| {
            root_index_for_relation(plan, relation.slot)
                .and_then(|root_index| plan.roots.get(root_index))
        })
        .map(|root| root.protobuf_fqn.as_str())
        .or_else(|| plan.roots.first().map(|root| root.protobuf_fqn.as_str()))
        .unwrap_or("<unknown>")
}

fn root_index_for_relation(plan: &RelationalPlan, mut slot: usize) -> Option<usize> {
    loop {
        match &plan.relations.get(slot)?.source {
            RelationSource::Root { root_index } => return Some(*root_index),
            source => slot = source.parent()?,
        }
    }
}
