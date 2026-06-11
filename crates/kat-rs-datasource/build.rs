fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let proto_files = ["proto/hitrace.proto", "proto/ftrace_data/sched.proto"];
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.type_attribute(
        ".kat.hitrace.ProfilerPluginData",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.type_attribute(
        ".SchedSwitchFormat",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.enum_attribute(
        ".kat.hitrace.FtraceEvent.event",
        "#[allow(clippy::enum_variant_names)]",
    );
    config.field_attribute(
        ".kat.hitrace.ProfilerPluginData.data",
        "#[serde(with = \"serde_bytes\")]",
    );
    config
        .compile_protos(&proto_files, &["proto"])
        .expect("hitrace and sched protos compile");

    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}
