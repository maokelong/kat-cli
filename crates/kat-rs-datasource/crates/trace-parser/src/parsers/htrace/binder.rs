use super::*;

impl HtraceParser {
    pub(super) fn on_print_event(
        &mut self,
        ts: i64,
        tid: u32,
        _tgid: u32,
        _comm: &str,
        print: PrintFormat,
    ) {
        let Some(marker) = shared::parse_trace_marker(&print.buf) else {
            return;
        };
        match marker {
            shared::TraceMarker::Counter {
                callid,
                name,
                value,
            } => {
                let ipid = self.get_or_create_process(ts, callid, None);
                memory::append_process_metric(
                    &mut self.tables,
                    &mut self.memory_state,
                    ts,
                    ipid,
                    &name,
                    value,
                );
            }
            marker => shared::handle_trace_marker(
                &mut self.tables,
                &mut self.shared_trace,
                ts,
                tid,
                marker,
            ),
        }
    }

    pub(super) fn on_workqueue_execute_start(
        &mut self,
        ts: i64,
        tid: u32,
        comm: &str,
        workqueue: WorkqueueExecuteStartFormat,
    ) {
        let utid = self.get_or_create_thread(ts, tid, non_empty_str(comm));
        let name = self
            .symbols_by_addr
            .get(&workqueue.function)
            .cloned()
            .unwrap_or_else(|| format!("0x{:x}", workqueue.function));
        let parent_id = self
            .workqueue_stack_by_tid
            .get(&utid)
            .and_then(|stack| stack.last())
            .and_then(|row_id| self.tables.callstack_id_at(*row_id));
        let depth = self
            .workqueue_stack_by_tid
            .get(&utid)
            .map(|stack| stack.len() as u32)
            .unwrap_or_default();
        let row_id = self.push_callstack_slice(
            ts,
            utid,
            Some("workqueue"),
            &name,
            Some(depth),
            parent_id,
            None,
            None,
        );
        self.workqueue_stack_by_tid
            .entry(utid)
            .or_default()
            .push(row_id);
    }

    pub(super) fn on_workqueue_execute_end(&mut self, ts: i64, tid: u32) {
        let utid = self.get_or_create_thread(ts, tid, None);
        if let Some(stack) = self.workqueue_stack_by_tid.get_mut(&utid) {
            if let Some(row_id) = stack.pop() {
                self.close_callstack_row(row_id, ts);
            }
        }
    }

    pub(super) fn on_oom_score_adj_update(&mut self, ts: i64, oom: OomScoreAdjUpdateFormat) {
        let pid = u32::try_from(oom.pid).unwrap_or_default();
        let ipid = self.get_or_create_process(ts, pid, non_empty_str(&oom.comm));
        memory::append_process_metric(
            &mut self.tables,
            &mut self.memory_state,
            ts,
            ipid,
            "oom_score_adj",
            i64::from(oom.oom_score_adj),
        );
    }

    pub(super) fn on_binder_transaction(
        &mut self,
        ts: i64,
        tid: u32,
        transaction: BinderTransactionFormat,
    ) {
        if transaction.reply == 1 {
            if let Some(row_id) = self.binder_state.reply_by_tid.remove(&tid) {
                if self
                    .binder_state
                    .reply_destination_by_tid
                    .get(&tid)
                    .copied()
                    == Some(u32::try_from(transaction.to_thread).unwrap_or_default())
                {
                    let dest_tid = u32::try_from(transaction.to_thread).unwrap_or_default();
                    let dest_name = self.thread_name_for_tid(dest_tid);
                    self.append_destination_thread_args(row_id, dest_tid, dest_name.as_deref());
                    self.binder_state.reply_destination_by_tid.remove(&tid);
                }
                let argset = self.ensure_callstack_argset(row_id);
                self.append_binder_transaction_args(argset, tid, &transaction);
                self.close_callstack_row(row_id, ts);
            }
            self.binder_state
                .reply_waiting_by_id
                .insert(transaction.debug_id);
            return;
        }

        let argset = self.binder_transaction_argset(tid, &transaction);
        if (transaction.flags & BINDER_ONEWAY_FLAG) == BINDER_ONEWAY_FLAG {
            let row_id =
                self.push_binder_row(ts, tid, "binder transaction async", Some(0), Some(argset));
            self.binder_state
                .async_transaction_args
                .insert(transaction.debug_id, argset);
            self.binder_state.sync_transaction_by_id.insert(
                transaction.debug_id,
                PendingBinderTransaction {
                    row_id,
                    sender_tid: tid,
                },
            );
        } else {
            let row_id = self.push_binder_row(ts, tid, "binder transaction", None, Some(argset));
            self.binder_state.sync_transaction_by_id.insert(
                transaction.debug_id,
                PendingBinderTransaction {
                    row_id,
                    sender_tid: tid,
                },
            );
        }
    }

