use std::{collections::BTreeMap, future::Future, pin::Pin};

use arrow_array::RecordBatch;
use kat_rs_datasource::TraceDatasource;
use serde_json::Value;

use crate::{api::RunOutputDto, error::ApiError};

use super::{
    model::{
        ExecutionSnapshot, FlowResource, FlowStep, InputSpec, OutputBinding, RepeatCondition,
        Resource, RunStep,
    },
    sql::render_sql,
};

pub struct ExecutionResult {
    pub outputs: BTreeMap<String, RunOutputDto>,
    pub working_tables: BTreeMap<String, WorkingTable>,
}

#[derive(Clone)]
pub struct WorkingTable {
    pub batches: Vec<RecordBatch>,
}

pub async fn execute_snapshot(
    datasource: &TraceDatasource,
    snapshot: &ExecutionSnapshot,
    inputs: BTreeMap<String, Value>,
) -> Result<ExecutionResult, ApiError> {
    let mut executor = Executor {
        datasource,
        snapshot,
        values: inputs,
        tables: BTreeMap::new(),
    };
    let Resource::Flow(entry_flow) = &snapshot.entry.resource else {
        return Err(ApiError::validation("entry pack resource must be a flow"));
    };
    executor.apply_defaults(&entry_flow.inputs);
    executor.execute_flow(entry_flow).await?;
    let outputs = entry_flow
        .outputs
        .keys()
        .filter_map(|name| {
            executor.tables.get(name).map(|table| {
                (
                    name.clone(),
                    RunOutputDto {
                        kind: "table".to_string(),
                        name: name.clone(),
                        row_count: Some(table_row_count(table)),
                    },
                )
            })
        })
        .collect();

    Ok(ExecutionResult {
        outputs,
        working_tables: executor.tables,
    })
}

struct Executor<'a> {
    datasource: &'a TraceDatasource,
    snapshot: &'a ExecutionSnapshot,
    values: BTreeMap<String, Value>,
    tables: BTreeMap<String, WorkingTable>,
}

impl<'a> Executor<'a> {
    fn execute_flow<'b>(
        &'b mut self,
        flow: &'b FlowResource,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'b>> {
        Box::pin(async move {
            self.apply_defaults(&flow.inputs);
            for step in &flow.steps {
                self.execute_step(step).await?;
            }
            Ok(())
        })
    }

    fn execute_step<'b>(
        &'b mut self,
        step: &'b FlowStep,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'b>> {
        Box::pin(async move {
            match step {
                FlowStep::Run(step) => self.execute_run_step(step).await,
                FlowStep::IfEmpty(step) => {
                    let is_empty = self
                        .tables
                        .get(&step.if_empty)
                        .map(table_row_count)
                        .unwrap_or(0)
                        == 0;
                    let branch = if is_empty {
                        &step.then
                    } else {
                        &step.else_steps
                    };
                    for child in branch {
                        self.execute_step(child).await?;
                    }
                    Ok(())
                }
                FlowStep::RepeatUntil(step) => {
                    let max_iterations = repeat_max_iterations(&step.repeat_until, &self.values)?;
                    for _ in 0..max_iterations {
                        if repeat_empty_conditions_met(&step.repeat_until, &self.tables) {
                            break;
                        }
                        for child in &step.body {
                            self.execute_step(child).await?;
                        }
                    }
                    Ok(())
                }
            }
        })
    }

    async fn execute_run_step(&mut self, step: &RunStep) -> Result<(), ApiError> {
        let resource = self.snapshot.resources.get(&step.run).ok_or_else(|| {
            ApiError::validation(format!("snapshot resource missing: {}", step.run))
        })?;
        match &resource.resource {
            Resource::Flow(flow) => {
                let scoped_inputs =
                    self.resolve_inputs_with_defaults(&step.inputs, &flow.inputs)?;
                let saved_values = self.values.clone();
                self.values.extend(scoped_inputs);
                let result = self.execute_flow(flow).await;
                self.values = saved_values;
                result
            }
            Resource::Query(query) => {
                let inputs = self.resolve_inputs_with_defaults(&step.inputs, &query.inputs)?;
                let sql = render_sql(&query.sql, &inputs)?;
                let batches = self
                    .datasource
                    .query_batches(&sql)
                    .await
                    .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
                self.commit_outputs(&step.outputs, batches)?;
                Ok(())
            }
            Resource::Summaries(_) => Ok(()),
        }
    }

