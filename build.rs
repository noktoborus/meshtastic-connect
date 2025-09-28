// build.rs
use std::path::Path;
use std::path::PathBuf;

fn main() {
    let protobuf_dir = "./protobufs";

    let protos = [
        "meshtastic/admin.proto",
        "meshtastic/apponly.proto",
        "meshtastic/atak.proto",
        "meshtastic/cannedmessages.proto",
        "meshtastic/channel.proto",
        "meshtastic/clientonly.proto",
        "meshtastic/config.proto",
        "meshtastic/connection_status.proto",
        //"meshtastic/deviceonly.proto",
        "meshtastic/device_ui.proto",
        "meshtastic/interdevice.proto",
        "meshtastic/localonly.proto",
        "meshtastic/mesh.proto",
        "meshtastic/module_config.proto",
        "meshtastic/mqtt.proto",
        "meshtastic/paxcount.proto",
        "meshtastic/portnums.proto",
        "meshtastic/powermon.proto",
        "meshtastic/remote_hardware.proto",
        "meshtastic/rtttl.proto",
        "meshtastic/storeforward.proto",
        "meshtastic/telemetry.proto",
        "meshtastic/xmodem.proto",
    ];

    if Path::new(protobuf_dir).exists() {
        let mut protos_paths = vec![];
        for proto in &protos {
            protos_paths.push(PathBuf::from(protobuf_dir).join(proto));
        }

        let mut config = prost_build::Config::new();
        config.protoc_arg("--experimental_allow_proto3_optional");
        config.out_dir("src");

        config
            .compile_protos(&protos_paths, &[protobuf_dir])
            .unwrap();

        for proto in &protos_paths {
            println!("cargo:rerun-if-changed={}", proto.display());
        }
    }
}
