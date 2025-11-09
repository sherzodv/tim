use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../tim-api";
    let protos = &["../tim-api/tim/api/g1/api.proto"];

    let status = Command::new("buf")
        .args(["lint", ".."])
        .status()
        .expect("failed to run buf build");
    assert!(status.success(), "buf build failed");

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(protos, &[&proto_root])?;

    println!("cargo:rerun-if-changed={}", proto_root);
    Ok(())
}
