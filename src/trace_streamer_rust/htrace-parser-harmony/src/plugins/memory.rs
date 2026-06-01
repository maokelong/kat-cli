use htrace_core::{TraceEngineError, TraceResult};
use htrace_model::{MeasureRow, ProcessMeasureFilterRow, SysEventFilterRow, TraceTableBuilder};
use prost::Message;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Message)]
pub struct SysMeminfo {
    #[prost(int32, tag = "1")]
    pub key: i32,
    #[prost(uint64, tag = "2")]
    pub value: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct SysVMeminfo {
    #[prost(int32, tag = "1")]
    pub key: i32,
    #[prost(uint64, tag = "2")]
    pub value: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct SmapsInfo {
    #[prost(string, tag = "1")]
    pub start_addr: String,
    #[prost(string, tag = "2")]
    pub end_addr: String,
    #[prost(string, tag = "3")]
    pub permission: String,
    #[prost(string, tag = "4")]
    pub path: String,
    #[prost(uint64, tag = "5")]
    pub size: u64,
    #[prost(uint64, tag = "6")]
    pub rss: u64,
    #[prost(uint64, tag = "7")]
    pub pss: u64,
    #[prost(double, tag = "8")]
    pub reside: f64,
    #[prost(uint64, tag = "9")]
    pub dirty: u64,
    #[prost(uint64, tag = "10")]
    pub swapper: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct AppSummary {
    #[prost(uint64, tag = "1")]
    pub java_heap: u64,
    #[prost(uint64, tag = "2")]
    pub native_heap: u64,
    #[prost(uint64, tag = "3")]
    pub code: u64,
    #[prost(uint64, tag = "4")]
    pub stack: u64,
    #[prost(uint64, tag = "5")]
    pub graphics: u64,
    #[prost(uint64, tag = "6")]
    pub private_other: u64,
    #[prost(uint64, tag = "7")]
    pub system: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProcessMemoryInfo {
    #[prost(int32, tag = "1")]
    pub pid: i32,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(uint64, tag = "3")]
    pub vm_size_kb: u64,
    #[prost(uint64, tag = "4")]
    pub vm_rss_kb: u64,
    #[prost(uint64, tag = "5")]
    pub rss_anon_kb: u64,
    #[prost(uint64, tag = "6")]
    pub rss_file_kb: u64,
    #[prost(uint64, tag = "7")]
    pub rss_shmem_kb: u64,
    #[prost(uint64, tag = "8")]
    pub vm_swap_kb: u64,
    #[prost(uint64, tag = "9")]
    pub vm_locked_kb: u64,
    #[prost(uint64, tag = "10")]
    pub vm_hwm_kb: u64,
    #[prost(int64, tag = "11")]
    pub oom_score_adj: i64,
    #[prost(message, repeated, tag = "12")]
    pub smapinfo: Vec<SmapsInfo>,
    #[prost(message, optional, tag = "13")]
    pub memsummary: Option<AppSummary>,
    #[prost(uint64, optional, tag = "14")]
    pub purg_sum_kb: Option<u64>,
    #[prost(uint64, optional, tag = "15")]
    pub purg_pin_kb: Option<u64>,
    #[prost(uint64, optional, tag = "16")]
    pub gl_pss_kb: Option<u64>,
    #[prost(uint64, optional, tag = "17")]
    pub graph_pss_kb: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MemoryData {
    #[prost(message, repeated, tag = "1")]
    pub processesinfo: Vec<ProcessMemoryInfo>,
    #[prost(message, repeated, tag = "2")]
    pub meminfo: Vec<SysMeminfo>,
    #[prost(message, repeated, tag = "3")]
    pub vmeminfo: Vec<SysVMeminfo>,
    #[prost(uint64, tag = "4")]
    pub zram: u64,
    #[prost(uint64, tag = "9")]
    pub gpu_limit_size: u64,
    #[prost(uint64, tag = "10")]
    pub gpu_used_size: u64,
}

#[derive(Default)]
pub struct MemoryMeasureState {
    process_filter_by_key: HashMap<(u32, String), u64>,
    sys_filter_by_key: HashMap<(String, String), u64>,
    open_process_rows: HashMap<u64, usize>,
    open_sys_rows: HashMap<u64, usize>,
    next_process_filter_id: u64,
    next_sys_filter_id: u64,
}

pub fn parse_memory_plugin<F>(
    data: &[u8],
    ts: Option<i64>,
    tables: &mut TraceTableBuilder,
    state: &mut MemoryMeasureState,
    mut process_id: F,
) -> TraceResult<()>
where
    F: FnMut(i64, u32, Option<&str>) -> u32,
{
    let Some(ts) = ts else {
        return Ok(());
    };
    let memory = MemoryData::decode(data)
        .map_err(|err| TraceEngineError::Parse(format!("failed to decode MemoryData: {err}")))?;

    for process in memory.processesinfo {
        let pid = u32::try_from(process.pid).unwrap_or(0);
        let ipid = process_id(ts, pid, Some(process.name.as_str()));
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.vm.size",
            process.vm_size_kb as i64,
        );
        append_process_metric(tables, state, ts, ipid, "mem.rss", process.vm_rss_kb as i64);
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.rss.anon",
            process.rss_anon_kb as i64,
        );
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.rss.file",
            process.rss_file_kb as i64,
        );
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.rss.schem",
            process.rss_shmem_kb as i64,
        );
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.swap",
            process.vm_swap_kb as i64,
        );
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mem.locked",
            process.vm_locked_kb as i64,
        );
        append_process_metric(tables, state, ts, ipid, "mem.hwm", process.vm_hwm_kb as i64);
        append_process_metric(
            tables,
            state,
            ts,
            ipid,
            "mm.oom_score_adj",
            process.oom_score_adj,
        );

        if let Some(purg_sum_kb) = process.purg_sum_kb {
            append_process_metric(tables, state, ts, ipid, "mem.purg_sum", purg_sum_kb as i64);
        }
        if let Some(purg_pin_kb) = process.purg_pin_kb {
            append_process_metric(tables, state, ts, ipid, "mem.purg_pin", purg_pin_kb as i64);
        }
        if let Some(gl_pss_kb) = process.gl_pss_kb {
            append_process_metric(tables, state, ts, ipid, "mem.gl_pss", gl_pss_kb as i64);
        }
        if let Some(graph_pss_kb) = process.graph_pss_kb {
            append_process_metric(
                tables,
                state,
                ts,
                ipid,
                "mem.graph_pss",
                graph_pss_kb as i64,
            );
        }
    }

    let has_meminfo = !memory.meminfo.is_empty();
    for mem in memory.meminfo {
        if let Some(name) = sys_mem_name(mem.key) {
            append_sys_metric(
                tables,
                state,
                ts,
                "sys_mem_measure_filter",
                name,
                mem.value as i64,
            );
        }
    }
    for mem in memory.vmeminfo {
        if let Some(name) = sys_vmem_name(mem.key) {
            append_sys_metric(
                tables,
                state,
                ts,
                "sys_virtual_mem_measure_filter",
                name,
                mem.value as i64,
            );
        }
    }
    if has_meminfo {
        append_sys_metric(
            tables,
            state,
            ts,
            "sys_mem_measure_filter",
            "sys.mem.zram",
            memory.zram as i64,
        );
        append_sys_metric(
            tables,
            state,
            ts,
            "sys_mem_measure_filter",
            "sys.mem.gpu.limit",
            memory.gpu_limit_size as i64,
        );
        append_sys_metric(
            tables,
            state,
            ts,
            "sys_mem_measure_filter",
            "sys.mem.gpu.used",
            memory.gpu_used_size as i64,
        );
    }

    Ok(())
}

