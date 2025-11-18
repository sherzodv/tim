use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_proto_root = "../tim-api";
    let agent_proto_root = "tim";
    let protos = &[
        "../tim-api/tim/api/g1/api.proto",
        "tim/agent/db/g1/db.proto",
    ];

    let status = Command::new("buf")
        .args(["lint", ".."])
        .status()
        .expect("failed to run buf build");
    assert!(status.success(), "buf build failed");

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(protos, &[&api_proto_root, &agent_proto_root])?;

    println!("cargo:rerun-if-changed={}", api_proto_root);
    println!("cargo:rerun-if-changed=tim/agent/db/g1/db.proto");
    Ok(())
}
