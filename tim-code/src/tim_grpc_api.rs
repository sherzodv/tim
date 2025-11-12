use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::api::tim_grpc_api_server::TimGrpcApi;
use crate::api::DeclareAbilitiesReq;
use crate::api::DeclareAbilitiesRes;
use crate::api::ListAbilitiesReq;
use crate::api::ListAbilitiesRes;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::Session;
use crate::api::SpaceUpdate;
use crate::api::SubscribeToSpaceReq;
use crate::api::TrustedConnectReq;
use crate::api::TrustedConnectRes;
use crate::api::TrustedRegisterReq;
use crate::api::TrustedRegisterRes;
use crate::tim_api::TimApi;

#[derive(Clone)]
pub struct TimGrpcApiService {
    api: Arc<TimApi>,
}

#[tonic::async_trait]
impl TimGrpcApi for TimGrpcApiService {
    type SubscribeToSpaceStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<SpaceUpdate, Status>> + Send>>;

    async fn trusted_register(
        &self,
        req: Request<TrustedRegisterReq>,
    ) -> Result<Response<TrustedRegisterRes>, Status> {
        let res = self
            .api
            .trusted_register(&req.into_inner())
            .await
            .map(|r| Response::new(r));
        res.map_err(|e| Status::ok(e.to_string()))
    }

    async fn trusted_connect(
        &self,
        req: Request<TrustedConnectReq>,
    ) -> Result<Response<TrustedConnectRes>, Status> {
        let res = self
            .api
            .trusted_connect(&req.into_inner())
            .await
            .map(|r| Response::new(r));
        res.map_err(|e| Status::ok(e.to_string()))
    }

    async fn declare_abilities(
        &self,
        req: Request<DeclareAbilitiesReq>,
    ) -> Result<Response<DeclareAbilitiesRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .declare_abilities(&req.into_inner(), &session)
            .await
            .map(|r| Response::new(r));
        res.map_err(|e| Status::ok(e.to_string()))
    }

    async fn list_abilities(
        &self,
        req: Request<ListAbilitiesReq>,
    ) -> Result<Response<ListAbilitiesRes>, Status> {
        self.require_session(&req)?;
        let res = self.api.list_abilities().await.map(|r| Response::new(r));
        res.map_err(|e| Status::ok(e.to_string()))
    }

    async fn send_message(
        &self,
        req: Request<SendMessageReq>,
    ) -> Result<Response<SendMessageRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_message(&req.into_inner(), &session)
            .await
            .map(|r| Response::new(r));
        res.map_err(|e| Status::ok(e.to_string()))
    }

    async fn subscribe_to_space(
        &self,
        req: Request<SubscribeToSpaceReq>,
    ) -> Result<Response<Self::SubscribeToSpaceStream>, Status> {
        let session = self.require_session(&req)?;
        let stream = self.api.subscribe(&req.into_inner(), &session);
        Ok(Response::new(
            Box::pin(ReceiverStream::new(stream).map(Ok::<SpaceUpdate, Status>))
                as Self::SubscribeToSpaceStream,
        ))
    }
}

impl TimGrpcApiService {
    pub fn new(api: Arc<TimApi>) -> Self {
        Self { api }
    }

    fn require_session<T>(&self, req: &Request<T>) -> Result<Session, Status> {
        req.extensions()
            .get::<Session>()
            .cloned()
            .ok_or_else(|| Status::unauthenticated("No session"))
    }
}