pub(crate) fn append_process_metric(
    tables: &mut TraceTableBuilder,
    state: &mut MemoryMeasureState,
    ts: i64,
    ipid: u32,
    name: &str,
    value: i64,
) {
    let key = (ipid, name.to_string());
    let filter_id = if let Some(id) = state.process_filter_by_key.get(&key) {
        *id
    } else {
        let id = state.next_process_filter_id;
        state.next_process_filter_id += 1;
        tables.push_process_measure_filter(ProcessMeasureFilterRow {
            id,
            name: name.to_string(),
            ipid,
        });
        state.process_filter_by_key.insert(key, id);
        id
    };
    if let Some(row_id) = state.open_process_rows.insert(filter_id, usize::MAX) {
        if row_id != usize::MAX {
            if let Some(row) = tables.process_measure_mut(row_id) {
                row.dur = Some(ts.saturating_sub(row.ts));
            }
        }
    }
    let row_id = tables.push_process_measure(MeasureRow {
        measure_type: "measure".to_string(),
        ts,
        dur: None,
        value,
        filter_id,
    });
    state.open_process_rows.insert(filter_id, row_id);
}

fn append_sys_metric(
    tables: &mut TraceTableBuilder,
    state: &mut MemoryMeasureState,
    ts: i64,
    filter_type: &str,
    name: &str,
    value: i64,
) {
    let key = (filter_type.to_string(), name.to_string());
    let filter_id = if let Some(id) = state.sys_filter_by_key.get(&key) {
        *id
    } else {
        let id = state.next_sys_filter_id;
        state.next_sys_filter_id += 1;
        tables.push_sys_event_filter(SysEventFilterRow {
            id,
            filter_type: filter_type.to_string(),
            name: name.to_string(),
        });
        state.sys_filter_by_key.insert(key, id);
        id
    };
    if let Some(row_id) = state.open_sys_rows.insert(filter_id, usize::MAX) {
        if row_id != usize::MAX {
            if let Some(row) = tables.sys_mem_measure_mut(row_id) {
                row.dur = Some(ts.saturating_sub(row.ts));
            }
        }
    }
    let row_id = tables.push_sys_mem_measure(MeasureRow {
        measure_type: "measure".to_string(),
        ts,
        dur: None,
        value,
        filter_id,
    });
    state.open_sys_rows.insert(filter_id, row_id);
}

