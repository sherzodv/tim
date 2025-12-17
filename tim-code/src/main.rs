use std::net::SocketAddr;
use std::sync::Arc;

use tim_code::api::tim_grpc_api_server::TimGrpcApiServer;
use tim_code::tim_ability::TimAbility;
use tim_code::tim_api::TimApi;
use tim_code::tim_grpc_api::TimGrpcApiService;
use tim_code::tim_message::TimMessage;
use tim_code::tim_session::SessionLayer;
use tim_code::tim_session::TimSession;
use tim_code::tim_space::TimSpace;
use tim_code::tim_storage::TimStorage;
use tim_code::tim_timite::TimTimite;
use tonic::transport::Server;
use tonic_web::GrpcWebLayer;
use tower_http::cors::Any;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::fmt::format::FmtSpan;

fn init_tracing() {
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(default_filter))
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_ansi(true)
        .with_level(true)
        .with_thread_ids(true)
        .with_target(false)
        .with_line_number(true)
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
    let space_svc = Arc::new(TimSpace::new(storage_svc.clone())?);
    let timite_svc = Arc::new(TimTimite::new(storage_svc.clone())?);
    let ability_svc = Arc::new(TimAbility::new(storage_svc.clone(), space_svc.clone())?);
    let message_svc = Arc::new(TimMessage::new(storage_svc.clone(), space_svc.clone())?);

    let api_svc = Arc::new(TimApi::new(
        session_svc.clone(),
        space_svc.clone(),
        timite_svc.clone(),
        ability_svc.clone(),
        message_svc.clone(),
    ));

    let api_svc = TimGrpcApiService::new(api_svc.clone());
    let server = TimGrpcApiServer::new(api_svc);
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any);

    // Spawn periodic cleanup task for disconnected subscribers
    tokio::spawn({
        let space = space_svc.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                match space.cleanup_disconnected().await {
                    Ok(removed) if removed > 0 => {
                        info!("Cleaned up {removed} disconnected subscriber(s)");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!("Failed to cleanup disconnected subscribers: {error}");
                    }
                }
            }
        }
    });

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