    fn resolve_inputs_with_defaults(
        &self,
        bindings: &BTreeMap<String, String>,
        spec: &InputSpec,
    ) -> Result<BTreeMap<String, Value>, ApiError> {
        let mut inputs = BTreeMap::new();
        for (input_name, value_name) in bindings {
            if self.tables.contains_key(value_name) {
                inputs.insert(input_name.clone(), Value::String(value_name.clone()));
            } else if let Some(value) = self.values.get(value_name) {
                inputs.insert(input_name.clone(), value.clone());
            } else {
                return Err(ApiError::validation(format!(
                    "run input {input_name} references undefined value {value_name}"
                )));
            }
        }
        for (name, default) in &spec.defaults {
            inputs
                .entry(name.clone())
                .or_insert_with(|| default.clone());
        }
        Ok(inputs)
    }

    fn apply_defaults(&mut self, spec: &InputSpec) {
        for (name, default) in &spec.defaults {
            self.values
                .entry(name.clone())
                .or_insert_with(|| default.clone());
        }
    }

    fn commit_outputs(
        &mut self,
        bindings: &BTreeMap<String, OutputBinding>,
        batches: Vec<RecordBatch>,
    ) -> Result<(), ApiError> {
        for binding in bindings.values() {
            let table = WorkingTable {
                batches: batches.clone(),
            };
            if let Some(set_name) = &binding.set {
                self.datasource
                    .register_record_batches(set_name, table.batches.clone())
                    .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
                self.tables.insert(set_name.clone(), table.clone());
            }
            if let Some(append_name) = &binding.append {
                let mut appended = self
                    .tables
                    .get(append_name)
                    .map(|table| table.batches.clone())
                    .unwrap_or_default();
                appended.extend(table.batches.clone());
                self.datasource
                    .register_record_batches(append_name, appended.clone())
                    .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
                self.tables
                    .insert(append_name.clone(), WorkingTable { batches: appended });
            }
        }
        Ok(())
    }
}

pub fn table_row_count(table: &WorkingTable) -> usize {
    table.batches.iter().map(RecordBatch::num_rows).sum()
}

fn positive_max_iterations(value: Option<u64>) -> Result<usize, ApiError> {
    value
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .ok_or_else(|| ApiError::validation("max_iterations must be greater than zero"))
}

fn repeat_max_iterations(
    conditions: &[RepeatCondition],
    values: &BTreeMap<String, Value>,
) -> Result<usize, ApiError> {
    for condition in conditions {
        if let RepeatCondition::MaxIterations { max_iterations } = condition {
            return match max_iterations {
                Value::Number(value) => positive_max_iterations(value.as_u64()),
                Value::String(name) => positive_max_iterations(
                    values.get(name).and_then(Value::as_u64),
                )
                .map_err(|_| {
                    ApiError::validation(format!(
                        "max_iterations references non-positive integer value {name}"
                    ))
                }),
                _ => Err(ApiError::validation("unsupported max_iterations value")),
            };
        }
    }
    Err(ApiError::validation("repeat_until missing max_iterations"))
}