fn sys_mem_name(key: i32) -> Option<&'static str> {
    Some(match key {
        0 => "sys.mem.unspecified",
        1 => "sys.mem.total",
        2 => "sys.mem.free",
        3 => "sys.mem.avaiable",
        4 => "sys.mem.buffers",
        5 => "sys.mem.cached",
        6 => "sys.mem.swap.chard",
        7 => "sys.mem.active",
        8 => "sys.mem.inactive",
        9 => "sys.mem.active.anon",
        10 => "sys.mem.inactive.anon",
        11 => "sys.mem.active_file",
        12 => "sys.mem.inactive_file",
        13 => "sys.mem.unevictable",
        14 => "sys.mem.mlocked",
        15 => "sys.mem.swap.total",
        16 => "sys.mem.swap.free",
        17 => "sys.mem.dirty",
        18 => "sys.mem.writeback",
        19 => "sys.mem.anon.pages",
        20 => "sys.mem.mapped",
        21 => "sys.mem.shmem",
        22 => "sys.mem.slab",
        23 => "sys.mem.slab.reclaimable",
        24 => "sys.mem.slab.unreclaimable",
        25 => "sys.mem.kernel.stack",
        26 => "sys.mem.page.tables",
        27 => "sys.mem.commit.limit",
        28 => "sys.mem.commited.as",
        29 => "sys.mem.vmalloc.total",
        30 => "sys.mem.vmalloc.used",
        31 => "sys.mem.vmalloc.chunk",
        32 => "sys.mem.cma.total",
        33 => "sys.mem.cma.free",
        34 => "sys.mem.kernel.reclaimable",
        35 => "sys.mem.active.purg",
        36 => "sys.mem.inactive.purg",
        37 => "sys.mem.pined.purg",
        _ => return None,
    })
}

