use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Result, bail};

use crate::{payload_value::PayloadValue, record::DecodedPayload};

use super::{
    descriptor::{FieldDescriptor, ProtoFieldType},
    plan::{ExpansionPlanItem, expansion_plan_for_roots, table_name},
    row::ColumnSpec,
    rules::ExpansionRule,
    sink::RelationalDatasetSink,
    table_batch::push_row_to_table,
    table_data::{
        append_table_values, append_value_row_values, collect_present_child_fields_at_path,
        json_child, leaf_field_descriptor, oneof_variant_object_value_at, serde_oneof_variant_key,
        table_columns, value_columns, visit_row_sources_at_path,
    },
};

pub(super) struct CompiledRootPlan {
    items: Vec<CompiledPlanItem>,
    dispatch_steps: Vec<DispatchStep>,
    optional_child_groups: Vec<Vec<String>>,
    oneof_variant_groups: Vec<Arc<CompiledOneofVariantGroup>>,
}

#[derive(Clone)]
pub(super) struct CompiledPlanItem {
    pub(super) item: ExpansionPlanItem,
    pub(super) parent_table_by_segment: Vec<Option<String>>,
    needs_parent_index: bool,
}

struct DispatchStep {
    action: DispatchAction,
    optional_child: Option<OptionalChildDispatch>,
}

enum DispatchAction {
    Item { item_index: usize },
    OneofVariantGroup { group_index: usize },
}

struct OptionalChildDispatch {
    group_index: usize,
    field: String,
}

#[derive(Clone)]
struct CompiledOneofVariantGroup {
    parent_path: Vec<String>,
    oneof_name: String,
    parent_table_by_segment: Vec<Option<String>>,
    variants: Vec<CompiledOneofVariant>,
    variant_by_json_key: HashMap<String, usize>,
    first_item_index: usize,
    item_indexes: HashSet<usize>,
}

#[derive(Clone)]
struct CompiledOneofVariant {
    item: ExpansionPlanItem,
    field: &'static FieldDescriptor,
    columns: Vec<ColumnSpec>,
    needs_parent_index: bool,
}

impl CompiledRootPlan {
    pub(super) fn new(root_message: &str) -> Result<Self> {
        let plan_items = expansion_plan_for_roots(&[root_message])?;
        reject_output_table_collisions(&plan_items)?;
        let mut items: Vec<_> = plan_items.into_iter().map(CompiledPlanItem::new).collect();
        let parent_index_tables = items
            .iter()
            .filter_map(|item| item.item.parent_table.as_ref())
            .cloned()
            .collect::<HashSet<_>>();
        for item in &mut items {
            item.needs_parent_index = parent_index_tables.contains(&item.item.output_table);
        }
        let oneof_variant_groups = compile_oneof_variant_groups(&items);
        let (dispatch_steps, optional_child_groups) =
            compile_dispatch_steps(&items, &oneof_variant_groups);
        Ok(Self {
            items,
            dispatch_steps,
            optional_child_groups,
            oneof_variant_groups,
        })
    }
}

fn reject_output_table_collisions(items: &[ExpansionPlanItem]) -> Result<()> {
    let mut source_paths = HashMap::<&str, &[String]>::new();
    for item in items {
        if let Some(previous) = source_paths.insert(&item.output_table, &item.source_path)
            && previous != item.source_path
        {
            bail!(
                "proto paths {} and {} map to the same Dataset table {}",
                previous.join("."),
                item.source_path.join("."),
                item.output_table
            );
        }
    }
    Ok(())
}

impl CompiledPlanItem {
    fn new(item: ExpansionPlanItem) -> Self {
        let mut parent_table_by_segment = vec![None; item.source_path.len()];
        if let Some(last) = parent_table_by_segment.last_mut() {
            *last = item.parent_table.clone();
        }
        Self {
            item,
            parent_table_by_segment,
            needs_parent_index: false,
        }
    }

    fn optional_child_field(&self) -> Option<(&[String], &str)> {
        if let Some(field) = self.oneof_variant_prerequisite() {
            return Some(field);
        }

        if self.item.rule != ExpansionRule::OneofVariant {
            return None;
        }
        let (field, parent_path) = self.item.source_path.split_last()?;
        if parent_path.is_empty() {
            return None;
        }
        Some((parent_path, field))
    }

    fn oneof_variant_prerequisite(&self) -> Option<(&[String], &str)> {
        let path = &self.item.source_path;
        for index in 0..path.len().saturating_sub(1) {
            let previous_table = table_name(&self.item.root_message, &path[..index]);
            let current_table = table_name(&self.item.root_message, &path[..=index]);
            if current_table == previous_table {
                return Some((&path[..=index], path[index + 1].as_str()));
            }
        }
        None
    }
}

