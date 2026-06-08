use std::path::PathBuf;

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    // build.rs runs before crate compilation; setting PROTOC here only affects this build process.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    let proto_file = "proto/hitrace.proto";
    prost_build::Config::new()
        .compile_protos(&[proto_file], &["proto"])
        .expect("hitrace proto compiles");

    println!("cargo:rerun-if-changed={proto_file}");

    let _ = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set"));
}
