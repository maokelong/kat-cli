# parsers

`src/parsers` 鏀惧叿浣撹緭鍏ユ牸寮忚В鏋愬櫒锛岃礋璐ｆ妸涓嶅悓 trace 杈撳叆杞崲涓虹粺涓€鐨?`ParsedTrace`銆?
## 涓昏妯″潡

- `htrace.rs`: profiler/htrace 浜岃繘鍒跺叆鍙ｃ€傝鍙?header 鎴?length-prefixed segment锛岃В鐮?`ProfilerPluginData`锛屽垎鍙?ftrace/cpu/diskio/memory/process/arkts 鎻掍欢锛屽苟缁存姢 ftrace 璋冨害銆乥inder銆亀orkqueue銆乮rq銆乧lock銆乨ma fence銆乷om score 绛変簨浠剁姸鎬併€?- `registry.rs`: 椤跺眰鏍煎紡璇嗗埆鍏ュ彛銆傚厛瑙ｅ紑甯歌 zip/zlib 鍖呰锛屽啀鎸?bytrace銆乺awtrace銆乸erf銆乭isysevent銆乭ilog銆乭trace 椤哄簭閫夋嫨鍏蜂綋 parser銆?- `bytrace.rs`: bytrace 鏂囨湰鍏ュ彛銆傝В鏋愭枃鏈銆乻ched switch/wakeup銆乼race marker銆乥inder transaction 鍜?softirq entry/exit锛岀淮鎶?CPU running slice銆乼hread state銆乮rq 涓?shared callstack銆?- `rawtrace.rs`: rawtrace segment 瑙ｆ瀽锛屾敮鎸佷簩杩涘埗 segment 鍜屾枃鏈?dump 褰㈡€侊紝淇濈暀鍘熷浜嬩欢淇℃伅銆?- `hilog.rs`: hilog 鏂囨湰瑙ｆ瀽锛岀敓鎴?`log` 琛ㄥ苟缁存姢鏃ュ織鏃堕棿鎴炽€佺骇鍒€乼ag銆乸id/tid銆佹秷鎭綋銆?- `hisysevent.rs`: hisysevent JSON lines 瑙ｆ瀽锛岀敓鎴愮郴缁熶簨浠舵槑缁嗗拰 measure 琛ㄣ€?- `perf.rs`: perf 鏁版嵁瑙ｆ瀽锛屽鐞?header銆乫eature section銆乵map/comm/sample record锛屽苟鐢熸垚 perf 鏂囦欢銆佺嚎绋嬨€乻ample銆乧allchain 绛夎〃銆?
## 涓氬姟瑙勫垯

- 姣忎釜 parser 閮借緭鍑虹粺涓€鐨?`ParsedTrace`锛屽苟閫氳繃 `TraceTableBuilder` 鍐欒〃銆?- 鏃犳硶缁撴瀯鍖栬瘑鍒殑琛屾垨鎻掍欢鏁版嵁搴斿敖閲忎繚鐣欏埌 `raw_event`锛屾柟渚垮悗缁墿灞曡В鏋愯兘鍔涖€?- 闇€瑕佽法浜嬩欢璁＄畻 duration 鐨?parser 搴旂淮鎶ょ姸鎬佹満锛屽苟鍦ㄧ粨鏉熸椂鍏抽棴鎴栧洖鍐欐湭瀹屾垚鐨勮銆?- htrace ftrace 浜嬩欢浼氬厛鎸?timestamp 鍜屽師濮嬮『搴忔帓搴忥紝鍐嶈繘鍏ョ姸鎬佹満锛岄伩鍏嶈法 CPU segment 涔卞簭褰卞搷缁撴灉銆?- ArkTS CPU profiler sample 鏃堕棿浣跨敤 htrace 浼犲叆鐨?MONOTONIC 鍒?BOOTTIME 杞崲閫昏緫瀵归綈涓绘椂闂磋酱銆?
## 璁捐杈圭晫

- 鏈洰褰曞彧璐熻矗杈撳叆鏍煎紡瑙ｆ瀽鍜屼簨浠剁姸鎬佹帹杩涖€?- 琛?schema銆乮d 鍒嗛厤鍜?batch 鏋勯€犱緷璧?`trace-model`銆?- plugin 绾т笟鍔¤涔変紭鍏堟斁鍦?`src/plugins`锛岄伩鍏?htrace 涓?parser 鏃犻檺鑶ㄨ儉銆?

