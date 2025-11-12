use std::net::SocketAddr;
use std::sync::Arc;

use tim_code::api::tim_grpc_api_server::TimGrpcApiServer;
use tim_code::tim_api::TimApi;
use tim_code::tim_capability::TimCapability;
use tim_code::tim_grpc_api::TimGrpcApiService;
use tim_code::tim_session::SessionLayer;
use tim_code::tim_session::TimSession;
use tim_code::tim_space::TimSpace;
use tim_code::tim_storage::TimStorage;
use tim_code::tim_timite::TimTimite;
use tonic::transport::Server;
use tonic_web::GrpcWebLayer;
use tower_http::cors::Any;
use tower_http::cors::CorsLayer;
use tracing::info;

fn init_tracing() {
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(default_filter))
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port: u16 = std::env::var("TIM_CODE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let host = std::env::var("TIM_CODE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("invalid TIM_CODE_HOST or TIM_CODE_PORT");

    let data_dir = std::env::var("TIM_DATA_DIR").unwrap_or_else(|_| "./.tim".to_string());

    let storage_svc = Arc::new(TimStorage::new(&data_dir)?);
    let session_svc = Arc::new(TimSession::new(storage_svc.clone()));
    let space_svc = Arc::new(TimSpace::new());
    let timite_svc = Arc::new(TimTimite::new(storage_svc.clone())?);
    let capability_svc = Arc::new(TimCapability::new(storage_svc.clone())?);

    let api_svc = Arc::new(TimApi::new(
        session_svc.clone(),
        space_svc.clone(),
        timite_svc.clone(),
        capability_svc.clone(),
    ));

    let api_svc = TimGrpcApiService::new(api_svc.clone());
    let server = TimGrpcApiServer::new(api_svc);
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any);

    info!("Starting tim-code gRPC backend on {addr}");

    Server::builder()
        .accept_http1(true)
        .layer(cors)
        .layer(SessionLayer::new(session_svc.clone()))
        .layer(GrpcWebLayer::new())
        .add_service(server)
        .serve(addr)
        .await?;

    Ok(())
}