    pub(super) fn on_binder_transaction_received(
        &mut self,
        ts: i64,
        tid: u32,
        comm: &str,
        received: BinderTransactionReceivedFormat,
    ) {
        let pending = self
            .binder_state
            .sync_transaction_by_id
            .remove(&received.debug_id);
        if let Some(pending) = pending {
            self.close_callstack_row(pending.row_id, ts);
        }

        if let Some(argset) = self
            .binder_state
            .async_transaction_args
            .remove(&received.debug_id)
        {
            self.push_binder_row(ts, tid, "binder async rcv", Some(0), Some(argset));
            return;
        }

        if self
            .binder_state
            .reply_waiting_by_id
            .remove(&received.debug_id)
        {
            return;
        }

        let row_id = self.push_binder_row(ts, tid, "binder reply", None, None);
        let dest_name = self
            .thread_name_for_tid(tid)
            .or_else(|| non_empty_str(comm).map(ToOwned::to_owned));
        if let Some(pending) = pending {
            let reply_slice_id = self.tables.callstack_id_at(row_id).unwrap_or_default() as i64;
            self.append_int_arg_to_callstack(
                pending.row_id,
                "destination slice id",
                reply_slice_id,
            );
            self.append_destination_thread_args(pending.row_id, tid, dest_name.as_deref());
            if let Some(trans_slice_id) = self.tables.callstack_id_at(pending.row_id) {
                self.append_int_arg_to_callstack(
                    row_id,
                    "destination slice id",
                    trans_slice_id as i64,
                );
            }
            self.binder_state
                .reply_destination_by_tid
                .insert(tid, pending.sender_tid);
        }
        self.binder_state.reply_by_tid.insert(tid, row_id);
    }

    pub(super) fn on_binder_transaction_alloc_buf(
        &mut self,
        alloc: BinderTransactionAllocBufFormat,
    ) {
        let Some(pending) = self
            .binder_state
            .sync_transaction_by_id
            .get(&alloc.debug_id)
            .copied()
        else {
            return;
        };
        self.append_int_arg_to_callstack(pending.row_id, "data size", alloc.data_size as i64);
        self.append_int_arg_to_callstack(pending.row_id, "offsets size", alloc.offsets_size as i64);
    }

    pub(super) fn on_binder_lock(&mut self, ts: i64, tid: u32) {
        let row_id = self.push_binder_row(ts, tid, "binder lock waiting", None, None);
        self.binder_state.lock_wait_by_tid.insert(tid, row_id);
    }

    pub(super) fn on_binder_locked(&mut self, ts: i64, tid: u32) {
        if let Some(row_id) = self.binder_state.lock_wait_by_tid.remove(&tid) {
            self.close_callstack_row(row_id, ts);
        }
        let row_id = self.push_binder_row(ts, tid, "binder lock held", None, None);
        self.binder_state.lock_held_by_tid.insert(tid, row_id);
    }

    pub(super) fn on_binder_unlock(&mut self, ts: i64, tid: u32) {
        if let Some(row_id) = self.binder_state.lock_held_by_tid.remove(&tid) {
            self.close_callstack_row(row_id, ts);
        }
    }

    pub(super) fn push_binder_row(
        &mut self,
        ts: i64,
        tid: u32,
        name: &str,
        dur: Option<i64>,
        argsetid: Option<u64>,
    ) -> usize {
        let utid = self.get_or_create_thread(ts, tid, None);
        self.push_callstack_slice(ts, utid, Some("binder"), name, Some(0), None, dur, argsetid)
    }

