fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let proto_file = "proto/hitrace.proto";
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.type_attribute(
        ".kat.hitrace.ProfilerPluginData",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.type_attribute(
        ".kat.hitrace.SchedSwitchFormat",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.field_attribute(
        ".kat.hitrace.ProfilerPluginData.data",
        "#[serde(with = \"serde_bytes\")]",
    );
    config
        .compile_protos(&[proto_file], &["proto"])
        .expect("hitrace proto compiles");

    println!("cargo:rerun-if-changed={proto_file}");
}