fn compile_oneof_variant_groups(items: &[CompiledPlanItem]) -> Vec<Arc<CompiledOneofVariantGroup>> {
    let mut groups = Vec::<CompiledOneofVariantGroup>::new();

    for (item_index, item) in items.iter().enumerate() {
        if item.item.rule != ExpansionRule::OneofVariant {
            continue;
        }
        let Some((parent_path, oneof_name, field_name)) = oneof_path_parts(&item.item.source_path)
        else {
            continue;
        };
        let group_index = oneof_variant_group_index(&mut groups, parent_path, oneof_name);
        groups[group_index].push_variant(item_index, item, field_name);
    }

    groups.into_iter().map(Arc::new).collect()
}

fn oneof_path_parts(path: &[String]) -> Option<(&[String], &str, &str)> {
    let (field_name, parent_with_group) = path.split_last()?;
    let (oneof_name, parent_path) = parent_with_group.split_last()?;
    Some((parent_path, oneof_name, field_name))
}

fn oneof_variant_group_index(
    groups: &mut Vec<CompiledOneofVariantGroup>,
    parent_path: &[String],
    oneof_name: &str,
) -> usize {
    if let Some(index) = groups
        .iter()
        .position(|group| group.parent_path == parent_path && group.oneof_name == oneof_name)
    {
        return index;
    }

    let index = groups.len();
    groups.push(CompiledOneofVariantGroup::new(
        parent_path.to_vec(),
        oneof_name.to_string(),
    ));
    index
}

fn compile_dispatch_steps(
    items: &[CompiledPlanItem],
    oneof_variant_groups: &[Arc<CompiledOneofVariantGroup>],
) -> (Vec<DispatchStep>, Vec<Vec<String>>) {
    let mut optional_child_groups = Vec::<Vec<String>>::new();
    let mut dispatch_steps = Vec::with_capacity(items.len());
    for (item_index, item) in items.iter().enumerate() {
        if let Some((group_index, group)) = oneof_variant_groups
            .iter()
            .enumerate()
            .find(|(_, group)| group.item_indexes.contains(&item_index))
        {
            if group.first_item_index == item_index {
                dispatch_steps.push(DispatchStep {
                    action: DispatchAction::OneofVariantGroup { group_index },
                    optional_child: None,
                });
            }
            continue;
        }

        let optional_child =
            item.optional_child_field()
                .map(|(parent_path, field)| OptionalChildDispatch {
                    group_index: optional_child_group_index(
                        &mut optional_child_groups,
                        parent_path,
                    ),
                    field: field.to_string(),
                });
        dispatch_steps.push(DispatchStep {
            action: DispatchAction::Item { item_index },
            optional_child,
        });
    }

    (dispatch_steps, optional_child_groups)
}

fn optional_child_group_index(groups: &mut Vec<Vec<String>>, parent_path: &[String]) -> usize {
    if let Some(index) = groups.iter().position(|group| group == parent_path) {
        return index;
    }

    let index = groups.len();
    groups.push(parent_path.to_vec());
    index
}

impl CompiledOneofVariantGroup {
    fn new(parent_path: Vec<String>, oneof_name: String) -> Self {
        Self {
            parent_path,
            oneof_name,
            parent_table_by_segment: Vec::new(),
            variants: Vec::new(),
            variant_by_json_key: HashMap::new(),
            first_item_index: usize::MAX,
            item_indexes: HashSet::new(),
        }
    }

    fn push_variant(
        &mut self,
        item_index: usize,
        compiled_item: &CompiledPlanItem,
        field_name: &str,
    ) {
        if self.parent_table_by_segment.is_empty() {
            self.parent_table_by_segment = vec![None; self.parent_path.len()];
            if let Some(last) = self.parent_table_by_segment.last_mut() {
                *last = compiled_item.item.parent_table.clone();
            }
        }
        self.first_item_index = self.first_item_index.min(item_index);
        self.item_indexes.insert(item_index);

        let field = leaf_field_descriptor(&compiled_item.item.source_message, &compiled_item.item)
            .expect("oneof variant field descriptor resolves");
        let columns = match field.field_type {
            ProtoFieldType::Message(source_message) => {
                table_columns(source_message).expect("oneof message columns resolve")
            }
            ProtoFieldType::Scalar(_) => {
                value_columns(field).expect("oneof scalar columns resolve")
            }
        };
        let variant_index = self.variants.len();
        self.variant_by_json_key
            .insert(field_name.to_string(), variant_index);
        self.variant_by_json_key
            .insert(serde_oneof_variant_key(field_name), variant_index);
        self.variants.push(CompiledOneofVariant {
            item: compiled_item.item.clone(),
            field,
            columns,
            needs_parent_index: compiled_item.needs_parent_index,
        });
    }

