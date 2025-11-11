use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{info, instrument};

use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

use crate::api::tim_api_server::TimApi;
use crate::api::{
    SendMessageReq, SendMessageRes, Session, SpaceUpdate, SubscribeToSpaceReq, TrustedConnectReq,
    TrustedConnectRes,
};
use crate::tim_session::TimSessionService;
use crate::tim_space::TimSpace;

#[derive(Clone)]
pub struct TimApiService {
    sessions: Arc<TimSessionService>,
    space: Arc<TimSpace>,
}

impl fmt::Debug for TimApiService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimApi").finish()
    }
}

#[tonic::async_trait]
impl TimApi for TimApiService {
    type SubscribeToSpaceStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<SpaceUpdate, Status>> + Send>>;

    #[instrument(level = "info")]
    async fn trusted_connect(
        &self,
        request: Request<TrustedConnectReq>,
    ) -> Result<Response<TrustedConnectRes>, Status> {
        let session = self
            .sessions
            .create(request.into_inner())
            .map_err(|e| Status::internal(e))?;
        Ok(Response::new(TrustedConnectRes {
            session: Some(session),
        }))
    }

    async fn send_message(
        &self,
        req: Request<SendMessageReq>,
    ) -> Result<Response<SendMessageRes>, Status> {
        let session = self.require_session(&req)?;
        let payload = req.into_inner();
        info!(
            "message received from timite {}: {}",
            session.timite_id, &payload.content
        );
        let result = self
            .space
            .process(payload, session)
            .await
            .map_err(|e| Status::internal(e))?;
        Ok(Response::new(result))
    }

    async fn subscribe_to_space(
        &self,
        req: Request<SubscribeToSpaceReq>,
    ) -> Result<Response<Self::SubscribeToSpaceStream>, Status> {
        let session = self.require_session(&req)?;
        let stream = self.space.subscribe(&req.into_inner(), &session);
        Ok(Response::new(
            Box::pin(ReceiverStream::new(stream).map(Ok::<SpaceUpdate, Status>))
                as Self::SubscribeToSpaceStream,
        ))
    }
}

impl TimApiService {
    pub fn new(sessions: Arc<TimSessionService>, space: Arc<TimSpace>) -> Self {
        Self { sessions, space }
    }

    fn require_session<T>(&self, req: &Request<T>) -> Result<Session, Status> {
        req.extensions()
            .get::<Session>()
            .cloned()
            .ok_or_else(|| Status::unauthenticated("No session"))
    }
}
