pub(crate) mod api {
    tonic::include_proto!("tim.api.g1");
}

mod tim_api;
mod tim_session;
mod tim_space;
mod tim_storage;

use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::api::tim_api_server::TimApiServer;
use crate::tim_api::TimApiService;
use crate::tim_session::{SessionLayer, TimSessionService};
use crate::tim_space::TimSpace;

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

    let session_svc = Arc::new(TimSessionService::new());
    let space_svc = Arc::new(TimSpace::new());
    let service = TimApiService::new(session_svc.clone(), space_svc.clone());
    let server = TimApiServer::new(service);
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