    fn variant_value<'a>(
        &self,
        value: &'a PayloadValue,
    ) -> Option<(&CompiledOneofVariant, &'a PayloadValue)> {
        if let Some((json_key, value)) = oneof_variant_object_value_at(value, &self.oneof_name) {
            let variant_index = *self.variant_by_json_key.get(json_key)?;
            return Some((&self.variants[variant_index], value));
        }

        for variant in &self.variants {
            if let Some(value) = json_child(value, variant.field.name) {
                return Some((variant, value));
            }
        }

        None
    }
}

fn items_for_payload(plan: &CompiledRootPlan, payload: &DecodedPayload) -> Vec<PayloadDispatch> {
    let mut present_child_fields = vec![None; plan.optional_child_groups.len()];
    let mut items = Vec::with_capacity(plan.items.len());

    for step in &plan.dispatch_steps {
        if let Some(optional_child) = &step.optional_child {
            let present_fields = present_child_fields[optional_child.group_index]
                .get_or_insert_with(|| {
                    collect_present_child_fields_at_path(
                        &payload.message,
                        &plan.optional_child_groups[optional_child.group_index],
                    )
                });
            if !present_fields.contains(&optional_child.field) {
                continue;
            }
        }

        match step.action {
            DispatchAction::Item { item_index } => items.push(PayloadDispatch::Item {
                item: plan.items[item_index].clone(),
            }),
            DispatchAction::OneofVariantGroup { group_index } => {
                items.push(PayloadDispatch::OneofVariantGroup {
                    group: Arc::clone(&plan.oneof_variant_groups[group_index]),
                });
            }
        }
    }

    items
}

enum PayloadDispatch {
    Item {
        item: CompiledPlanItem,
    },
    OneofVariantGroup {
        group: Arc<CompiledOneofVariantGroup>,
    },
}

impl RelationalDatasetSink {
    pub(super) fn emit_payload(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
    ) -> Result<()> {
        let plan = self.ensure_root_plan(&payload.root_message)?;
        let dispatches = items_for_payload(plan, payload);

        for dispatch in dispatches {
            match dispatch {
                PayloadDispatch::Item { item } => {
                    self.emit_item(source_index, payload, &item)?;
                }
                PayloadDispatch::OneofVariantGroup { group } => {
                    self.emit_oneof_variant_group(source_index, payload, &group)?;
                }
            }
        }

        Ok(())
    }

    fn ensure_root_plan(&mut self, root_message: &str) -> Result<&CompiledRootPlan> {
        if !self.compiled_plans.contains_key(root_message) {
            let plan = CompiledRootPlan::new(root_message)?;
            self.compiled_plans.insert(root_message.to_string(), plan);
        }
        Ok(self
            .compiled_plans
            .get(root_message)
            .expect("compiled plan was inserted"))
    }

    fn emit_item(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        compiled_item: &CompiledPlanItem,
    ) -> Result<()> {
        let item = &compiled_item.item;
        match item.rule {
            ExpansionRule::RootRecord => {
                self.emit_root_record(source_index, payload, compiled_item)
            }
            ExpansionRule::RepeatedScalar => {
                self.emit_repeated_scalar(source_index, payload, compiled_item)
            }
            ExpansionRule::RepeatedMessage => {
                self.emit_repeated_message(source_index, payload, compiled_item)
            }
            ExpansionRule::OneofVariant => {
                self.emit_oneof_variant(source_index, payload, compiled_item)
            }
        }
    }

    fn emit_root_record(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        compiled_item: &CompiledPlanItem,
    ) -> Result<()> {
        let item = &compiled_item.item;
        let columns = table_columns(&item.source_message)?;
        let row_index = push_row_to_table(
            &self.table_writer,
            &mut self.tables,
            item,
            &columns,
            source_index,
            None,
            |builders| append_table_values(builders, &payload.message, &item.source_message),
        )?;
        if compiled_item.needs_parent_index {
            self.parent_indexes
                .entry(item.output_table.clone())
                .or_default()
                .insert(Default::default(), row_index);
        }
        Ok(())
    }

