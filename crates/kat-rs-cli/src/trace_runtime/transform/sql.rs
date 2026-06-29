use std::{
    collections::BTreeSet,
    fs,
    ops::ControlFlow,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sqlparser::{
    ast::{ObjectName, Query, Statement, Visit, Visitor, visit_relations},
    dialect::SQLiteDialect,
    parser::Parser,
};

use crate::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::sql::scalar_literal},
    pack::spec::SqlViewTransformSpec,
};

pub fn run_sql_view_transform(
    adapter: &mut dyn DatasetAdapter,
    pack_root: &Path,
    spec: &SqlViewTransformSpec,
    params: &Value,
) -> Result<()> {
    for table in spec.inputs.table_names() {
        if !adapter.table_exists(table)? {
            bail!(
                "transform `{}` input table does not exist: {table}",
                spec.id
            );
        }
    }

    let sql_path = safe_pack_relative_path(&spec.sql)?;
    let full_sql_path = pack_root.join(sql_path);
    let raw_sql = fs::read_to_string(&full_sql_path)
        .with_context(|| format!("failed to read {}", full_sql_path.display()))?;
    let rendered = render_sql(&raw_sql, params)?;
    validate_allowed_tables(&rendered, &spec.safety.allowed_tables)?;
    adapter.create_derived_table_as(&spec.output.table, &rendered)?;
    Ok(())
}

fn safe_pack_relative_path(path: &Path) -> Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            _ => bail!("unsafe sql path: {}", path.display()),
        }
    }
    if safe.as_os_str().is_empty() {
        bail!("unsafe sql path: {}", path.display());
    }
    Ok(safe)
}

fn render_sql(template: &str, params: &Value) -> Result<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            bail!("unterminated SQL placeholder");
        };
        let name = after_start[..end].trim();
        rendered.push_str(&scalar_literal(params.get(name))?);
        rest = &after_start[end + 1..];
    }
    rendered.push_str(rest);
    Ok(rendered)
}

fn validate_allowed_tables(sql: &str, allowed_tables: &[String]) -> Result<()> {
    if allowed_tables.is_empty() {
        return Ok(());
    }
    let allowed = allowed_tables
        .iter()
        .map(|table| table.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let disallowed = referenced_tables(sql)?
        .into_iter()
        .filter(|table| !allowed.contains(table))
        .collect::<Vec<_>>();
    if !disallowed.is_empty() {
        bail!(
            "SQL references tables outside safety.allowedTables: {}",
            disallowed.join(", ")
        );
    }
    Ok(())
}

fn referenced_tables(sql: &str) -> Result<BTreeSet<String>> {
    let statements = Parser::parse_sql(&SQLiteDialect {}, sql)
        .context("failed to parse SQL for table references")?;
    let cte_aliases = cte_aliases(&statements);
    let mut tables = BTreeSet::new();
    let _: ControlFlow<(), ()> = visit_relations(&statements, |relation| {
        if let Some(table) = relation_name(relation) {
            if relation.0.len() != 1 || !cte_aliases.contains(&table) {
                tables.insert(table);
            }
        }
        ControlFlow::Continue(())
    });
    Ok(tables)
}

fn cte_aliases(statements: &[Statement]) -> BTreeSet<String> {
    struct CteAliasVisitor {
        aliases: BTreeSet<String>,
    }

    impl Visitor for CteAliasVisitor {
        type Break = ();

        fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
            if let Some(with) = &query.with {
                for cte in &with.cte_tables {
                    self.aliases
                        .insert(cte.alias.name.value.to_ascii_lowercase());
                }
            }
            ControlFlow::Continue(())
        }
    }

    let mut visitor = CteAliasVisitor {
        aliases: BTreeSet::new(),
    };
    for statement in statements {
        let _: ControlFlow<(), ()> = statement.visit(&mut visitor);
    }
    visitor.aliases
}

fn relation_name(relation: &ObjectName) -> Option<String> {
    relation
        .0
        .last()
        .map(ToString::to_string)
        .map(|name| name.trim_matches('"').to_ascii_lowercase())
        .filter(|name| !name.is_empty())
}