    pub(super) fn push_callstack_slice(
        &mut self,
        ts: i64,
        callid: u32,
        cat: Option<&str>,
        name: &str,
        depth: Option<u32>,
        parent_id: Option<u64>,
        dur: Option<i64>,
        argsetid: Option<u64>,
    ) -> usize {
        self.tables.push_callstack(CallstackRow {
            id: self.tables.next_callstack_id(),
            ts,
            dur,
            callid: Some(callid),
            cat: cat.map(ToOwned::to_owned),
            name: Some(name.to_string()),
            depth,
            cookie: None,
            parent_id,
            argsetid,
            chain_id: None,
            span_id: None,
            parent_span_id: None,
            flag: None,
            trace_level: None,
            trace_tag: None,
            custom_category: None,
            custom_args: None,
            child_callid: None,
        })
    }

    pub(super) fn close_callstack_row(&mut self, row_id: usize, ts: i64) {
        if let Some(row) = self.tables.callstack_mut(row_id) {
            row.dur = Some(ts.saturating_sub(row.ts));
        }
    }

    pub(super) fn binder_transaction_argset(
        &mut self,
        tid: u32,
        transaction: &BinderTransactionFormat,
    ) -> u64 {
        let argset = self.tables.next_argset_id();
        self.append_binder_transaction_args(argset, tid, transaction);
        argset
    }

    pub(super) fn append_binder_transaction_args(
        &mut self,
        argset: u64,
        tid: u32,
        transaction: &BinderTransactionFormat,
    ) {
        self.push_int_arg(argset, "transaction id", i64::from(transaction.debug_id));
        self.push_int_arg(
            argset,
            "destination node",
            i64::from(transaction.target_node),
        );
        self.push_int_arg(
            argset,
            "destination process",
            i64::from(transaction.to_proc),
        );
        self.push_bool_arg(argset, "reply transaction?", transaction.reply == 1);
        let flags_desc = binder_flags_desc(transaction.flags);
        self.push_string_arg(
            argset,
            "flags",
            &format!("0x{:x}{}", transaction.flags, flags_desc.trim_end()),
        );
        self.push_string_arg(
            argset,
            "code",
            &format!("0x{:x} Java Layer Dependent", transaction.code),
        );
        self.push_int_arg(argset, "calling tid", i64::from(tid));
    }

    pub(super) fn append_int_arg_to_callstack(&mut self, row_id: usize, key: &str, value: i64) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_int_arg(argset, key, value);
    }

    pub(super) fn append_string_arg_to_callstack(&mut self, row_id: usize, key: &str, value: &str) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_string_arg(argset, key, value);
    }

    pub(super) fn append_destination_thread_args(
        &mut self,
        row_id: usize,
        destination_tid: u32,
        destination_name: Option<&str>,
    ) {
        self.append_int_arg_to_callstack(row_id, "destination thread", i64::from(destination_tid));
        self.append_string_arg_to_callstack(
            row_id,
            "destination name",
            destination_name.unwrap_or(""),
        );
    }

    pub(super) fn ensure_callstack_argset(&mut self, row_id: usize) -> u64 {
        let existing_argset = self
            .tables
            .callstack_mut(row_id)
            .and_then(|row| row.argsetid);
        let argset = existing_argset.unwrap_or_else(|| self.tables.next_argset_id());
        if existing_argset.is_none() {
            if let Some(row) = self.tables.callstack_mut(row_id) {
                row.argsetid = Some(argset);
            }
        }
        argset
    }

    pub(super) fn thread_name_for_tid(&self, tid: u32) -> Option<String> {
        self.threads_by_tid
            .get(&tid)
            .and_then(|info| non_empty_str(info.name.as_deref()?))
            .map(ToOwned::to_owned)
    }

    pub(super) fn push_int_arg(&mut self, argset: u64, key: &str, value: i64) {
        let key_id = self.tables.intern_string(key);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_INT, value, argset);
    }

    pub(super) fn push_bool_arg(&mut self, argset: u64, key: &str, value: bool) {
        let key_id = self.tables.intern_string(key);
        self.tables.push_arg(
            key_id,
            ARG_DATATYPE_BOOLEAN,
            if value { 1 } else { 0 },
            argset,
        );
    }

    pub(super) fn push_string_arg(&mut self, argset: u64, key: &str, value: &str) {
        let key_id = self.tables.intern_string(key);
        let value_id = self.tables.intern_string(value);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_STRING, value_id as i64, argset);
    }
}
