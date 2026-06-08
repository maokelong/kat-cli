//! 编译 htrace.proto，并输出 Rust 类型和 descriptor set。

use std::path::PathBuf;

/// 使用 vendored protoc 生成 htrace protobuf 产物。
fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    std::env::set_var("PROTOC", protoc);

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set"));
    let descriptor_path = out_dir.join("htrace_descriptor.bin");
    let temp_descriptor_path = std::env::temp_dir().join(format!(
        "kat-rs-datasource-htrace-{}-descriptor.bin",
        std::process::id()
    ));
    let proto_file = "proto/htrace.proto";

    let mut config = prost_build::Config::new();
    config.file_descriptor_set_path(&temp_descriptor_path);
    config
        .compile_protos(&[proto_file], &["proto"])
        .expect("htrace proto compiles");
    std::fs::copy(&temp_descriptor_path, &descriptor_path)
        .expect("descriptor bytes are copied into OUT_DIR");
    let _ = std::fs::remove_file(temp_descriptor_path);

    println!("cargo:rerun-if-changed={proto_file}");
}
