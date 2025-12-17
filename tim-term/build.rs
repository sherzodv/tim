fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_proto_root = "../tim-api";
    let protos = &["../tim-api/tim/api/g1/api.proto"];

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(false)
        .compile_protos(protos, &[api_proto_root])?;

    println!("cargo:rerun-if-changed={}", api_proto_root);
    Ok(())
}
