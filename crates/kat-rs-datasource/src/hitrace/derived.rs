use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{sched_rows::*, sched_table_builders::SchedEventObserver};

use super::{HitraceTable, table_from_rows};

pub(super) const PROCESS_TABLE: &str = "process";
pub(super) const THREAD_TABLE: &str = "thread";
pub(super) const THREAD_STATE_TABLE: &str = "thread_state";
pub(super) const INSTANT_TABLE: &str = "instant";
pub(super) const SCHED_SLICE_TABLE: &str = "sched_slice";
pub(super) const RAW_EVENT_TABLE: &str = "raw_event";

#[derive(Default)]
pub(super) struct DerivedTables {
    threads: ThreadProcessIndex,
    thread_state: ThreadStateBuilder,
    sched_slice: SchedSliceBuilder,
    instant: Vec<InstantRow>,
    raw_event: Vec<RawEventRow>,
}

impl SchedEventObserver for DerivedTables {
    fn observe_sched_blocked_reason(&mut self, row: &SchedBlockedReasonRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, None);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_blocked_reason",
            json!({ "pid": row.pid, "caller": row.caller, "io_wait": row.io_wait }),
        );
    }

    fn observe_sched_kthread_stop(&mut self, row: &SchedKthreadStopRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_kthread_stop",
            json!({ "comm": row.comm, "pid": row.pid }),
        );
    }

    fn observe_sched_kthread_stop_ret(&mut self, row: &SchedKthreadStopRetRow) {
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            None,
            "sched_kthread_stop_ret",
            json!({ "ret": row.ret }),
        );
    }

    fn observe_sched_migrate_task(&mut self, row: &SchedMigrateTaskRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_migrate_task",
            json!({
                "comm": row.comm,
                "pid": row.pid,
                "prio": row.prio,
                "orig_cpu": row.orig_cpu,
                "dest_cpu": row.dest_cpu,
            }),
        );
    }

    fn observe_sched_move_numa(&mut self, row: &SchedMoveNumaRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, None);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_move_numa",
            json!({ "pid": row.pid, "tgid": row.tgid, "src_cpu": row.src_cpu, "dst_cpu": row.dst_cpu }),
        );
    }

    fn observe_sched_pi_setprio(&mut self, row: &SchedPiSetprioRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_pi_setprio",
            json!({ "comm": row.comm, "pid": row.pid, "oldprio": row.oldprio, "newprio": row.newprio }),
        );
    }

    fn observe_sched_process_exec(&mut self, row: &SchedProcessExecRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.filename));
        self.threads
            .mark_thread_ended(row.event_timestamp, row.old_pid);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_process_exec",
            json!({ "filename": row.filename, "pid": row.pid, "old_pid": row.old_pid }),
        );
    }

    fn observe_sched_process_exit(&mut self, row: &SchedProcessExitRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.threads.mark_thread_ended(row.event_timestamp, row.pid);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_process_exit",
            json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio }),
        );
    }

    fn observe_sched_process_fork(&mut self, row: &SchedProcessForkRow) {
        self.threads.get_or_create_thread(
            row.event_timestamp,
            row.parent_pid,
            Some(&row.parent_comm),
        );
        self.threads.get_or_create_thread(
            row.event_timestamp,
            row.child_pid,
            Some(&row.child_comm),
        );
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.child_pid),
            "sched_process_fork",
            json!({
                "parent_comm": row.parent_comm,
                "parent_pid": row.parent_pid,
                "child_comm": row.child_comm,
                "child_pid": row.child_pid,
            }),
        );
    }

    fn observe_sched_process_free(&mut self, row: &SchedProcessFreeRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.threads.mark_thread_ended(row.event_timestamp, row.pid);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_process_free",
            json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio }),
        );
    }

    fn observe_sched_process_wait(&mut self, row: &SchedProcessWaitRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_process_wait",
            json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio }),
        );
    }

    fn observe_sched_stat_blocked(&mut self, row: &SchedStatBlockedRow) {
        self.observe_sched_stat(
            "sched_stat_blocked",
            row.event_timestamp,
            row.event_cpu,
            row.pid,
            &row.comm,
            row.delay,
        );
    }

    fn observe_sched_stat_iowait(&mut self, row: &SchedStatIowaitRow) {
        self.observe_sched_stat(
            "sched_stat_iowait",
            row.event_timestamp,
            row.event_cpu,
            row.pid,
            &row.comm,
            row.delay,
        );
    }

    fn observe_sched_stat_runtime(&mut self, row: &SchedStatRuntimeRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_stat_runtime",
            json!({ "comm": row.comm, "pid": row.pid, "runtime": row.runtime, "vruntime": row.vruntime }),
        );
    }

    fn observe_sched_stat_sleep(&mut self, row: &SchedStatSleepRow) {
        self.observe_sched_stat(
            "sched_stat_sleep",
            row.event_timestamp,
            row.event_cpu,
            row.pid,
            &row.comm,
            row.delay,
        );
    }

    fn observe_sched_stat_wait(&mut self, row: &SchedStatWaitRow) {
        self.observe_sched_stat(
            "sched_stat_wait",
            row.event_timestamp,
            row.event_cpu,
            row.pid,
            &row.comm,
            row.delay,
        );
    }

    fn observe_sched_stick_numa(&mut self, row: &SchedStickNumaRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, None);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_stick_numa",
            json!({ "pid": row.pid, "tgid": row.tgid, "src_cpu": row.src_cpu, "dst_cpu": row.dst_cpu }),
        );
    }

    fn observe_sched_swap_numa(&mut self, row: &SchedSwapNumaRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.src_pid, None);
        self.threads
            .get_or_create_thread(row.event_timestamp, row.dst_pid, None);
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.dst_pid),
            "sched_swap_numa",
            json!({ "src_pid": row.src_pid, "dst_pid": row.dst_pid, "src_cpu": row.src_cpu, "dst_cpu": row.dst_cpu }),
        );
    }

    fn observe_sched_switch(&mut self, row: &SchedSwitchRow) {
        let prev_thread = (row.prev_pid != 0).then(|| {
            self.threads.get_or_create_thread(
                row.event_timestamp,
                row.prev_pid,
                Some(&row.prev_comm),
            )
        });
        let next_thread = (row.next_pid != 0).then(|| {
            self.threads.get_or_create_thread(
                row.event_timestamp,
                row.next_pid,
                Some(&row.next_comm),
            )
        });

        if let Some(thread) = prev_thread {
            self.thread_state.push_state(
                row.event_timestamp,
                None,
                thread,
                format!("prev_state:{}", row.prev_state),
                row.prev_comm.clone(),
            );
        }
        if let Some(thread) = next_thread {
            self.threads.increment_thread_switch(thread.itid);
            self.thread_state.push_state(
                row.event_timestamp,
                Some(row.event_cpu),
                thread,
                "Running".to_string(),
                row.next_comm.clone(),
            );
            self.sched_slice.push_switch(
                row.event_timestamp,
                row.event_cpu,
                row.prev_state,
                thread,
                row.next_prio,
            );
        } else {
            self.sched_slice
                .close_cpu(row.event_timestamp, row.event_cpu, row.prev_state);
        }

        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.next_pid),
            "sched_switch",
            json!({
                "prev_comm": row.prev_comm,
                "prev_pid": row.prev_pid,
                "prev_prio": row.prev_prio,
                "prev_state": row.prev_state,
                "next_comm": row.next_comm,
                "next_pid": row.next_pid,
                "next_prio": row.next_prio,
            }),
        );
    }

    fn observe_sched_wait_task(&mut self, row: &SchedWaitTaskRow) {
        self.threads
            .get_or_create_thread(row.event_timestamp, row.pid, Some(&row.comm));
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            Some(row.pid),
            "sched_wait_task",
            json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio }),
        );
    }

    fn observe_sched_wake_idle_without_ipi(&mut self, row: &SchedWakeIdleWithoutIpiRow) {
        self.push_raw_event(
            row.event_timestamp,
            row.event_cpu,
            None,
            "sched_wake_idle_without_ipi",
            json!({ "cpu": row.cpu }),
        );
    }

    fn observe_sched_wakeup(&mut self, row: &SchedWakeupRow) {
        self.push_wakeup(WakeupEvent {
            ts: row.event_timestamp,
            cpu: row.event_cpu,
            event_name: "sched_wakeup",
            target_tid: row.pid,
            target_comm: &row.comm,
            source_tid: row.event_tgid,
            source_comm: &row.event_comm,
            payload: json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio, "success": row.success, "target_cpu": row.target_cpu }),
        });
    }

    fn observe_sched_wakeup_new(&mut self, row: &SchedWakeupNewRow) {
        self.push_wakeup(WakeupEvent {
            ts: row.event_timestamp,
            cpu: row.event_cpu,
            event_name: "sched_wakeup_new",
            target_tid: row.pid,
            target_comm: &row.comm,
            source_tid: row.event_tgid,
            source_comm: &row.event_comm,
            payload: json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio, "success": row.success, "target_cpu": row.target_cpu }),
        });
    }

    fn observe_sched_waking(&mut self, row: &SchedWakingRow) {
        self.push_wakeup(WakeupEvent {
            ts: row.event_timestamp,
            cpu: row.event_cpu,
            event_name: "sched_waking",
            target_tid: row.pid,
            target_comm: &row.comm,
            source_tid: row.event_tgid,
            source_comm: &row.event_comm,
            payload: json!({ "comm": row.comm, "pid": row.pid, "prio": row.prio, "success": row.success, "target_cpu": row.target_cpu }),
        });
    }
}

