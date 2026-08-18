use crate::proto;
use prost::Message;

pub(super) fn profiler_section(
    envelopes: impl IntoIterator<Item = proto::kat::hitrace::ProfilerPluginData>,
) -> Vec<u8> {
    const PROFILER_HEADER_SIZE: usize = 1024;
    const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

    let mut body = Vec::new();
    for envelope in envelopes {
        let frame = envelope.encode_to_vec();
        body.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        body.extend_from_slice(&frame);
    }

    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(101_u64..=106) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    bytes
}

pub(super) fn batches(path: &std::path::Path) -> Vec<arrow_array::RecordBatch> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(path).expect("Parquet file opens"))
        .expect("Parquet metadata reads")
        .build()
        .expect("Parquet reader builds")
        .collect::<Result<Vec<_>, _>>()
        .expect("Parquet batches read")
}

pub(super) fn full_native_hook_batches() -> (
    proto::kat::native_hook::BatchNativeHookData,
    proto::kat::native_hook::BatchNativeHookData,
) {
    use proto::kat::native_hook::{
        AllocEvent, FilePathMap, FrameMap, FreeEvent, MapsInfo, MemTagEvent, MmapEvent,
        MunmapEvent, NativeHookData, RecordStatisticsEvent, StackMap, SymbolMap, SymbolTable,
        ThreadNameMap, TraceAllocEvent, TraceFreeEvent, TraceType, native_hook_data::Event,
        record_statistics_event::MemoryType,
    };

    let event = |index: u64, event| NativeHookData {
        tv_sec: 100 + index,
        tv_nsec: 200 + index,
        event,
    };
    let first = proto::kat::native_hook::BatchNativeHookData {
        events: vec![
            event(
                0,
                Some(Event::AllocEvent(AllocEvent {
                    pid: 1000,
                    tid: 1001,
                    addr: 0x1000,
                    size: 64,
                    frame_info: vec![native_hook_frame(10), native_hook_frame(11)],
                    thread_name_id: 12,
                    stack_id: 13,
                })),
            ),
            event(
                1,
                Some(Event::FreeEvent(FreeEvent {
                    pid: 1100,
                    tid: 1101,
                    addr: 0x1100,
                    frame_info: vec![native_hook_frame(20), native_hook_frame(21)],
                    thread_name_id: 22,
                    stack_id: 23,
                })),
            ),
            event(
                2,
                Some(Event::MmapEvent(MmapEvent {
                    pid: 1200,
                    tid: 1201,
                    addr: 0x1200,
                    r#type: "file-backed".to_string(),
                    size: 4096,
                    frame_info: vec![native_hook_frame(30), native_hook_frame(31)],
                    thread_name_id: 32,
                    stack_id: 33,
                })),
            ),
            event(
                3,
                Some(Event::MunmapEvent(MunmapEvent {
                    pid: 1300,
                    tid: 1301,
                    addr: 0x1300,
                    size: 8192,
                    frame_info: vec![native_hook_frame(40), native_hook_frame(41)],
                    thread_name_id: 42,
                    stack_id: 43,
                })),
            ),
            event(
                4,
                Some(Event::TagEvent(MemTagEvent {
                    addr: 0x1400,
                    size: 128,
                    tag: "graphics".to_string(),
                    pid: 1400,
                })),
            ),
            event(
                5,
                Some(Event::FilePath(FilePathMap {
                    id: 51,
                    name: "/system/lib64/libfixture.so".to_string(),
                    pid: 1500,
                })),
            ),
            event(
                6,
                Some(Event::SymbolName(SymbolMap {
                    id: 61,
                    name: "fixture_symbol".to_string(),
                    pid: 1600,
                })),
            ),
            event(
                7,
                Some(Event::ThreadNameMap(ThreadNameMap {
                    id: 71,
                    name: "fixture-thread".to_string(),
                    pid: 1700,
                })),
            ),
            event(
                8,
                Some(Event::MapsInfo(MapsInfo {
                    pid: 1800,
                    start: 0x1800,
                    end: 0x18ff,
                    offset: 24,
                    file_path_id: 81,
                })),
            ),
        ],
    };
    let second = proto::kat::native_hook::BatchNativeHookData {
        events: vec![
            event(
                9,
                Some(Event::SymbolTab(SymbolTable {
                    file_path_id: 91,
                    text_exec_vaddr: 0x1900,
                    text_exec_vaddr_file_offset: 32,
                    sym_entry_size: 24,
                    sym_table: vec![0x00, 0xff, 0x80],
                    str_table: vec![0xfe, 0x00, 0x7f],
                    pid: 1900,
                })),
            ),
            event(
                10,
                Some(Event::FrameMap(FrameMap {
                    id: 101,
                    frame: None,
                    pid: 2000,
                })),
            ),
            event(
                11,
                Some(Event::FrameMap(FrameMap {
                    id: 111,
                    frame: Some(native_hook_frame(50)),
                    pid: 2100,
                })),
            ),
            event(
                12,
                Some(Event::StackMap(StackMap {
                    id: 121,
                    frame_map_id: vec![501, 502],
                    ip: vec![0x2200, 0x2201, 0x2202],
                    pid: 2200,
                })),
            ),
            event(
                13,
                Some(Event::StatisticsEvent(RecordStatisticsEvent {
                    pid: 2300,
                    callstack_id: 131,
                    r#type: MemoryType::GpuVk as i32,
                    apply_count: 5,
                    release_count: 3,
                    apply_size: 500,
                    release_size: 300,
                    tag_name: "stats".to_string(),
                })),
            ),
            event(
                14,
                Some(Event::TraceAllocEvent(TraceAllocEvent {
                    pid: 2400,
                    tid: 2401,
                    addr: 0x2400,
                    trace_type: TraceType::Other as i32,
                    tag_name: "trace-alloc".to_string(),
                    size: 1024,
                    frame_info: vec![native_hook_frame(60), native_hook_frame(61)],
                    thread_name_id: 142,
                    stack_id: 143,
                })),
            ),
            event(
                15,
                Some(Event::TraceFreeEvent(TraceFreeEvent {
                    pid: 2500,
                    tid: 2501,
                    addr: 0x2500,
                    trace_type: 99,
                    tag_name: "trace-free".to_string(),
                    frame_info: vec![native_hook_frame(70), native_hook_frame(71)],
                    thread_name_id: 152,
                    stack_id: 153,
                })),
            ),
            event(16, None),
        ],
    };
    (first, second)
}