    fn emit_repeated_scalar(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        compiled_item: &CompiledPlanItem,
    ) -> Result<()> {
        let item = &compiled_item.item;
        let field = leaf_field_descriptor(&item.source_message, item)?;
        let columns = value_columns(field)?;
        let table_writer = &self.table_writer;
        let tables = &mut self.tables;
        let parent_indexes = &self.parent_indexes;

        visit_row_sources_at_path(
            payload,
            &item.source_path,
            &compiled_item.parent_table_by_segment,
            parent_indexes,
            &mut |source| {
                push_row_to_table(
                    table_writer,
                    tables,
                    item,
                    &columns,
                    source_index,
                    source.parent_index,
                    |builders| append_value_row_values(builders, source.value, field),
                )?;
                Ok(())
            },
        )?;

        Ok(())
    }

    fn emit_repeated_message(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        compiled_item: &CompiledPlanItem,
    ) -> Result<()> {
        let item = &compiled_item.item;
        let columns = table_columns(&item.source_message)?;
        let table_writer = &self.table_writer;
        let tables = &mut self.tables;
        let parent_indexes = &self.parent_indexes;
        let mut pending_parent_indexes = Vec::new();

        visit_row_sources_at_path(
            payload,
            &item.source_path,
            &compiled_item.parent_table_by_segment,
            parent_indexes,
            &mut |source| {
                let row_index = push_row_to_table(
                    table_writer,
                    tables,
                    item,
                    &columns,
                    source_index,
                    source.parent_index,
                    |builders| append_table_values(builders, source.value, &item.source_message),
                )?;
                if compiled_item.needs_parent_index {
                    pending_parent_indexes.push((source.ordinals, row_index));
                }
                Ok(())
            },
        )?;

        if compiled_item.needs_parent_index {
            self.parent_indexes
                .entry(item.output_table.clone())
                .or_default()
                .extend(pending_parent_indexes);
        }

        Ok(())
    }

    fn emit_oneof_variant_group(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        group: &CompiledOneofVariantGroup,
    ) -> Result<()> {
        let table_writer = &self.table_writer;
        let tables = &mut self.tables;
        let parent_indexes = &self.parent_indexes;
        let mut pending_parent_indexes = HashMap::<String, Vec<_>>::new();

        visit_row_sources_at_path(
            payload,
            &group.parent_path,
            &group.parent_table_by_segment,
            parent_indexes,
            &mut |source| {
                let Some((variant, value)) = group.variant_value(source.value) else {
                    return Ok(());
                };
                let row_index = push_row_to_table(
                    table_writer,
                    tables,
                    &variant.item,
                    &variant.columns,
                    source_index,
                    source.parent_index,
                    |builders| match variant.field.field_type {
                        ProtoFieldType::Message(source_message) => {
                            append_table_values(builders, value, source_message)
                        }
                        ProtoFieldType::Scalar(_) => {
                            append_value_row_values(builders, value, variant.field)
                        }
                    },
                )?;
                if variant.needs_parent_index {
                    pending_parent_indexes
                        .entry(variant.item.output_table.clone())
                        .or_default()
                        .push((source.ordinals, row_index));
                }
                Ok(())
            },
        )?;

        for (table_name, indexes) in pending_parent_indexes {
            self.parent_indexes
                .entry(table_name)
                .or_default()
                .extend(indexes);
        }

        Ok(())
    }

    fn emit_oneof_variant(
        &mut self,
        source_index: u64,
        payload: &DecodedPayload,
        compiled_item: &CompiledPlanItem,
    ) -> Result<()> {
        let item = &compiled_item.item;
        let field = leaf_field_descriptor(&item.source_message, item)?;
        let columns = match field.field_type {
            super::descriptor::ProtoFieldType::Message(source_message) => {
                table_columns(source_message)?
            }
            super::descriptor::ProtoFieldType::Scalar(_) => value_columns(field)?,
        };
        let table_writer = &self.table_writer;
        let tables = &mut self.tables;
        let parent_indexes = &self.parent_indexes;
        let mut pending_parent_indexes = Vec::new();

        visit_row_sources_at_path(
            payload,
            &item.source_path,
            &compiled_item.parent_table_by_segment,
            parent_indexes,
            &mut |source| {
                let row_index = push_row_to_table(
                    table_writer,
                    tables,
                    item,
                    &columns,
                    source_index,
                    source.parent_index,
                    |builders| match field.field_type {
                        super::descriptor::ProtoFieldType::Message(source_message) => {
                            append_table_values(builders, source.value, source_message)
                        }
                        super::descriptor::ProtoFieldType::Scalar(_) => {
                            append_value_row_values(builders, source.value, field)
                        }
                    },
                )?;
                if compiled_item.needs_parent_index {
                    pending_parent_indexes.push((source.ordinals, row_index));
                }
                Ok(())
            },
        )?;

        if compiled_item.needs_parent_index {
            self.parent_indexes
                .entry(item.output_table.clone())
                .or_default()
                .extend(pending_parent_indexes);
        }

        Ok(())
    }
}
