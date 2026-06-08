//! htrace protobuf 生成类型和 descriptor bytes。

/// htrace protobuf descriptor set。
pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/htrace_descriptor.bin"));

/// prost-build 生成的 htrace protobuf 类型。
pub mod kat {
    pub mod htrace {
        include!(concat!(env!("OUT_DIR"), "/kat.htrace.rs"));
    }
}
