use std::sync::Arc;

use anyhow::{Context, Result, bail};
use arrow_schema::{DataType, Field, Schema};

use super::{
    EnumOriginSpec, EstimatedRow, PreparedSourceTables, RelationSlot, RelationSpec, SpoolOptions,
    spec::{PROTOBUF_ENUM_SYMBOL_TABLE, validate_specs},
    spool::{ActiveTable, PreparedSourceTable},
};

struct RelationState {
    spec: RelationSpec,
    active: Option<ActiveTable>,
    next_row_id: Option<u64>,
}

/// 构建期生成的 descriptor-derived relation 布局。
///
/// 调用者只能把它整体交给 capture adapter 或直接构造 capture，不能读取、重排或追加
/// relation slots。Plan 外 provenance relation 由所属 adapter 在本模块内部组合。
pub(crate) struct SourceTableLayout {
    relations: Vec<RelationSpec>,
    enum_origins: Vec<EnumOriginSpec>,
}

impl SourceTableLayout {
    pub(crate) fn from_generated(
        relations: Vec<RelationSpec>,
        enum_origins: Vec<EnumOriginSpec>,
    ) -> Self {
        Self {
            relations,
            enum_origins,
        }
    }

    pub(super) fn append_relation(&mut self, relation: RelationSpec) -> RelationSlot {
        let slot = RelationSlot::new(self.relations.len());
        self.relations.push(relation);
        slot
    }

    pub(super) fn append_enum_origin(&mut self, origin: EnumOriginSpec) {
        self.enum_origins.push(origin);
    }

    pub(crate) fn into_capture(self, options: SpoolOptions) -> Result<SourceTableCapture> {
        SourceTableCapture::new(self.relations, self.enum_origins, options)
    }
}

pub(crate) struct SourceTableCapture {
    relations: Vec<RelationState>,
    enum_origins: Vec<EnumOriginSpec>,
    options: SpoolOptions,
    poisoned: Option<String>,
}

impl SourceTableCapture {
    pub(crate) fn new(
        relations: Vec<RelationSpec>,
        enum_origins: Vec<EnumOriginSpec>,
        options: SpoolOptions,
    ) -> Result<Self> {
        options.validate()?;
        validate_specs(&relations, &enum_origins)?;
        Ok(Self {
            relations: relations
                .into_iter()
                .map(|spec| RelationState {
                    spec,
                    active: None,
                    next_row_id: Some(0),
                })
                .collect(),
            enum_origins,
            options,
            poisoned: None,
        })
    }

    /// Row ID 只在指定 relation 内递增；达到 UInt64 上限后下一次分配失败。
    pub(crate) fn allocate_row_id(&mut self, relation: RelationSlot) -> Result<u64> {
        self.ensure_healthy()?;
        let result = (|| {
            let state = self.relation_mut(relation)?;
            let row_id = state.next_row_id.take().with_context(|| {
                format!(
                    "protobuf Source row id overflows in relation {:?}",
                    state.spec.name
                )
            })?;
            state.next_row_id = row_id.checked_add(1);
            Ok(row_id)
        })();
        match result {
            Ok(row_id) => Ok(row_id),
            Err(error) => {
                self.poisoned = Some(format!(
                    "relation slot {} row-id allocation failed: {error:#}",
                    relation.index()
                ));
                Err(error)
            }
        }
    }

    pub(crate) fn append_row<T>(&mut self, relation: RelationSlot, row: &T) -> Result<()>
    where
        T: EstimatedRow,
    {
        self.ensure_healthy()?;
        let options = self.options;
        let result = (|| {
            let state = self.relation_mut(relation)?;
            if state.active.is_none() {
                state.active = Some(ActiveTable::new(state.spec.clone(), options)?);
            }
            state
                .active
                .as_mut()
                .expect("protobuf Source relation spool is initialized")
                .append_row(row)
        })();
        if let Err(error) = result {
            self.poisoned = Some(format!(
                "relation slot {} append failed: {error:#}",
                relation.index()
            ));
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<PreparedSourceTables> {
        if let Some(source) = self.poisoned {
            bail!("protobuf Source capture is poisoned by an earlier failure: {source}");
        }

        let active_relations = self
            .relations
            .iter()
            .map(|state| state.active.as_ref().is_some_and(ActiveTable::has_rows))
            .collect::<Vec<_>>();
        let enum_definitions =
            collect_enum_definitions(&self.relations, &active_relations, &self.enum_origins);

        let mut prepared = Vec::<PreparedSourceTable>::new();
        for state in self.relations {
            if let Some(table) = state.active {
                prepared.push(table.prepare()?);
            }
        }
        if !enum_definitions.is_empty() {
            let mut table = ActiveTable::new(protobuf_enum_symbol_spec(), self.options)?;
            for definition in enum_definitions {
                table.append_row(&definition)?;
            }
            prepared.push(table.prepare()?);
        }

        Ok(PreparedSourceTables::new(prepared))
    }

    fn ensure_healthy(&self) -> Result<()> {
        if let Some(source) = &self.poisoned {
            bail!("protobuf Source capture is poisoned by an earlier failure: {source}");
        }
        Ok(())
    }

    fn relation_mut(&mut self, relation: RelationSlot) -> Result<&mut RelationState> {
        self.relations.get_mut(relation.index()).with_context(|| {
            format!(
                "protobuf Source capture has no relation slot {}",
                relation.index()
            )
        })
    }
}

#[derive(Clone, Copy, serde::Serialize)]
struct EnumDefinition {
    origin_table: &'static str,
    origin_field_path: &'static str,
    enum_type_name: &'static str,
    enum_number: i32,
    enum_symbol: &'static str,
}

impl EstimatedRow for EnumDefinition {
    fn estimated_bytes(&self) -> Result<usize> {
        use super::EstimatedValue;

        let mut total = 0;
        for bytes in [
            self.origin_table.estimated_bytes()?,
            self.origin_field_path.estimated_bytes()?,
            self.enum_type_name.estimated_bytes()?,
            self.enum_number.estimated_bytes()?,
            self.enum_symbol.estimated_bytes()?,
        ] {
            super::add_estimated_bytes(&mut total, bytes)?;
        }
        Ok(total)
    }
}

fn collect_enum_definitions(
    relations: &[RelationState],
    active_relations: &[bool],
    origins: &[EnumOriginSpec],
) -> Vec<EnumDefinition> {
    let mut definitions = Vec::new();
    for origin in origins {
        if !active_relations[origin.relation.index()] {
            continue;
        }
        let origin_table = relations[origin.relation.index()].spec.name;
        definitions.extend(origin.symbols.iter().map(|symbol| EnumDefinition {
            origin_table,
            origin_field_path: origin.field_path,
            enum_type_name: origin.enum_type_name,
            enum_number: symbol.number,
            enum_symbol: symbol.symbol,
        }));
    }
    definitions.sort_by(|left, right| {
        (left.origin_table, left.origin_field_path, left.enum_number).cmp(&(
            right.origin_table,
            right.origin_field_path,
            right.enum_number,
        ))
    });
    definitions
}

fn protobuf_enum_symbol_spec() -> RelationSpec {
    RelationSpec::new(
        PROTOBUF_ENUM_SYMBOL_TABLE,
        Arc::new(Schema::new(vec![
            Field::new("origin_table", DataType::Utf8, false),
            Field::new("origin_field_path", DataType::Utf8, false),
            Field::new("enum_type_name", DataType::Utf8, false),
            Field::new("enum_number", DataType::Int32, false),
            Field::new("enum_symbol", DataType::Utf8, false),
        ])),
    )
}