fn sys_vmem_name(key: i32) -> Option<&'static str> {
    Some(match key {
        0 => "sys.virtual.mem.unspecified",
        1 => "sys.virtual.mem.nr.free.pages",
        2 => "sys.virtual.mem.nr.alloc.batch",
        3 => "sys.virtual.mem.nr.inactive.anon",
        4 => "sys.virtual.mem.nr.active_anon",
        5 => "sys.virtual.mem.nr.inactive.file",
        6 => "sys.virtual.mem.nr.active_file",
        7 => "sys.virtual.mem.nr.unevictable",
        8 => "sys.virtual.mem.nr.mlock",
        9 => "sys.virtual.mem.anon.pages",
        10 => "sys.virtual.mem.nr.mapped",
        11 => "sys.virtual.mem.nr.file.pages",
        12 => "sys.virtual.mem.nr.dirty",
        13 => "sys.virtual.mem.nr.writeback",
        14 => "sys.virtual.mem.nr.slab.reclaimable",
        15 => "sys.virtual.mem.nr.slab.unreclaimable",
        16 => "sys.virtual.mem.nr.page_table.pages",
        17 => "sys.virtual.mem.nr_kernel.stack",
        18 => "sys.virtual.mem.nr.overhead",
        19 => "sys.virtual.mem.nr.unstable",
        20 => "sys.virtual.mem.nr.bounce",
        21 => "sys.virtual.mem.nr.vmscan.write",
        22 => "sys.virtual.mem.nr.vmscan.immediate.reclaim",
        23 => "sys.virtual.mem.nr.writeback_temp",
        24 => "sys.virtual.mem.nr.isolated_anon",
        25 => "sys.virtual.mem.nr.isolated_file",
        26 => "sys.virtual.mem.nr.shmem",
        27 => "sys.virtual.mem.nr.dirtied",
        28 => "sys.virtual.mem.nr.written",
        29 => "sys.virtual.mem.nr.pages.scanned",
        30 => "sys.virtual.mem.workingset.refault",
        31 => "sys.virtual.mem.workingset.activate",
        32 => "sys.virtual.mem.workingset_nodereclaim",
        33 => "sys.virtual.mem.nr_anon.transparent.hugepages",
        34 => "sys.virtual.mem.nr.free_cma",
        35 => "sys.virtual.mem.nr.swapcache",
        36 => "sys.virtual.mem.nr.dirty.threshold",
        37 => "sys.virtual.mem.nr.dirty.background.threshold",
        38 => "sys.virtual.mem.vmeminfo.pgpgin",
        39 => "sys.virtual.mem.pgpgout",
        40 => "sys.virtual.mem.pgpgoutclean",
        41 => "sys.virtual.mem.pswpin",
        42 => "sys.virtual.mem.pswpout",
        43 => "sys.virtual.mem.pgalloc.dma",
        44 => "sys.virtual.mem.pgalloc.normal",
        45 => "sys.virtual.mem.pgalloc.movable",
        46 => "sys.virtual.mem.pgfree",
        47 => "sys.virtual.mem.pgactivate",
        48 => "sys.virtual.mem.pgdeactivate",
        49 => "sys.virtual.mem.pgfault",
        50 => "sys.virtual.mem.pgmajfault",
        51 => "sys.virtual.mem.pgrefill.dma",
        52 => "sys.virtual.mem.pgrefill.normal",
        53 => "sys.virtual.mem.pgrefill.movable",
        54 => "sys.virtual.mem.pgsteal.kswapd.dma",
        55 => "sys.virtual.mem.pgsteal.kswapd.normal",
        56 => "sys.virtual.mem.pgsteal.kswapd.movable",
        57 => "sys.virtual.mem.pgsteal.direct.dma",
        58 => "sys.virtual.mem.pgsteal.direct.normal",
        59 => "sys.virtual.mem.pgsteal_direct.movable",
        60 => "sys.virtual.mem.pgscan.kswapd.dma",
        61 => "sys.virtual.mem.pgscan_kswapd.normal",
        62 => "sys.virtual.mem.pgscan.kswapd.movable",
        63 => "sys.virtual.mem.pgscan.direct.dma",
        64 => "sys.virtual.mem.pgscan.direct.normal",
        65 => "sys.virtual.mem.pgscan.direct.movable",
        66 => "sys.virtual.mem.pgscan.direct.throttle",
        67 => "sys.virtual.mem.pginodesteal",
        68 => "sys.virtual.mem.slabs_scanned",
        69 => "sys.virtual.mem.kswapd.inodesteal",
        70 => "sys.virtual.mem.kswapd.low.wmark.hit.quickly",
        71 => "sys.virtual.mem.high.wmark.hit.quickly",
        72 => "sys.virtual.mem.pageoutrun",
        73 => "sys.virtual.mem.allocstall",
        74 => "sys.virtual.mem.pgrotated",
        75 => "sys.virtual.mem.drop.pagecache",
        76 => "sys.virtual.mem.drop.slab",
        77 => "sys.virtual.mem.pgmigrate.success",
        78 => "sys.virtual.mem.pgmigrate.fail",
        79 => "sys.virtual.mem.compact.migrate.scanned",
        80 => "sys.virtual.mem.compact.free.scanned",
        81 => "sys.virtual.mem.compact.isolated",
        82 => "sys.virtual.mem.compact.stall",
        83 => "sys.virtual.mem.compact.fail",
        84 => "sys.virtual.mem.compact.success",
        85 => "sys.virtual.mem.compact.daemon.wake",
        86 => "sys.virtual.mem.unevictable.pgs.culled",
        87 => "sys.virtual.mem.unevictable.pgs.scanned",
        88 => "sys.virtual.mem.unevictable.pgs.rescued",
        89 => "sys.virtual.mem.unevictable.pgs.mlocked",
        90 => "sys.virtual.mem.unevictable.pgs.munlocked",
        91 => "sys.virtual.mem.unevictable.pgs.cleared",
        92 => "sys.virtual.mem.unevictable.pgs.stranded",
        93 => "sys.virtual.mem.nr.zspages",
        94 => "sys.virtual.mem.nr.ion.heap",
        95 => "sys.virtual.mem.nr.gpu.heap",
        96 => "sys.virtual.mem.allocstall.dma",
        97 => "sys.virtual.mem.allocstall.movable",
        98 => "sys.virtual.mem.allocstall.normal",
        99 => "sys.virtual.mem.compact_daemon.free.scanned",
        100 => "sys.virtual.mem.compact.daemon.migrate.scanned",
        101 => "sys.virtual.mem.nr.fastrpc",
        102 => "sys.virtual.mem.nr.indirectly.reclaimable",
        103 => "sys.virtual.mem.nr_ion_heap_pool",
        104 => "sys.virtual.mem.nr.kernel_misc.reclaimable",
        105 => "sys.virtual.mem.nr.shadow_call.stack_bytes",
        106 => "sys.virtual.mem.nr.shmem.hugepages",
        107 => "sys.virtual.mem.nr.shmem.pmdmapped",
        108 => "sys.virtual.mem.nr.unreclaimable.pages",
        109 => "sys.virtual.mem.nr.zone.active.anon",
        110 => "sys.virtual.mem.nr.zone.active.file",
        111 => "sys.virtual.mem.nr.zone.inactive_anon",
        112 => "sys.virtual.mem.nr.zone.inactive_file",
        113 => "sys.virtual.mem.nr.zone.unevictable",
        114 => "sys.virtual.mem.nr.zone.write_pending",
        115 => "sys.virtual.mem.oom.kill",
        116 => "sys.virtual.mem.pglazyfree",
        117 => "sys.virtual.mem.pglazyfreed",
        118 => "sys.virtual.mem.pgrefill",
        119 => "sys.virtual.mem.pgscan.direct",
        120 => "sys.virtual.mem.pgscan.kswapd",
        121 => "sys.virtual.mem.pgskip.dma",
        122 => "sys.virtual.mem.pgskip.movable",
        123 => "sys.virtual.mem.pgskip.normal",
        124 => "sys.virtual.mem.pgsteal.direct",
        125 => "sys.virtual.mem.pgsteal.kswapd",
        126 => "sys.virtual.mem.swap.ra",
        127 => "sys.virtual.mem.swap.ra.hit",
        128 => "sys.virtual.mem.workingset.restore",
        _ => return None,
    })
}
