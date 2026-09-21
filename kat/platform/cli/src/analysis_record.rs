use std::collections::{BTreeMap, BTreeSet};

use miette::{Result, bail, miette};
use serde::{Deserialize, Serialize};

use crate::{
    run_manifest::PublishedRun,
    session_store::{RunId, SessionId},
    workflow_runtime::Column,
};

pub(super) const SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnalysisRecord {
    pub(super) schema_version: u32,
    pub(super) session_id: String,
    pub(super) revision: u64,
    pub(super) goal: Goal,
    pub(super) nodes: Vec<Node>,
    pub(super) materials: BTreeMap<String, Material>,
    pub(super) report: Option<Report>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Goal {
    pub(super) question: String,
    pub(super) scope: String,
    pub(super) gaps: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Node {
    pub(super) run_id: String,
    pub(super) content: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Material {
    Query {
        run_id: String,
        sql: String,
        outputs: Vec<String>,
        columns: Vec<Column>,
        ndjson: String,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    pub(super) content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SaveInput {
    #[serde(default, deserialize_with = "present")]
    pub(super) goal: Option<Goal>,
    #[serde(default, deserialize_with = "present")]
    nodes: Option<Vec<Node>>,
    #[serde(default, deserialize_with = "present")]
    report: Option<Report>,
}

// 省略表示保留原值，显式 null 不能静默变成未提交或删除已有正文。
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Serialize)]
pub(super) struct ReportNode {
    run_id: String,
    content: String,
    children: Vec<ReportNode>,
}

impl AnalysisRecord {
    pub(super) fn new(session_id: String, goal: Goal) -> Result<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            session_id,
            revision: 1,
            goal,
            nodes: Vec::new(),
            materials: BTreeMap::new(),
            report: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(super) fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!("unsupported Analysis Record schema_version");
        }
        if SessionId::parse(&self.session_id).is_none() {
            bail!("Invalid analysis record Session ID");
        }
        if self.revision == 0 {
            bail!("Analysis record revision must be positive");
        }
        require_text(&self.goal.question, "Analysis question")?;
        let mut run_ids = BTreeSet::new();
        for node in &self.nodes {
            validate_run_id(&node.run_id)?;
            require_text(&node.content, "Node content")?;
            if !run_ids.insert(node.run_id.as_str()) {
                bail!("Duplicate analysis node: {}", node.run_id);
            }
        }
        if let Some(report) = &self.report {
            require_text(&report.content, "Report content")?;
        }
        for (id, material) in &self.materials {
            require_text(id, "Material ID")?;
            let Material::Query {
                run_id,
                sql,
                outputs,
                ndjson,
                ..
            } = material;
            validate_run_id(run_id)?;
            require_text(sql, "Query SQL")?;
            for name in outputs {
                require_text(name, "Query Output name")?;
            }
            for line in ndjson.lines().filter(|line| !line.trim().is_empty()) {
                let mut deserializer = serde_json::Deserializer::from_str(line);
                serde::de::IgnoredAny::deserialize(&mut deserializer)
                    .and_then(|_| deserializer.end())
                    .map_err(|error| miette!("Invalid NDJSON in material {id}: {error}"))?;
            }
        }
        Ok(())
    }

    /// 正文只有增量替换语义，不从内容或树的位置推断解释依赖和有效性。
    pub(super) fn apply(
        &mut self,
        input: SaveInput,
        runs: &[PublishedRun],
    ) -> Result<Vec<ReportNode>> {
        if input.goal.is_none()
            && input.nodes.as_ref().is_none_or(Vec::is_empty)
            && input.report.is_none()
        {
            bail!("Analysis save must include a goal, node content, or report content");
        }
        if let Some(goal) = input.goal {
            self.goal = goal;
        }
        let mut written = BTreeSet::new();
        for node in input.nodes.unwrap_or_default() {
            if !written.insert(node.run_id.clone()) {
                bail!("A Run can only be saved once per request: {}", node.run_id);
            }
            if let Some(saved) = self
                .nodes
                .iter_mut()
                .find(|saved| saved.run_id == node.run_id)
            {
                *saved = node;
            } else {
                self.nodes.push(node);
            }
        }
        if let Some(report) = input.report {
            self.report = Some(report);
        }
        self.validate()?;
        self.report_tree(runs)
    }

    /// 仅选择真实直接父已选入的边，未选入的中间节点不被跨越。
    pub(super) fn report_tree(&self, runs: &[PublishedRun]) -> Result<Vec<ReportNode>> {
        let selected: BTreeMap<_, _> = self
            .nodes
            .iter()
            .map(|node| (node.run_id.as_str(), node))
            .collect();
        let published: BTreeMap<_, _> = runs.iter().map(|run| (run.run_id.as_str(), run)).collect();
        for run_id in selected.keys() {
            if !published.contains_key(run_id) {
                bail!("Run is not published in this Session: {run_id}");
            }
        }
        let mut parents = BTreeMap::new();
        for run in runs {
            for child in &run.child_runs {
                if parents
                    .insert(child.as_str(), run.run_id.as_str())
                    .is_some()
                {
                    bail!("Run has more than one published direct parent: {child}");
                }
            }
        }
        let mut children: BTreeMap<Option<&str>, Vec<&Node>> = BTreeMap::new();
        for node in &self.nodes {
            let parent = parents
                .get(node.run_id.as_str())
                .copied()
                .filter(|parent| selected.contains_key(parent));
            children.entry(parent).or_default().push(node);
        }
        let mut visited = BTreeSet::new();
        let tree = build_children(None, &children, &mut visited);
        if visited.len() != self.nodes.len() {
            bail!("Published Run relationships contain a report tree cycle");
        }
        Ok(tree)
    }
}

fn build_children<'a>(
    parent: Option<&str>,
    children: &BTreeMap<Option<&str>, Vec<&'a Node>>,
    visited: &mut BTreeSet<&'a str>,
) -> Vec<ReportNode> {
    children
        .get(&parent)
        .into_iter()
        .flatten()
        .filter_map(|node| {
            if !visited.insert(node.run_id.as_str()) {
                return None;
            }
            Some(ReportNode {
                run_id: node.run_id.clone(),
                content: node.content.clone(),
                children: build_children(Some(&node.run_id), children, visited),
            })
        })
        .collect()
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if RunId::parse(run_id).is_none() {
        bail!("Invalid analysis Run ID: {run_id}");
    }
    Ok(())
}

fn require_text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(())
}
