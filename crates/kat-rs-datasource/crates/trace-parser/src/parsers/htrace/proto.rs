use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct ProfilerPluginData {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(uint32, tag = "2")]
    pub status: u32,
    #[prost(bytes = "vec", tag = "3")]
    pub data: Vec<u8>,
    #[prost(int32, tag = "4")]
    pub clock_id: i32,
    #[prost(uint64, tag = "5")]
    pub tv_sec: u64,
    #[prost(uint64, tag = "6")]
    pub tv_nsec: u64,
    #[prost(string, tag = "7")]
    pub version: String,
    #[prost(uint32, tag = "8")]
    pub sample_interval: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct TracePluginResult {
    #[prost(message, repeated, tag = "1")]
    pub ftrace_cpu_stats: Vec<FtraceCpuStatsMsg>,
    #[prost(message, repeated, tag = "2")]
    pub ftrace_cpu_detail: Vec<FtraceCpuDetailMsg>,
    #[prost(message, repeated, tag = "5")]
    pub symbols_detail: Vec<SymbolsDetailMsg>,
    #[prost(message, repeated, tag = "6")]
    pub clocks_detail: Vec<ClockDetailMsg>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClockDetailMsg {
    #[prost(int32, tag = "1")]
    pub id: i32,
    #[prost(message, optional, tag = "2")]
    pub time: Option<ClockDetailTimeSpec>,
    #[prost(message, optional, tag = "3")]
    pub resolution: Option<ClockDetailTimeSpec>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClockDetailTimeSpec {
    #[prost(uint32, tag = "1")]
    pub tv_sec: u32,
    #[prost(uint32, tag = "2")]
    pub tv_nsec: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SymbolsDetailMsg {
    #[prost(uint64, tag = "1")]
    pub symbol_addr: u64,
    #[prost(string, tag = "2")]
    pub symbol_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct FtraceCpuStatsMsg {
    #[prost(string, tag = "3")]
    pub trace_clock: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct FtraceCpuDetailMsg {
    #[prost(uint32, tag = "1")]
    pub cpu: u32,
    #[prost(message, repeated, tag = "2")]
    pub event: Vec<FtraceEvent>,
    #[prost(uint64, tag = "3")]
    pub overwrite: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct FtraceEvent {
    #[prost(uint64, tag = "1")]
    pub timestamp: u64,
    #[prost(int32, tag = "2")]
    pub tgid: i32,
    #[prost(string, tag = "3")]
    pub comm: String,
    #[prost(message, optional, tag = "50")]
    pub common_fields: Option<FtraceEventCommonFields>,
    #[prost(message, optional, tag = "109")]
    pub binder_lock_format: Option<BinderTagFormat>,
    #[prost(message, optional, tag = "110")]
    pub binder_locked_format: Option<BinderTagFormat>,
    #[prost(message, optional, tag = "113")]
    pub binder_transaction_format: Option<BinderTransactionFormat>,
    #[prost(message, optional, tag = "114")]
    pub binder_transaction_alloc_buf_format: Option<BinderTransactionAllocBufFormat>,
    #[prost(message, optional, tag = "119")]
    pub binder_transaction_received_format: Option<BinderTransactionReceivedFormat>,
    #[prost(message, optional, tag = "122")]
    pub binder_unlock_format: Option<BinderTagFormat>,
    #[prost(message, optional, tag = "1109")]
    pub print_format: Option<PrintFormat>,
    #[prost(message, optional, tag = "1800")]
    pub oom_score_adj_update_format: Option<OomScoreAdjUpdateFormat>,
    #[prost(message, optional, tag = "2417")]
    pub sched_switch_format: Option<SchedSwitchFormat>,
    #[prost(message, optional, tag = "2420")]
    pub sched_wakeup_format: Option<SchedWakeupFormat>,
    #[prost(message, optional, tag = "2421")]
    pub sched_wakeup_new_format: Option<SchedWakeupNewFormat>,
    #[prost(message, optional, tag = "2422")]
    pub sched_waking_format: Option<SchedWakingFormat>,
    #[prost(message, optional, tag = "410")]
    pub clk_set_rate_format: Option<ClkSetRateFormat>,
    #[prost(message, optional, tag = "411")]
    pub clk_set_rate_complete_format: Option<ClkSetRateCompleteFormat>,
    #[prost(message, optional, tag = "400")]
    pub clk_disable_format: Option<ClkNameFormat>,
    #[prost(message, optional, tag = "402")]
    pub clk_enable_format: Option<ClkNameFormat>,
    #[prost(message, optional, tag = "1500")]
    pub irq_handler_entry_format: Option<IrqHandlerEntryFormat>,
    #[prost(message, optional, tag = "1501")]
    pub irq_handler_exit_format: Option<IrqHandlerExitFormat>,
    #[prost(message, optional, tag = "1502")]
    pub softirq_entry_format: Option<SoftirqEntryFormat>,
    #[prost(message, optional, tag = "1503")]
    pub softirq_exit_format: Option<SoftirqExitFormat>,
    #[prost(message, optional, tag = "1504")]
    pub softirq_raise_format: Option<SoftirqRaiseFormat>,
    #[prost(message, optional, tag = "2002")]
    pub clock_set_rate_format: Option<ClockSetRateFormat>,
    #[prost(message, optional, tag = "2004")]
    pub cpu_frequency_limits_format: Option<CpuFrequencyLimitsFormat>,
    #[prost(message, optional, tag = "2005")]
    pub cpu_idle_format: Option<CpuIdleFormat>,
    #[prost(message, optional, tag = "700")]
    pub dma_fence_destroy_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "701")]
    pub dma_fence_emit_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "702")]
    pub dma_fence_enable_signal_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "703")]
    pub dma_fence_init_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "704")]
    pub dma_fence_signaled_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "705")]
    pub dma_fence_wait_end_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "706")]
    pub dma_fence_wait_start_format: Option<DmaFenceFormat>,
    #[prost(message, optional, tag = "3101")]
    pub workqueue_execute_end_format: Option<WorkqueueExecuteEndFormat>,
    #[prost(message, optional, tag = "3102")]
    pub workqueue_execute_start_format: Option<WorkqueueExecuteStartFormat>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FtraceEventCommonFields {
    #[prost(uint32, tag = "1")]
    pub event_type: u32,
    #[prost(uint32, tag = "2")]
    pub flags: u32,
    #[prost(uint32, tag = "3")]
    pub preempt_count: u32,
    #[prost(int32, tag = "4")]
    pub pid: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct PrintFormat {
    #[prost(uint64, tag = "1")]
    pub ip: u64,
    #[prost(string, tag = "2")]
    pub buf: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct BinderTagFormat {
    #[prost(string, tag = "1")]
    pub tag: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct BinderTransactionFormat {
    #[prost(int32, tag = "1")]
    pub debug_id: i32,
    #[prost(int32, tag = "2")]
    pub target_node: i32,
    #[prost(int32, tag = "3")]
    pub to_proc: i32,
    #[prost(int32, tag = "4")]
    pub to_thread: i32,
    #[prost(int32, tag = "5")]
    pub reply: i32,
    #[prost(uint32, tag = "6")]
    pub code: u32,
    #[prost(uint32, tag = "7")]
    pub flags: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct BinderTransactionAllocBufFormat {
    #[prost(int32, tag = "1")]
    pub debug_id: i32,
    #[prost(uint64, tag = "2")]
    pub data_size: u64,
    #[prost(uint64, tag = "3")]
    pub offsets_size: u64,
    #[prost(uint64, tag = "4")]
    pub extra_buffers_size: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BinderTransactionReceivedFormat {
    #[prost(int32, tag = "1")]
    pub debug_id: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct OomScoreAdjUpdateFormat {
    #[prost(int32, tag = "1")]
    pub pid: i32,
    #[prost(string, tag = "2")]
    pub comm: String,
    #[prost(int32, tag = "3")]
    pub oom_score_adj: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct WorkqueueExecuteEndFormat {
    #[prost(uint64, tag = "1")]
    pub work: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct WorkqueueExecuteStartFormat {
    #[prost(uint64, tag = "1")]
    pub work: u64,
    #[prost(uint64, tag = "2")]
    pub function: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct SchedSwitchFormat {
    #[prost(string, tag = "1")]
    pub prev_comm: String,
    #[prost(int32, tag = "2")]
    pub prev_pid: i32,
    #[prost(int32, tag = "3")]
    pub prev_prio: i32,
    #[prost(uint64, tag = "4")]
    pub prev_state: u64,
    #[prost(string, tag = "5")]
    pub next_comm: String,
    #[prost(int32, tag = "6")]
    pub next_pid: i32,
    #[prost(int32, tag = "7")]
    pub next_prio: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SchedWakeupFormat {
    #[prost(string, tag = "1")]
    pub comm: String,
    #[prost(int32, tag = "2")]
    pub pid: i32,
    #[prost(int32, tag = "3")]
    pub prio: i32,
    #[prost(int32, tag = "4")]
    pub success: i32,
    #[prost(int32, tag = "5")]
    pub target_cpu: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SchedWakeupNewFormat {
    #[prost(string, tag = "1")]
    pub comm: String,
    #[prost(int32, tag = "2")]
    pub pid: i32,
    #[prost(int32, tag = "3")]
    pub prio: i32,
    #[prost(int32, tag = "4")]
    pub success: i32,
    #[prost(int32, tag = "5")]
    pub target_cpu: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SchedWakingFormat {
    #[prost(string, tag = "1")]
    pub comm: String,
    #[prost(int32, tag = "2")]
    pub pid: i32,
    #[prost(int32, tag = "3")]
    pub prio: i32,
    #[prost(int32, tag = "4")]
    pub success: i32,
    #[prost(int32, tag = "5")]
    pub target_cpu: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClkSetRateFormat {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(uint64, tag = "2")]
    pub rate: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClkSetRateCompleteFormat {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(uint64, tag = "2")]
    pub rate: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClkNameFormat {
    #[prost(string, tag = "1")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct IrqHandlerEntryFormat {
    #[prost(int32, tag = "1")]
    pub irq: i32,
    #[prost(string, tag = "2")]
    pub name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct IrqHandlerExitFormat {
    #[prost(int32, tag = "1")]
    pub irq: i32,
    #[prost(int32, tag = "2")]
    pub ret: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SoftirqEntryFormat {
    #[prost(uint32, tag = "1")]
    pub vec: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SoftirqExitFormat {
    #[prost(uint32, tag = "1")]
    pub vec: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SoftirqRaiseFormat {
    #[prost(uint32, tag = "1")]
    pub vec: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ClockSetRateFormat {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(uint64, tag = "2")]
    pub state: u64,
    #[prost(uint64, tag = "3")]
    pub cpu_id: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct CpuFrequencyLimitsFormat {
    #[prost(uint32, tag = "1")]
    pub min_freq: u32,
    #[prost(uint32, tag = "2")]
    pub max_freq: u32,
    #[prost(uint32, tag = "3")]
    pub cpu_id: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct CpuIdleFormat {
    #[prost(uint32, tag = "1")]
    pub state: u32,
    #[prost(uint32, tag = "2")]
    pub cpu_id: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct DmaFenceFormat {
    #[prost(string, tag = "1")]
    pub driver: String,
    #[prost(string, tag = "2")]
    pub timeline: String,
    #[prost(uint32, tag = "3")]
    pub context: u32,
    #[prost(uint32, tag = "4")]
    pub seqno: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct SampleTimeStamp {
    #[prost(uint64, tag = "1")]
    pub tv_sec: u64,
    #[prost(uint64, tag = "2")]
    pub tv_nsec: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct CpuUsageInfo {
    #[prost(int64, tag = "1")]
    pub prev_process_cpu_time_ms: i64,
    #[prost(int64, tag = "2")]
    pub prev_system_cpu_time_ms: i64,
    #[prost(int64, tag = "3")]
    pub prev_system_boot_time_ms: i64,
    #[prost(int64, tag = "4")]
    pub process_cpu_time_ms: i64,
    #[prost(int64, tag = "5")]
    pub system_cpu_time_ms: i64,
    #[prost(int64, tag = "6")]
    pub system_boot_time_ms: i64,
    #[prost(message, optional, tag = "8")]
    pub timestamp: Option<SampleTimeStamp>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CpuData {
    #[prost(message, optional, tag = "1")]
    pub cpu_usage_info: Option<CpuUsageInfo>,
    #[prost(int64, tag = "3")]
    pub process_num: i64,
    #[prost(double, tag = "4")]
    pub user_load: f64,
    #[prost(double, tag = "5")]
    pub sys_load: f64,
    #[prost(double, tag = "6")]
    pub total_load: f64,
}

#[derive(Clone, PartialEq, Message)]
pub struct CollectTimeStamp {
    #[prost(uint64, tag = "1")]
    pub tv_sec: u64,
    #[prost(uint64, tag = "2")]
    pub tv_nsec: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct DiskioData {
    #[prost(int64, tag = "1")]
    pub prev_rd_sectors_kb: i64,
    #[prost(int64, tag = "2")]
    pub prev_wr_sectors_kb: i64,
    #[prost(message, optional, tag = "3")]
    pub prev_timestamp: Option<CollectTimeStamp>,
    #[prost(int64, tag = "4")]
    pub rd_sectors_kb: i64,
    #[prost(int64, tag = "5")]
    pub wr_sectors_kb: i64,
    #[prost(message, optional, tag = "6")]
    pub timestamp: Option<CollectTimeStamp>,
}
