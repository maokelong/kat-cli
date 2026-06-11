use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    sched_rows::{SchedSwitchRow, SchedWakeupNewRow, SchedWakeupRow, SchedWakingRow},
    sched_table_builders::SchedEventObserver,
};

use super::{HitraceTable, table_from_rows};

pub(super) const THREAD_STATE_TABLE: &str = "thread_state";
pub(super) const INSTANT_TABLE: &str = "instant";

#[derive(Default)]
pub(super) struct DerivedTables {
    thread_state: ThreadStateBuilder,
    instant: Vec<InstantRow>,
}

impl SchedEventObserver for DerivedTables {
    fn observe_sched_switch(&mut self, row: &SchedSwitchRow) {
        self.thread_state.push_switch(row);
    }

    fn observe_sched_wakeup(&mut self, row: &SchedWakeupRow) {
        self.instant.push(InstantRow::from_wakeup(
            row.event_timestamp,
            "sched_wakeup",
            row.pid,
            row.event_tgid,
        ));
    }

    fn observe_sched_wakeup_new(&mut self, row: &SchedWakeupNewRow) {
        self.instant.push(InstantRow::from_wakeup(
            row.event_timestamp,
            "sched_wakeup_new",
            row.pid,
            row.event_tgid,
        ));
    }

    fn observe_sched_waking(&mut self, row: &SchedWakingRow) {
        self.instant.push(InstantRow::from_wakeup(
            row.event_timestamp,
            "sched_waking",
            row.pid,
            row.event_tgid,
        ));
    }
}

impl DerivedTables {
    pub(super) fn into_tables(self) -> Result<Vec<HitraceTable>> {
        Ok(vec![
            table_from_rows(THREAD_STATE_TABLE, self.thread_state.into_rows())?,
            table_from_rows(INSTANT_TABLE, self.instant)?,
        ])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ThreadStateRow {
    ts: u64,
    dur: Option<u64>,
    cpu: Option<u32>,
    tid: i32,
    state: String,
    comm: String,
}

#[derive(Default)]
pub(super) struct ThreadStateBuilder {
    rows: Vec<ThreadStateRow>,
    active_by_tid: HashMap<i32, usize>,
}

impl ThreadStateBuilder {
    pub(super) fn push_switch(&mut self, row: &SchedSwitchRow) {
        if row.prev_pid != 0 {
            self.push_state(
                row.event_timestamp,
                None,
                row.prev_pid,
                format!("prev_state:{}", row.prev_state),
                row.prev_comm.clone(),
            );
        }
        if row.next_pid != 0 {
            self.push_state(
                row.event_timestamp,
                Some(row.event_cpu),
                row.next_pid,
                "Running".to_string(),
                row.next_comm.clone(),
            );
        }
    }

    fn push_state(&mut self, ts: u64, cpu: Option<u32>, tid: i32, state: String, comm: String) {
        if let Some(active_row) = self.active_by_tid.insert(tid, self.rows.len()) {
            let start_ts = self.rows[active_row].ts;
            if ts >= start_ts {
                self.rows[active_row].dur = Some(ts - start_ts);
            }
        }

        self.rows.push(ThreadStateRow {
            ts,
            dur: None,
            cpu,
            tid,
            state,
            comm,
        });
    }

    pub(super) fn into_rows(self) -> Vec<ThreadStateRow> {
        self.rows
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct InstantRow {
    ts: u64,
    name: String,
    #[serde(rename = "ref")]
    ref_tid: i32,
    wakeup_from: i32,
    ref_type: String,
    value: f64,
}

impl InstantRow {
    pub(super) fn from_wakeup(ts: u64, name: &str, ref_tid: i32, wakeup_from: i32) -> Self {
        Self {
            ts,
            name: name.to_string(),
            ref_tid,
            wakeup_from,
            ref_type: "tid".to_string(),
            value: 0.0,
        }
    }
}