impl DerivedTables {
    pub(super) fn into_tables(self) -> Result<Vec<HitraceTable>> {
        let (process_rows, thread_rows) = self.threads.into_rows();
        Ok(vec![
            table_from_rows(PROCESS_TABLE, process_rows)?,
            table_from_rows(THREAD_TABLE, thread_rows)?,
            table_from_rows(THREAD_STATE_TABLE, self.thread_state.into_rows())?,
            table_from_rows(INSTANT_TABLE, self.instant)?,
            table_from_rows(SCHED_SLICE_TABLE, self.sched_slice.into_rows())?,
            table_from_rows(RAW_EVENT_TABLE, self.raw_event)?,
        ])
    }

    fn observe_sched_stat(
        &mut self,
        event_name: &'static str,
        ts: u64,
        cpu: u32,
        pid: i32,
        comm: &str,
        delay: u64,
    ) {
        self.threads.get_or_create_thread(ts, pid, Some(comm));
        self.push_raw_event(
            ts,
            cpu,
            Some(pid),
            event_name,
            json!({ "comm": comm, "pid": pid, "delay": delay }),
        );
    }

    fn push_wakeup(&mut self, event: WakeupEvent<'_>) {
        let target =
            self.threads
                .get_or_create_thread(event.ts, event.target_tid, Some(event.target_comm));
        let source =
            self.threads
                .get_or_create_thread(event.ts, event.source_tid, Some(event.source_comm));
        self.instant.push(InstantRow::from_wakeup(
            event.ts,
            event.event_name,
            target.itid,
            source.itid,
        ));
        self.push_raw_event(
            event.ts,
            event.cpu,
            Some(event.target_tid),
            event.event_name,
            event.payload,
        );
    }
}

