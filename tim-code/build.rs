fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_client(false)
        .build_server(true)
        .compile_protos(&["api/api.proto"], &["api"])?;

    println!("cargo:rerun-if-changed=api/api.proto");
    println!("cargo:rerun-if-changed=api");
    Ok(())
}
