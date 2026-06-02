# plugins

`src/plugins` 鏀?htrace/profiler plugin 鍙婅法杈撳叆鏍煎紡鍏变韩妯″瀷鐨勪笟鍔¤В鏋愩€?
## shared.rs

- 瑙ｆ瀽 trace marker payload: `B|pid|name`銆乣E`銆乣E|pid`銆乣S|pid|name|cookie`銆乣F|pid|name|cookie`銆乣C|pid|name|value`锛屽苟鍏煎 bytrace/htrace 涓父瑙佺殑绌烘牸鍒嗛殧鍙傛暟褰㈡€併€?- 缁存姢鍚屾璋冪敤鏍堝拰寮傛 cookie 鏄犲皠锛岃礋璐?begin/end銆乤sync begin/end銆乧ounter 鐨勭姸鎬佹帹杩涖€?- 鍐欏叆 `callstack`锛屽苟鎶?`name##key=value`銆乧ounter value 绛夊弬鏁板啓鍏?`args`/`data_dict`銆?- 涓?bytrace 鏂囨湰鍜?htrace ftrace `print`/`tracing_mark_write` 鎻愪緵鍏变韩澶勭悊閫昏緫銆?
## memory.rs

- 瑙ｇ爜 memory plugin 鐨?process/system memory 鏁版嵁銆?- 灏嗚繘绋嬬淮搴︽寚鏍囧啓鍏?`process_measure` 鍜?`process_measure_filter`銆?- 灏嗙郴缁熺淮搴︽寚鏍囧啓鍏?`sys_mem_measure` 鍜?`sys_event_filter`銆?- 瀵瑰悓涓€涓?filter 鐨勪笂涓€鏉?metric 鍥炲啓 duration锛屼繚鎸佹寚鏍囧尯闂村彲鏌ヨ銆?
## process.rs

- 瑙ｇ爜 process plugin 鐨勮繘绋嬮噰鏍锋暟鎹€?- 缂撳瓨閲囨牱鐐癸紝骞跺湪瑙ｆ瀽缁撴潫鏃舵寜鏃堕棿鎺掑簭鐢熸垚 `live_process`銆?- 缁存姢 process name銆乸id銆乸pid銆乽id銆乼hread count銆丆PU/鍐呭瓨/IO 绛夐噰鏍峰瓧娈点€?- 浣跨敤鐩搁偦閲囨牱鐐规椂闂村樊璁＄畻 duration锛岄閲囨牱浣滀负鍩虹嚎鍙備笌鍚庣画鍖洪棿鐢熸垚銆?
## arkts.rs

- 瑙ｇ爜 `arkts-plugin_config` 鍜?`arkts-plugin` result銆?- 灏?ArkTS 閰嶇疆鍐欏叆 `js_config`锛屽寘鎷?heap 绫诲瀷銆侀噰鏍烽棿闅斻€乤llocation/cpu profiler 寮€鍏炽€?- 鏀寔 chunked JSON 鎷兼帴锛屽畬鏁存枃妗ｅ埌杈惧悗瑙ｆ瀽 JS heap snapshot銆?- 鍐欏叆 `js_heap_files`銆乣js_heap_info`銆乣js_heap_nodes`銆乣js_heap_edges`銆乣js_heap_string`銆乣js_heap_location`銆乣js_heap_sample`銆乣js_heap_trace_function_info`銆乣js_heap_trace_node`銆?- 瑙ｆ瀽 CPU profiler `profile.nodes/samples/timeDeltas/startTime`锛屽啓鍏?`js_cpu_profiler_node` 鍜?`js_cpu_profiler_sample`銆?
## 璁捐杈圭晫

- plugin 妯″潡鍙鐞嗘彃浠朵笟鍔¤涔夊拰鐘舵€佹満锛屼笉鐩存帴鍋?SQL 鎴?UI 灞曠ず銆?- 鏂?plugin 搴斾紭鍏堝鐢?`TraceTableBuilder`锛岄伩鍏嶇粫杩囩粺涓€ schema銆?- 杈撳叆 framing銆侀《灞?plugin 璺敱鍜岃法 CPU ftrace 鎺掑簭灞炰簬 `src/parsers` 鑱岃矗銆?