struct WakeupEvent<'a> {
    ts: u64,
    cpu: u32,
    event_name: &'static str,
    target_tid: i32,
    target_comm: &'a str,
    source_tid: i32,
    source_comm: &'a str,
    payload: Value,
}

impl DerivedTables {
    fn push_raw_event(
        &mut self,
        ts: u64,
        cpu: u32,
        tid: Option<i32>,
        event_name: &'static str,
        payload: Value,
    ) {
        self.raw_event.push(RawEventRow {
            ts,
            cpu,
            tid,
            event_name: event_name.to_string(),
            payload_json: Some(payload.to_string()),
        });
    }
}

#[derive(Clone, Copy)]
struct ThreadRef {
    itid: u32,
    tid: i32,
    ipid: u32,
    pid: i32,
}

#[derive(Default)]
struct ThreadProcessIndex {
    process_by_pid: HashMap<i32, u32>,
    thread_by_tid: HashMap<i32, u32>,
    processes: Vec<ProcessRow>,
    threads: Vec<ThreadRow>,
}

impl ThreadProcessIndex {
    fn get_or_create_thread(&mut self, ts: u64, tid: i32, name: Option<&str>) -> ThreadRef {
        let ipid = self.get_or_create_process(ts, tid, name);
        let itid = if let Some(&itid) = self.thread_by_tid.get(&tid) {
            let thread = &mut self.threads[itid as usize];
            if let Some(name) = normalized_name(name) {
                thread.name = Some(name.to_string());
            }
            thread.end_ts = Some(ts);
            if thread.ipid.is_none() {
                thread.ipid = Some(ipid);
            }
            itid
        } else {
            let itid = self.threads.len() as u32;
            self.thread_by_tid.insert(tid, itid);
            self.processes[ipid as usize].thread_count += 1;
            self.threads.push(ThreadRow {
                id: itid,
                itid,
                tid,
                name: normalized_name(name).map(ToOwned::to_owned),
                start_ts: Some(ts),
                end_ts: Some(ts),
                ipid: Some(ipid),
                is_main_thread: Some(true),
                switch_count: 0,
            });
            itid
        };

        ThreadRef {
            itid,
            tid,
            ipid,
            pid: self.processes[ipid as usize].pid,
        }
    }

    fn mark_thread_ended(&mut self, ts: u64, tid: i32) {
        if let Some(&itid) = self.thread_by_tid.get(&tid) {
            self.threads[itid as usize].end_ts = Some(ts);
        }
    }

    fn increment_thread_switch(&mut self, itid: u32) {
        if let Some(thread) = self.threads.get_mut(itid as usize) {
            thread.switch_count += 1;
        }
    }