fn repeat_empty_conditions_met(
    conditions: &[RepeatCondition],
    tables: &BTreeMap<String, WorkingTable>,
) -> bool {
    conditions.iter().any(|condition| match condition {
        RepeatCondition::Empty { empty } => {
            tables.get(empty).map(table_row_count).unwrap_or(0) == 0
        }
        RepeatCondition::MaxIterations { .. } => false,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use kat_rs_datasource::{TraceDatasource, materialize_sqlite_pack_demo_dataset};
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{repeat_empty_conditions_met, repeat_max_iterations};
    use crate::{
        error::ErrorCode,
        pack_runtime::model::{
            ExecutionSnapshot, FlowResource, InputSpec, LoadedResource, OutputBinding, OutputKind,
            QueryResource, RepeatCondition, Resource, ResourceKind, RunStep,
        },
    };

    #[test]
    fn repeat_max_iterations_reads_named_input_value() {
        let conditions = [RepeatCondition::MaxIterations {
            max_iterations: json!("max_iterations"),
        }];
        let values = BTreeMap::from([("max_iterations".to_string(), json!(8))]);

        let max_iterations =
            repeat_max_iterations(&conditions, &values).expect("max iterations resolves");

        assert_eq!(max_iterations, 8);
    }

    #[test]
    fn repeat_empty_conditions_treat_missing_tables_as_empty() {
        let conditions = [RepeatCondition::Empty {
            empty: "anchor_rows".to_string(),
        }];

        assert!(repeat_empty_conditions_met(&conditions, &BTreeMap::new()));
    }

    #[test]
    fn repeat_max_iterations_rejects_zero() {
        let conditions = [RepeatCondition::MaxIterations {
            max_iterations: json!(0),
        }];

        let error = repeat_max_iterations(&conditions, &BTreeMap::new())
            .err()
            .expect("zero should fail");

        assert_eq!(error.message, "max_iterations must be greater than zero");
    }

    #[tokio::test]
    async fn execute_snapshot_maps_append_registration_failures_to_query_failed() {
        let fixture = tempdir().expect("fixture tempdir is created");
        let sqlite_path = fixture.path().join("pack-demo.db");
        write_sqlite_fixture(&sqlite_path);
        let dataset_path = fixture.path().join("dataset");
        materialize_sqlite_pack_demo_dataset(&sqlite_path, &dataset_path)
            .await
            .expect("dataset materializes");
        let datasource = TraceDatasource::from_dataset(&dataset_path)
            .await
            .expect("dataset opens");

        let snapshot = ExecutionSnapshot {
            entry: loaded_resource(
                "local.flows.demo",
                Resource::Flow(FlowResource {
                    kind: ResourceKind::Flow,
                    description: "demo".to_string(),
                    inputs: empty_inputs(),
                    outputs: BTreeMap::new(),
                    steps: vec![
                        crate::pack_runtime::model::FlowStep::Run(RunStep {
                            run: "local.query.pids".to_string(),
                            inputs: BTreeMap::new(),
                            outputs: BTreeMap::from([(
                                "rows".to_string(),
                                OutputBinding {
                                    set: None,
                                    append: Some("shared".to_string()),
                                },
                            )]),
                        }),
                        crate::pack_runtime::model::FlowStep::Run(RunStep {
                            run: "local.query.names".to_string(),
                            inputs: BTreeMap::new(),
                            outputs: BTreeMap::from([(
                                "rows".to_string(),
                                OutputBinding {
                                    set: None,
                                    append: Some("shared".to_string()),
                                },
                            )]),
                        }),
                    ],
                    examples: Vec::new(),
                }),
            ),
            resources: BTreeMap::from([
                (
                    "local.query.pids".to_string(),
                    loaded_resource(
                        "local.query.pids",
                        Resource::Query(QueryResource {
                            kind: ResourceKind::Query,
                            description: "pids".to_string(),
                            inputs: empty_inputs(),
                            outputs: BTreeMap::from([("rows".to_string(), OutputKind::Table)]),
                            sql: "select pid from process".to_string(),
                        }),
                    ),
                ),
                (
                    "local.query.names".to_string(),
                    loaded_resource(
                        "local.query.names",
                        Resource::Query(QueryResource {
                            kind: ResourceKind::Query,
                            description: "names".to_string(),
                            inputs: empty_inputs(),
                            outputs: BTreeMap::from([("rows".to_string(), OutputKind::Table)]),
                            sql: "select name from process".to_string(),
                        }),
                    ),
                ),
            ]),
        };

        let error = super::execute_snapshot(&datasource, &snapshot, BTreeMap::new())
            .await
            .err()
            .expect("schema mismatch should fail");

        assert_eq!(error.code, ErrorCode::QueryFailed);
    }

    fn empty_inputs() -> InputSpec {
        InputSpec {
            required: BTreeMap::new(),
            optional: BTreeMap::new(),
            defaults: BTreeMap::new(),
        }
    }

    fn loaded_resource(coord: &str, resource: Resource) -> LoadedResource {
        LoadedResource {
            coord: coord.to_string(),
            path: format!("{coord}.yaml"),
            digest: "sha256:test".to_string(),
            resource,
        }
    }

    fn write_sqlite_fixture(path: &Path) {
        let connection = Connection::open(path).expect("sqlite opens");
        connection
            .execute_batch(
                "create table process(id int, ipid int, pid int, name text, start_ts int);
                 create table thread(id int, itid int, tid int, name text, start_ts int, end_ts int, ipid int, is_main_thread int);
                 create table callstack(id int, ts int, dur int, callid int, cat text, name text, depth int, parent_id int);
                 create table thread_state(id int, ts int, dur int, cpu int, itid int, tid int, pid int, state text);
                 create table instant(ts int, name text, ref int, wakeup_from int, ref_type text, value real);
                 insert into process(id, ipid, pid, name, start_ts) values (1, 89, 15040, '.tencent.wechat', 0);
                 insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (1, 405, 15040, '.tencent.wechat', 0, 0, 89, 1);
                 insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (1, 1000, 100, 405, 'H', 'HandleLaunchAbility##com.tencent.wechat', 0, null);
                 insert into thread_state(id, ts, dur, cpu, itid, tid, pid, state) values (1, 1100, 100, 0, 405, 15040, 15040, 'Sleeping');
                 insert into instant(ts, name, ref, wakeup_from, ref_type, value) values (1150, 'sched_wakeup', 405, 405, 'itid', null);",
            )
            .expect("sqlite fixture is written");
    }
}
