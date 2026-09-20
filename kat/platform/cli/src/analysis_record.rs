use std::collections::{BTreeMap, BTreeSet};

use miette::{Result, miette};
use serde::{Deserialize, Serialize};

use crate::{
    run_manifest::PublishedRun,
    session_store::{RunId, SessionId},
    workflow_runtime::Column,
};

const SCHEMA_VERSION: u32 = 1;

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
    pub(super) report_parent: Option<String>,
    pub(super) selection: Option<Selection>,
    pub(super) interpretation: Option<Interpretation>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Material {
    Guide {
        pack: String,
        workflow: String,
        guide: Option<String>,
    },
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
pub(super) struct Selection {
    pub(super) source_runs: Vec<String>,
    pub(super) materials: Vec<String>,
    pub(super) finding: String,
    pub(super) reason: String,
    pub(super) input_sources: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InterpretationInput {
    pub(super) facts: Vec<String>,
    pub(super) conclusion: String,
    pub(super) scope: String,
    pub(super) limitations: Vec<String>,
    pub(super) guide: String,
    pub(super) evidence: Vec<String>,
    pub(super) uses: Vec<InterpretationRef>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interpretation {
    pub(super) interpretation_version: u64,
    pub(super) state: State,
    pub(super) facts: Vec<String>,
    pub(super) conclusion: String,
    pub(super) scope: String,
    pub(super) limitations: Vec<String>,
    pub(super) guide: String,
    pub(super) evidence: Vec<String>,
    pub(super) uses: Vec<InterpretationRef>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InterpretationRef {
    pub(super) run_id: String,
    pub(super) interpretation_version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Current,
    Stale,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportInput {
    pub(super) content: String,
    pub(super) uses: Vec<InterpretationRef>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    pub(super) state: State,
    pub(super) content: String,
    pub(super) uses: Vec<InterpretationRef>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    SetGoal {
        goal: Goal,
    },
    SelectNode {
        run_id: String,
        report_parent: Option<String>,
        before: Option<String>,
    },
    SetSelection {
        run_id: String,
        selection: Option<Selection>,
    },
    SetInterpretation {
        run_id: String,
        interpretation: InterpretationInput,
    },
    SetReport {
        report: ReportInput,
    },
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

    /// 只校验保存内容；恢复历史不依赖当前 PACK 或 Output 是否仍可读取。
    pub(super) fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(miette!(
                "Unsupported analysis record schema version: {}",
                self.schema_version
            ));
        }
        if SessionId::parse(&self.session_id).is_none() {
            return Err(miette!("Invalid analysis record Session ID"));
        }
        if self.revision == 0 {
            return Err(miette!("Analysis record revision must be positive"));
        }
        require_text(&self.goal.question, "Analysis question")?;
        self.validate_materials()?;

        let mut run_ids = BTreeSet::new();
        for node in &self.nodes {
            validate_run_id(&node.run_id)?;
            if !run_ids.insert(node.run_id.as_str()) {
                return Err(miette!("Duplicate analysis node: {}", node.run_id));
            }
            if let Some(selection) = &node.selection {
                self.validate_selection(selection)?;
            }
        }
        for node in &self.nodes {
            if let Some(parent) = &node.report_parent
                && !run_ids.contains(parent.as_str())
            {
                return Err(miette!("Report parent is not a selected node: {parent}"));
            }
            let mut ancestors = BTreeSet::new();
            let mut current = Some(node.run_id.as_str());
            while let Some(run_id) = current {
                if !ancestors.insert(run_id) {
                    return Err(miette!("Report parent cycle at Run {run_id}"));
                }
                current = self.node(run_id)?.report_parent.as_deref();
            }
            if let Some(interpretation) = &node.interpretation {
                if interpretation.interpretation_version == 0 {
                    return Err(miette!("Interpretation version must be positive"));
                }
                require_text(&interpretation.conclusion, "Interpretation conclusion")?;
                self.validate_interpretation_materials(
                    &interpretation.guide,
                    &interpretation.evidence,
                )?;
                self.validate_refs(&interpretation.uses, interpretation.state)?;
                if interpretation
                    .uses
                    .iter()
                    .any(|dependency| dependency.run_id == node.run_id)
                {
                    return Err(miette!("Interpretation cannot use itself: {}", node.run_id));
                }
            }
        }
        self.validate_dependency_cycles()?;
        if let Some(report) = &self.report {
            require_text(&report.content, "Report content")?;
            self.validate_refs(&report.uses, report.state)?;
        }
        Ok(())
    }

    /// 调用者在候选副本上应用并提交，记录修订号由存储层统一管理。
    pub(super) fn apply(&mut self, changes: Vec<Change>, runs: &[PublishedRun]) -> Result<()> {
        self.validate()?;
        let published: BTreeMap<_, _> = runs.iter().map(|run| (run.run_id.as_str(), run)).collect();
        let mut replaced = BTreeSet::new();
        let writes_report = changes
            .iter()
            .any(|change| matches!(change, Change::SetReport { .. }));
        for change in &changes {
            if let Change::SetInterpretation { run_id, .. } = change
                && !replaced.insert(run_id.clone())
            {
                return Err(miette!(
                    "An interpretation can only be replaced once per update: {run_id}"
                ));
            }
        }
        // 不允许用请求内猜测的版本建立依赖，消费者必须在后续提交引用已保存解释。
        for change in &changes {
            let uses = match change {
                Change::SetInterpretation { interpretation, .. } => &interpretation.uses,
                Change::SetReport { report } => &report.uses,
                _ => continue,
            };
            for dependency in uses {
                if replaced.contains(dependency.run_id.as_str()) {
                    return Err(miette!(
                        "Cannot use an interpretation replaced in the same update: {}; save it first",
                        dependency.run_id
                    ));
                }
            }
            self.validate_refs(uses, State::Current)?;
        }

        for change in changes {
            match change {
                Change::SetGoal { goal } => {
                    require_text(&goal.question, "Analysis question")?;
                    if self.goal.question != goal.question || self.goal.scope != goal.scope {
                        for node in &mut self.nodes {
                            if let Some(interpretation) = &mut node.interpretation {
                                interpretation.state = State::Stale;
                            }
                        }
                        self.invalidate_report();
                    }
                    self.goal = goal;
                }
                Change::SelectNode {
                    run_id,
                    report_parent,
                    before,
                } => {
                    self.select_node(run_id, report_parent, before, &published)?;
                }
                Change::SetSelection { run_id, selection } => {
                    if let Some(selection) = &selection {
                        self.validate_selection(selection)?;
                        for source in &selection.source_runs {
                            published_run(&published, source)?;
                        }
                    }
                    self.node_mut(&run_id)?.selection = selection;
                }
                Change::SetInterpretation {
                    run_id,
                    interpretation,
                } => {
                    let run = published_run(&published, &run_id)?;
                    self.validate_interpretation_materials(
                        &interpretation.guide,
                        &interpretation.evidence,
                    )?;
                    self.validate_guide_identity(&interpretation.guide, run)?;
                    self.validate_refs(&interpretation.uses, State::Current)?;
                    require_text(&interpretation.conclusion, "Interpretation conclusion")?;
                    let node = self.node_mut(&run_id)?;
                    let version = node
                        .interpretation
                        .as_ref()
                        .map_or(0, |old| old.interpretation_version)
                        .checked_add(1)
                        .ok_or_else(|| {
                            miette!("Interpretation version exhausted for Run {run_id}")
                        })?;
                    node.interpretation = Some(Interpretation {
                        interpretation_version: version,
                        state: State::Current,
                        facts: interpretation.facts,
                        conclusion: interpretation.conclusion,
                        scope: interpretation.scope,
                        limitations: interpretation.limitations,
                        guide: interpretation.guide,
                        evidence: interpretation.evidence,
                        uses: interpretation.uses,
                    });
                    self.invalidate_dependents();
                }
                Change::SetReport { report } => {
                    require_text(&report.content, "Report content")?;
                    self.validate_refs(&report.uses, State::Current)?;
                    self.report = Some(Report {
                        state: State::Current,
                        content: report.content,
                        uses: report.uses,
                    });
                }
            }
        }
        // 新提交内容不能借助本批产生的失效逃过依赖校验，例如 B2 使用依赖 B1 的 C1。
        for run_id in replaced {
            if !self
                .node(&run_id)?
                .interpretation
                .as_ref()
                .is_some_and(|interpretation| interpretation.state == State::Current)
            {
                return Err(miette!(
                    "New interpretation was invalidated by this update: {run_id}; save valid dependencies first"
                ));
            }
        }
        if writes_report
            && !self
                .report
                .as_ref()
                .is_some_and(|report| report.state == State::Current)
        {
            return Err(miette!(
                "New report was invalidated by this update; save the report after its dependencies and organization"
            ));
        }
        self.validate()?;
        self.validate_published_nodes(&published)
    }

    fn node(&self, run_id: &str) -> Result<&Node> {
        self.nodes
            .iter()
            .find(|node| node.run_id == run_id)
            .ok_or_else(|| miette!("Run is not selected in the analysis record: {run_id}"))
    }

    fn node_mut(&mut self, run_id: &str) -> Result<&mut Node> {
        self.nodes
            .iter_mut()
            .find(|node| node.run_id == run_id)
            .ok_or_else(|| miette!("Run is not selected in the analysis record: {run_id}"))
    }

    fn validate_materials(&self) -> Result<()> {
        for (id, material) in &self.materials {
            require_text(id, "Material ID")?;
            match material {
                Material::Guide { pack, workflow, .. } => {
                    require_text(pack, "Guide PACK")?;
                    require_text(workflow, "Guide Workflow")?;
                }
                Material::Query {
                    run_id,
                    sql,
                    outputs,
                    ndjson,
                    ..
                } => {
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
            }
        }
        Ok(())
    }

    fn validate_selection(&self, selection: &Selection) -> Result<()> {
        let mut sources = BTreeSet::new();
        for source in &selection.source_runs {
            validate_run_id(source)?;
            if !sources.insert(source) {
                return Err(miette!("Duplicate selection source Run: {source}"));
            }
        }
        for id in &selection.materials {
            if !self.materials.contains_key(id) {
                return Err(miette!("Selection refers to missing material: {id}"));
            }
        }
        Ok(())
    }

    fn validate_interpretation_materials(&self, guide: &str, evidence: &[String]) -> Result<()> {
        if !matches!(self.materials.get(guide), Some(Material::Guide { .. })) {
            return Err(miette!(
                "Interpretation Guide must refer to a saved Guide material: {guide}"
            ));
        }
        for id in evidence {
            if !matches!(self.materials.get(id), Some(Material::Query { .. })) {
                return Err(miette!(
                    "Interpretation evidence must refer to a saved Query material: {id}"
                ));
            }
        }
        Ok(())
    }

    fn validate_guide_identity(&self, guide: &str, run: &PublishedRun) -> Result<()> {
        match self.materials.get(guide) {
            Some(Material::Guide { pack, workflow, .. })
                if pack == &run.pack && workflow == &run.workflow =>
            {
                Ok(())
            }
            _ => Err(miette!(
                "Guide material {guide} does not match Run {} PACK/Workflow",
                run.run_id
            )),
        }
    }

    fn validate_refs(&self, uses: &[InterpretationRef], state: State) -> Result<()> {
        let mut sources = BTreeSet::new();
        for dependency in uses {
            if !sources.insert(dependency.run_id.as_str()) {
                return Err(miette!(
                    "Duplicate interpretation dependency: {}",
                    dependency.run_id
                ));
            }
            let source = self
                .node(&dependency.run_id)?
                .interpretation
                .as_ref()
                .ok_or_else(|| {
                    miette!(
                        "Dependency Run has no saved interpretation: {}",
                        dependency.run_id
                    )
                })?;
            if dependency.interpretation_version == 0
                || dependency.interpretation_version > source.interpretation_version
            {
                return Err(miette!(
                    "Dependency references an unsaved interpretation version for Run {}",
                    dependency.run_id
                ));
            }
            if state == State::Current
                && (source.state != State::Current
                    || dependency.interpretation_version != source.interpretation_version)
            {
                return Err(miette!(
                    "Dependency interpretation is stale or its version changed: {}",
                    dependency.run_id
                ));
            }
        }
        Ok(())
    }

    fn validate_dependency_cycles(&self) -> Result<()> {
        let mut complete = BTreeSet::new();
        for node in &self.nodes {
            let mut pending = vec![(node.run_id.as_str(), false)];
            let mut active = BTreeSet::new();
            while let Some((run_id, exiting)) = pending.pop() {
                if exiting {
                    active.remove(run_id);
                    complete.insert(run_id);
                    continue;
                }
                if complete.contains(run_id) {
                    continue;
                }
                if !active.insert(run_id) {
                    return Err(miette!(
                        "Current interpretation dependency cycle at Run {run_id}"
                    ));
                }
                pending.push((run_id, true));
                if let Some(interpretation) = &self.node(run_id)?.interpretation
                    && interpretation.state == State::Current
                {
                    for dependency in &interpretation.uses {
                        pending.push((dependency.run_id.as_str(), false));
                    }
                }
            }
        }
        Ok(())
    }

    fn select_node(
        &mut self,
        run_id: String,
        report_parent: Option<String>,
        before: Option<String>,
        published: &BTreeMap<&str, &PublishedRun>,
    ) -> Result<()> {
        published_run(published, &run_id)?;
        if let Some(parent) = &report_parent {
            self.node(parent)?;
            let parent_run = published_run(published, parent)?;
            if !parent_run.child_runs.contains(&run_id) {
                return Err(miette!(
                    "Run {run_id} is not a published direct child of {parent}"
                ));
            }
        }
        if let Some(before) = &before {
            if before == &run_id || self.node(before)?.report_parent != report_parent {
                return Err(miette!("before must name another selected sibling"));
            }
        }
        let original_tree = self.tree_order();
        let mut node = if let Some(index) = self.nodes.iter().position(|node| node.run_id == run_id)
        {
            self.nodes.remove(index)
        } else {
            Node {
                run_id,
                report_parent: None,
                selection: None,
                interpretation: None,
            }
        };
        node.report_parent = report_parent;
        let position = before
            .as_deref()
            .and_then(|before| self.nodes.iter().position(|node| node.run_id == before));
        if let Some(position) = position {
            self.nodes.insert(position, node);
        } else {
            self.nodes.push(node);
        }
        if self.tree_order() != original_tree {
            self.invalidate_report();
        }
        Ok(())
    }

    fn tree_order(&self) -> BTreeMap<Option<String>, Vec<String>> {
        let mut order: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for node in &self.nodes {
            order
                .entry(node.report_parent.clone())
                .or_default()
                .push(node.run_id.clone());
        }
        order
    }

    fn validate_published_nodes(&self, published: &BTreeMap<&str, &PublishedRun>) -> Result<()> {
        for node in &self.nodes {
            let run = published_run(published, &node.run_id)?;
            if let Some(parent) = &node.report_parent
                && !published_run(published, parent)?
                    .child_runs
                    .contains(&node.run_id)
            {
                return Err(miette!(
                    "Run {} is not a published direct child of {parent}",
                    node.run_id
                ));
            }
            if let Some(interpretation) = &node.interpretation {
                self.validate_guide_identity(&interpretation.guide, run)?;
            }
        }
        Ok(())
    }

    fn invalidate_report(&mut self) {
        if let Some(report) = &mut self.report {
            report.state = State::Stale;
        }
    }

    fn invalidate_dependents(&mut self) {
        loop {
            let stale: BTreeSet<_> = self
                .nodes
                .iter()
                .filter_map(|node| {
                    let interpretation = node.interpretation.as_ref()?;
                    (interpretation.state == State::Current
                        && self
                            .validate_refs(&interpretation.uses, State::Current)
                            .is_err())
                    .then(|| node.run_id.clone())
                })
                .collect();
            if stale.is_empty() {
                break;
            }
            for node in &mut self.nodes {
                if stale.contains(&node.run_id)
                    && let Some(interpretation) = &mut node.interpretation
                {
                    interpretation.state = State::Stale;
                }
            }
        }
        if let Some(report) = &self.report
            && report.state == State::Current
            && self.validate_refs(&report.uses, State::Current).is_err()
        {
            self.invalidate_report();
        }
    }
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if RunId::parse(run_id).is_none() {
        return Err(miette!("Invalid analysis Run ID: {run_id}"));
    }
    Ok(())
}

fn require_text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(miette!("{name} must not be empty"));
    }
    Ok(())
}

fn published_run<'a>(
    published: &BTreeMap<&str, &'a PublishedRun>,
    run_id: &str,
) -> Result<&'a PublishedRun> {
    published
        .get(run_id)
        .copied()
        .ok_or_else(|| miette!("Run is not published in this Session: {run_id}"))
}