pub(super) fn native_hook_frame(seed: u64) -> proto::kat::native_hook::Frame {
    proto::kat::native_hook::Frame {
        ip: 10_000 + seed,
        sp: 20_000 + seed,
        symbol_name: format!("symbol-{seed}"),
        file_path: format!("/fixture/{seed}.so"),
        offset: 30_000 + seed,
        symbol_offset: 40_000 + seed,
        symbol_name_id: 50_000 + seed as u32,
        file_path_id: 60_000 + seed as u32,
    }
}

pub(super) fn full_native_hook_config(clock: &str) -> proto::kat::native_hook::NativeHookConfig {
    proto::kat::native_hook::NativeHookConfig {
        pid: 4242,
        save_file: true,
        file_name: "native-hook.htrace".to_string(),
        filter_size: 16,
        smb_pages: 32,
        max_stack_depth: 64,
        process_name: "fixture-process".to_string(),
        malloc_disable: true,
        mmap_disable: true,
        free_stack_report: true,
        munmap_stack_report: true,
        malloc_free_matching_interval: 101,
        malloc_free_matching_cnt: 102,
        string_compressed: true,
        fp_unwind: true,
        blocked: true,
        record_accurately: true,
        startup_mode: true,
        memtrace_enable: true,
        offline_symbolization: true,
        callframe_compress: true,
        statistics_interval: 103,
        clock: clock.to_string(),
        sample_interval: 104,
        response_library_mode: true,
        expand_pids: vec![4242, 4343],
        js_stack_report: 105,
        max_js_stack_depth: 106,
        filter_napi_name: "napi_fixture".to_string(),
        dump_nmd: true,
        target_so_name: "libfixture.so".to_string(),
        restrace_tag: vec!["tag-a".to_string(), "tag-b".to_string()],
    }
}