    fn into_rows(self) -> (Vec<ProcessRow>, Vec<ThreadRow>) {
        (self.processes, self.threads)
    }

    fn get_or_create_process(&mut self, ts: u64, pid: i32, name: Option<&str>) -> u32 {
        if let Some(&ipid) = self.process_by_pid.get(&pid) {
            let process = &mut self.processes[ipid as usize];
            if let Some(name) = normalized_name(name) {
                process.name = Some(name.to_string());
            }
            return ipid;
        }

        let ipid = self.processes.len() as u32;
        self.process_by_pid.insert(pid, ipid);
        self.processes.push(ProcessRow {
            id: ipid,
            ipid,
            pid,
            name: normalized_name(name).map(ToOwned::to_owned),
            start_ts: Some(ts),
            switch_count: 0,
            thread_count: 0,
            slice_count: 0,
            mem_count: 0,
        });
        ipid
    }
}

fn normalized_name(name: Option<&str>) -> Option<&str> {
    name.filter(|name| !name.is_empty())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ProcessRow {
    id: u32,
    ipid: u32,
    pid: i32,
    name: Option<String>,
    start_ts: Option<u64>,
    switch_count: u64,
    thread_count: u64,
    slice_count: u64,
    mem_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ThreadRow {
    id: u32,
    itid: u32,
    tid: i32,
    name: Option<String>,
    start_ts: Option<u64>,
    end_ts: Option<u64>,
    ipid: Option<u32>,
    is_main_thread: Option<bool>,
    switch_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ThreadStateRow {
    ts: u64,
    dur: Option<u64>,
    cpu: Option<u32>,
    itid: u32,
    tid: i32,
    pid: Option<i32>,
    state: String,
    comm: String,
}

#[derive(Default)]
pub(super) struct ThreadStateBuilder {
    rows: Vec<ThreadStateRow>,
    active_by_itid: HashMap<u32, usize>,
}

impl ThreadStateBuilder {
    fn push_state(
        &mut self,
        ts: u64,
        cpu: Option<u32>,
        thread: ThreadRef,
        state: String,
        comm: String,
    ) {
        if let Some(active_row) = self.active_by_itid.insert(thread.itid, self.rows.len()) {
            let start_ts = self.rows[active_row].ts;
            if ts >= start_ts {
                self.rows[active_row].dur = Some(ts - start_ts);
            }
        }

        self.rows.push(ThreadStateRow {
            ts,
            dur: None,
            cpu,
            itid: thread.itid,
            tid: thread.tid,
            pid: Some(thread.pid),
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
    ref_id: u32,
    wakeup_from: u32,
    ref_type: String,
    value: f64,
}

impl InstantRow {
    fn from_wakeup(ts: u64, name: &str, ref_id: u32, wakeup_from: u32) -> Self {
        Self {
            ts,
            name: name.to_string(),
            ref_id,
            wakeup_from,
            ref_type: "itid".to_string(),
            value: 0.0,
        }
    }
}

#[derive(Default)]
pub(super) struct SchedSliceBuilder {
    rows: Vec<SchedSliceRow>,
    active_by_cpu: HashMap<u32, usize>,
}

impl SchedSliceBuilder {
    fn push_switch(
        &mut self,
        ts: u64,
        cpu: u32,
        prev_state: u64,
        thread: ThreadRef,
        priority: i32,
    ) {
        self.close_cpu(ts, cpu, prev_state);

        let id = self.rows.len() as u64;
        self.active_by_cpu.insert(cpu, self.rows.len());
        self.rows.push(SchedSliceRow {
            id,
            ts,
            dur: None,
            ts_end: None,
            cpu,
            itid: thread.itid,
            ipid: Some(thread.ipid),
            end_state: None,
            priority,
            arg_setid: None,
        });
    }

    fn close_cpu(&mut self, ts: u64, cpu: u32, prev_state: u64) {
        if let Some(row_id) = self.active_by_cpu.remove(&cpu) {
            let row = &mut self.rows[row_id];
            if ts >= row.ts {
                row.dur = Some(ts - row.ts);
                row.ts_end = Some(ts);
            }
            row.end_state = Some(format!("prev_state:{prev_state}"));
        }
    }

    pub(super) fn into_rows(self) -> Vec<SchedSliceRow> {
        self.rows
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SchedSliceRow {
    id: u64,
    ts: u64,
    dur: Option<u64>,
    ts_end: Option<u64>,
    cpu: u32,
    itid: u32,
    ipid: Option<u32>,
    end_state: Option<String>,
    priority: i32,
    arg_setid: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct RawEventRow {
    ts: u64,
    cpu: u32,
    tid: Option<i32>,
    event_name: String,
    payload_json: Option<String>,
}
