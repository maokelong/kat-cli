use std::{env, fs, path::PathBuf};

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));
    let descriptor_file = "hitrace_descriptor.bin";
    let descriptor_path = env::temp_dir().join(format!(
        "kat-rs-datasource-{}-{descriptor_file}",
        std::process::id()
    ));

    let proto_file = "proto/hitrace.proto";
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.file_descriptor_set_path(&descriptor_path);
    config
        .compile_protos(&[proto_file], &["proto"])
        .expect("hitrace proto compiles");

    fs::copy(&descriptor_path, out_dir.join(descriptor_file))
        .expect("hitrace descriptor is copied into OUT_DIR");

    println!("cargo:rerun-if-changed={proto_file}");
}
