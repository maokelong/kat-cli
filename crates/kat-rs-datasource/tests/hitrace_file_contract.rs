// hitrace 文件契约测试覆盖私有 header 读取逻辑，避免测试重新回到 src 模块。
#[allow(dead_code)]
mod hitrace_file {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/formats/hitrace/file.rs"
    ));

    #[test]
    fn read_profiler_section_exposes_trace_file_header_fields() {
        let section_len = PROFILER_HEADER_SIZE + 12;
        let mut bytes = vec![0; section_len];
        bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
        bytes[HEADER_LENGTH_OFFSET..HEADER_LENGTH_OFFSET + 8]
            .copy_from_slice(&(section_len as u64).to_le_bytes());
        bytes[HEADER_VERSION_OFFSET..HEADER_VERSION_OFFSET + 4]
            .copy_from_slice(&0x0001_0000u32.to_le_bytes());
        bytes[HEADER_SEGMENTS_OFFSET..HEADER_SEGMENTS_OFFSET + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        let sha = [0xAB; HEADER_SHA256_SIZE];
        bytes[HEADER_SHA256_OFFSET..HEADER_SHA256_OFFSET + HEADER_SHA256_SIZE]
            .copy_from_slice(&sha);
        bytes[HEADER_DATA_TYPE_OFFSET..HEADER_DATA_TYPE_OFFSET + 4]
            .copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());

        let section = read_profiler_section(&bytes, 0).expect("section can be read");

        assert_eq!(section.start, 0);
        assert_eq!(section.body(&bytes).len(), 12);
        assert_eq!(section.end, section_len);
        assert_eq!(section.header.magic, PROFILER_HEADER_MAGIC);
        assert_eq!(section.header.length, section_len);
        assert_eq!(section.header.version, 0x0001_0000);
        assert_eq!(section.header.segments, 2);
        assert_eq!(section.header.sha256, sha);
        assert_eq!(section.header.data_type, HIPROFILER_PROTOBUF_BIN);
    }
}
