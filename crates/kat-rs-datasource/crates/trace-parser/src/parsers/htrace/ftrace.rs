use super::*;

impl HtraceParser {
    pub(super) fn process_pending_ftrace_events(&mut self) -> ParseResult<()> {
        let mut events = std::mem::take(&mut self.pending_ftrace_events);
        events.sort_by(|left, right| left.ts.cmp(&right.ts).then(left.order.cmp(&right.order)));

        for event in events {
            self.handle_ftrace_event(event.ts, event.cpu, event.event)?;
        }

        Ok(())
    }

    pub(super) fn handle_ftrace_event(
        &mut self,
        ts: i64,
        cpu: u32,
        event: FtraceEvent,
    ) -> ParseResult<()> {
        self.observe_ts(ts);
        let tid = event.common_fields.as_ref().map(|f| sanitize_tid(f.pid));
        let event_tid = tid.unwrap_or_else(|| sanitize_tid(event.tgid));
        let event_tgid = sanitize_tid(event.tgid);
        let event_comm = event.comm.clone();
        if let Some(print) = event.print_format {
            self.on_print_event(ts, event_tid, event_tgid, &event_comm, print);
        } else if let Some(binder_transaction) = event.binder_transaction_format {
            self.on_binder_transaction(ts, event_tid, binder_transaction);
        } else if let Some(binder_received) = event.binder_transaction_received_format {
            self.on_binder_transaction_received(ts, event_tid, &event_comm, binder_received);
        } else if let Some(alloc_buf) = event.binder_transaction_alloc_buf_format {
            self.on_binder_transaction_alloc_buf(alloc_buf);
        } else if event.binder_lock_format.is_some() {
            self.on_binder_lock(ts, event_tid);
        } else if event.binder_locked_format.is_some() {
            self.on_binder_locked(ts, event_tid);
        } else if event.binder_unlock_format.is_some() {
            self.on_binder_unlock(ts, event_tid);
        } else if let Some(oom) = event.oom_score_adj_update_format {
            self.on_oom_score_adj_update(ts, oom);
        } else if let Some(workqueue_start) = event.workqueue_execute_start_format {
            self.on_workqueue_execute_start(ts, event_tid, &event_comm, workqueue_start);
        } else if event.workqueue_execute_end_format.is_some() {
            self.on_workqueue_execute_end(ts, event_tid);
        } else if let Some(sched_switch) = event.sched_switch_format {
            self.on_sched_switch(ts, cpu, sched_switch)?;
        } else if let Some(sched_wakeup) = event.sched_wakeup_format {
            let target_utid =
                self.on_sched_wakeup(ts, sched_wakeup.pid, Some(sched_wakeup.comm.as_str()));
            self.push_sched_instant(ts, cpu, tid, "sched_wakeup", target_utid);
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_wakeup".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_wakeup.comm,
                        "pid": sched_wakeup.pid,
                        "prio": sched_wakeup.prio,
                        "success": sched_wakeup.success,
                        "target_cpu": sched_wakeup.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(sched_wakeup_new) = event.sched_wakeup_new_format {
            let target_utid = self.on_sched_wakeup(
                ts,
                sched_wakeup_new.pid,
                Some(sched_wakeup_new.comm.as_str()),
            );
            self.push_sched_instant(ts, cpu, tid, "sched_wakeup", target_utid);
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_wakeup_new".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_wakeup_new.comm,
                        "pid": sched_wakeup_new.pid,
                        "prio": sched_wakeup_new.prio,
                        "success": sched_wakeup_new.success,
                        "target_cpu": sched_wakeup_new.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(sched_waking) = event.sched_waking_format {
            let target_utid = self.get_or_create_thread(
                ts,
                sanitize_tid(sched_waking.pid),
                Some(sched_waking.comm.as_str()),
            );
            self.push_sched_instant(ts, cpu, tid, "sched_waking", Some(target_utid));
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_waking".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_waking.comm,
                        "pid": sched_waking.pid,
                        "prio": sched_waking.prio,
                        "success": sched_waking.success,
                        "target_cpu": sched_waking.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(irq_entry) = event.irq_handler_entry_format {
            self.on_irq_entry(ts, cpu, irq_entry);
        } else if let Some(irq_exit) = event.irq_handler_exit_format {
            self.on_irq_exit(ts, cpu, irq_exit.irq);
        } else if let Some(softirq_entry) = event.softirq_entry_format {
            self.on_softirq_entry(ts, cpu, softirq_entry.vec);
        } else if let Some(softirq_exit) = event.softirq_exit_format {
            self.on_softirq_exit(ts, cpu, softirq_exit.vec);
        } else if let Some(softirq_raise) = event.softirq_raise_format {
            self.push_named_raw_event(
                ts,
                cpu,
                tid,
                "softirq_raise",
                json!({
                    "vec": softirq_raise.vec,
                    "name": softirq_name(softirq_raise.vec)
                }),
            );
        } else if let Some(cpu_idle) = event.cpu_idle_format {
            self.on_cpu_idle(ts, cpu, cpu_idle);
        } else if let Some(clock_set_rate) = event.clock_set_rate_format {
            self.on_clock_set_rate(
                ts,
                cpu,
                "clock_set_rate",
                clock_set_rate.name,
                clock_set_rate.state,
            );
        } else if let Some(clk_set_rate) = event.clk_set_rate_format {
            self.on_clock_set_rate(
                ts,
                cpu,
                "clk_set_rate",
                clk_set_rate.name,
                clk_set_rate.rate,
            );
        } else if let Some(clk_set_rate) = event.clk_set_rate_complete_format {
            self.push_named_raw_event(
                ts,
                cpu,
                None,
                "clk_set_rate_complete",
                json!({ "name": clk_set_rate.name, "rate": clk_set_rate.rate }),
            );
        } else if let Some(clk_disable) = event.clk_disable_format {
            self.on_clock_set_rate(ts, cpu, "clk_disable", clk_disable.name, 0);
        } else if let Some(clk_enable) = event.clk_enable_format {
            self.on_clock_set_rate(ts, cpu, "clk_enable", clk_enable.name, 1);
        } else if let Some(cpu_limits) = event.cpu_frequency_limits_format {
            self.on_cpu_frequency_limits(ts, cpu, cpu_limits);
        } else if let Some(dma_fence) = event.dma_fence_destroy_format {
            self.on_dma_fence(ts, cpu, "dma_fence_destroy", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_emit_format {
            self.on_dma_fence(ts, cpu, "dma_fence_emit", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_enable_signal_format {
            self.on_dma_fence(ts, cpu, "dma_fence_enable_signal", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_init_format {
            self.on_dma_fence(ts, cpu, "dma_fence_init", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_signaled_format {
            self.on_dma_fence(ts, cpu, "dma_fence_signaled", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_wait_end_format {
            self.on_dma_fence(ts, cpu, "dma_fence_wait_end", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_wait_start_format {
            self.on_dma_fence(ts, cpu, "dma_fence_wait_start", dma_fence);
        } else {
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "unsupported_ftrace_event".to_string(),
                payload_json: Some(
                    json!({
                        "comm": event.comm,
                        "tgid": event.tgid
                    })
                    .to_string(),
                ),
            });
        }

        Ok(())
    }

    pub(super) fn push_named_raw_event(
        &mut self,
        ts: i64,
        cpu: u32,
        tid: Option<u32>,
        event_name: &str,
        payload: serde_json::Value,
    ) {
        self.tables.push_raw_event(RawEventRow {
            ts: Some(ts),
            cpu: Some(cpu),
            tid,
            event_name: event_name.to_string(),
            payload_json: Some(payload.to_string()),
        });
    }

    pub(super) fn push_sched_instant(
        &mut self,
        ts: i64,
        cpu: u32,
        waker_tid: Option<u32>,
        event_name: &str,
        target_utid: Option<u32>,
    ) {
        let wakeup_from = waker_tid.map(|tid| self.get_or_create_thread(ts, tid, None));
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts,
            name: event_name.to_string(),
            cpu,
            itid: target_utid,
        });
        self.tables.push_instant(InstantRow {
            ts,
            name: event_name.to_string(),
            ref_id: target_utid,
            wakeup_from,
            ref_type: Some("itid".to_string()),
            value: Some(0.0),
        });
    }

    pub(super) fn on_sched_switch(
        &mut self,
        ts: i64,
        cpu: u32,
        msg: SchedSwitchFormat,
    ) -> ParseResult<()> {
        let prev_tid = sanitize_tid(msg.prev_pid);
        let next_tid = sanitize_tid(msg.next_pid);
        let prev_utid = self.get_or_create_thread(ts, prev_tid, Some(msg.prev_comm.as_str()));
        let next_utid = self.get_or_create_thread(ts, next_tid, Some(msg.next_comm.as_str()));

        if let Some(open) = self.cpu_running.remove(&cpu) {
            if let Some(row) = self.tables.sched_slice_mut(open.row_id) {
                row.dur = Some(ts.saturating_sub(open.ts));
                row.end_state = Some(state_from_kernel(msg.prev_state));
            }
        }

        let row_id = self.tables.push_sched_slice(SchedSliceRow {
            cpu,
            utid: next_utid,
            ts,
            dur: None,
            priority: Some(msg.next_prio),
            end_state: Some("runnable".to_string()),
        });
        self.cpu_running.insert(cpu, OpenSchedSlice { row_id, ts });

        if prev_tid != 0 {
            self.check_wakeup_event(prev_tid, prev_utid);
            self.transition_thread_state(prev_utid, ts, state_from_kernel(msg.prev_state), None);
        }
        if next_tid != 0 {
            self.check_wakeup_event(next_tid, next_utid);
            self.transition_thread_state(next_utid, ts, "running".to_string(), None);
        }
        Ok(())
    }

    pub(super) fn on_sched_wakeup(&mut self, ts: i64, pid: i32, name: Option<&str>) -> Option<u32> {
        let tid = sanitize_tid(pid);
        if tid == 0 {
            return None;
        }
        let utid = self.get_or_create_thread(ts, tid, name);
        self.pending_wakeup_by_tid.entry(tid).or_insert(ts);
        Some(utid)
    }

    pub(super) fn on_irq_entry(&mut self, ts: i64, cpu: u32, event: IrqHandlerEntryFormat) {
        let callid = event.irq;
        let id = self.tables.next_irq_id();
        let row_id = self.tables.push_irq(IrqRow {
            id,
            ts,
            dur: None,
            callid: Some(callid),
            cat: "irq".to_string(),
            name: event.name.clone(),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid: Some(id),
            flag: Some("1".to_string()),
        });
        self.open_irqs
            .entry(IrqKey {
                cpu,
                cat: "irq",
                callid,
            })
            .or_default()
            .push(row_id);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "irq_handler_entry",
            json!({ "irq": event.irq, "name": event.name }),
        );
    }

    pub(super) fn on_irq_exit(&mut self, ts: i64, cpu: u32, irq: i32) {
        self.close_irq(
            ts,
            IrqKey {
                cpu,
                cat: "irq",
                callid: irq,
            },
        );
        self.push_named_raw_event(ts, cpu, None, "irq_handler_exit", json!({ "irq": irq }));
    }

    pub(super) fn on_softirq_entry(&mut self, ts: i64, cpu: u32, vec: u32) {
        let callid = vec as i32;
        let id = self.tables.next_irq_id();
        let row_id = self.tables.push_irq(IrqRow {
            id,
            ts,
            dur: None,
            callid: Some(callid),
            cat: "softirq".to_string(),
            name: softirq_name(vec).to_string(),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid: Some(id),
            flag: Some("1".to_string()),
        });
        self.open_irqs
            .entry(IrqKey {
                cpu,
                cat: "softirq",
                callid,
            })
            .or_default()
            .push(row_id);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "softirq_entry",
            json!({ "vec": vec, "name": softirq_name(vec) }),
        );
    }

    pub(super) fn on_softirq_exit(&mut self, ts: i64, cpu: u32, vec: u32) {
        self.close_irq(
            ts,
            IrqKey {
                cpu,
                cat: "softirq",
                callid: vec as i32,
            },
        );
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "softirq_exit",
            json!({ "vec": vec, "name": softirq_name(vec) }),
        );
    }

    pub(super) fn close_irq(&mut self, ts: i64, key: IrqKey) {
        let Some(stack) = self.open_irqs.get_mut(&key) else {
            return;
        };
        let Some(row_id) = stack.pop() else {
            return;
        };
        if let Some(row) = self.tables.irq_mut(row_id) {
            row.dur = Some(ts.saturating_sub(row.ts));
        }
    }

    pub(super) fn on_cpu_idle(&mut self, ts: i64, cpu: u32, event: CpuIdleFormat) {
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts,
            name: "cpu_idle".to_string(),
            cpu,
            itid: Some(0),
        });
        let filter_id = self.measure_filter("cpu_idle", "cpu_measure_filter", Some(event.cpu_id));
        self.push_measure(ts, filter_id, event.state as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "cpu_idle",
            json!({ "state": event.state, "cpu_id": event.cpu_id }),
        );
    }

    pub(super) fn on_clock_set_rate(
        &mut self,
        ts: i64,
        cpu: u32,
        event_name: &str,
        name: String,
        value: u64,
    ) {
        let filter_id = self.measure_filter(&name, "measure_filter", None);
        self.push_measure(ts, filter_id, value as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            event_name,
            json!({ "name": name, "value": value }),
        );
    }

    pub(super) fn on_cpu_frequency_limits(
        &mut self,
        ts: i64,
        cpu: u32,
        event: CpuFrequencyLimitsFormat,
    ) {
        let max_filter = self.measure_filter(
            "cpu_frequency_limits_max",
            "cpu_measure_filter",
            Some(event.cpu_id),
        );
        self.push_measure(ts, max_filter, event.max_freq as i64);
        let min_filter = self.measure_filter(
            "cpu_frequency_limits_min",
            "cpu_measure_filter",
            Some(event.cpu_id),
        );
        self.push_measure(ts, min_filter, event.min_freq as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "cpu_frequency_limits",
            json!({ "min_freq": event.min_freq, "max_freq": event.max_freq, "cpu_id": event.cpu_id }),
        );
    }

    pub(super) fn on_dma_fence(
        &mut self,
        ts: i64,
        cpu: u32,
        event_name: &str,
        event: DmaFenceFormat,
    ) {
        self.tables.push_dma_fence(DmaFenceRow {
            id: self.tables.next_dma_fence_id(),
            ts,
            dur: None,
            cat: event_name.to_string(),
            driver: event.driver.clone(),
            timeline: event.timeline.clone(),
            context: event.context,
            seqno: event.seqno,
        });
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            event_name,
            json!({
                "driver": event.driver,
                "timeline": event.timeline,
                "context": event.context,
                "seqno": event.seqno
            }),
        );
    }

    pub(super) fn measure_filter(
        &mut self,
        name: &str,
        filter_type: &str,
        cpu: Option<u32>,
    ) -> u64 {
        let key = (name.to_string(), filter_type.to_string(), cpu);
        if let Some(id) = self.measure_filters.get(&key) {
            return *id;
        }
        let id = self.next_measure_filter_id;
        self.next_measure_filter_id += 1;
        self.tables.push_measure_filter(MeasureFilterRow {
            id,
            name: name.to_string(),
            source_arg_set_id: None,
            filter_type: filter_type.to_string(),
        });
        if let Some(cpu) = cpu {
            self.tables.push_cpu_measure_filter(CpuMeasureFilterRow {
                id,
                name: name.to_string(),
                cpu,
            });
        }
        self.measure_filters.insert(key, id);
        id
    }

    pub(super) fn push_measure(&mut self, ts: i64, filter_id: u64, value: i64) {
        if let Some(open_row) = self.open_measures.insert(filter_id, usize::MAX) {
            if open_row != usize::MAX {
                if let Some(row) = self.tables.measure_mut(open_row) {
                    row.dur = Some(ts.saturating_sub(row.ts));
                }
            }
        }
        let row_id = self.tables.push_measure(MeasureRow {
            measure_type: "measure".to_string(),
            ts,
            dur: None,
            value,
            filter_id,
        });
        self.open_measures.insert(filter_id, row_id);
    }

    pub(super) fn check_wakeup_event(&mut self, tid: u32, utid: u32) {
        let Some(wakeup_ts) = self.pending_wakeup_by_tid.remove(&tid) else {
            return;
        };

        if let Some(row_id) = self.thread_state_open.get(&utid).copied() {
            let Some(row) = self.tables.thread_state_mut(row_id) else {
                return;
            };
            if row.state == "running" {
                return;
            }
            row.dur = Some(wakeup_ts.saturating_sub(row.ts));
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts: wakeup_ts,
            dur: None,
            state: "runnable".to_string(),
            io_wait: None,
            blocked_function: None,
            waker_utid: None,
        });
        self.thread_state_open.insert(utid, row_id);
    }

    pub(super) fn transition_thread_state(
        &mut self,
        utid: u32,
        ts: i64,
        state: String,
        waker_utid: Option<u32>,
    ) {
        if let Some(row_id) = self.thread_state_open.remove(&utid) {
            if let Some(row) = self.tables.thread_state_mut(row_id) {
                row.dur = Some(ts.saturating_sub(row.ts));
            }
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts,
            dur: None,
            state,
            io_wait: None,
            blocked_function: None,
            waker_utid,
        });
        self.thread_state_open.insert(utid, row_id);
    }

    pub(super) fn get_or_create_thread(&mut self, ts: i64, tid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.threads_by_tid.get_mut(&tid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.utid;
        }

        let upid = self.get_or_create_process(ts, tid, name);
        let id = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (tid == 0).then(|| "idle".to_string()));

        self.threads_by_tid.insert(
            tid,
            ThreadInfo {
                utid: id,
                tid,
                upid,
                name,
                end_ts: Some(ts),
            },
        );
        id
    }

    pub(super) fn get_or_create_process(&mut self, ts: i64, pid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.processes_by_pid.get_mut(&pid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.upid;
        }

        let upid = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (pid == 0).then(|| "idle".to_string()));
        self.processes_by_pid.insert(
            pid,
            ProcessInfoState {
                upid,
                pid,
                name,
                start_ts: Some(ts),
                end_ts: Some(ts),
            },
        );
        upid
    }

    pub(super) fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }
}
