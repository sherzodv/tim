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
use crate::api::GetTimelineReq;
use crate::api::GetTimelineRes;
use crate::api::ListAbilitiesReq;
use crate::api::ListAbilitiesRes;
use crate::api::SendCallAbilityOutcomeReq;
use crate::api::SendCallAbilityOutcomeRes;
use crate::api::SendCallAbilityReq;
use crate::api::SendCallAbilityRes;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::SendTaskFinishedReq;
use crate::api::SendTaskFinishedRes;
use crate::api::SendTaskModelEventReq;
use crate::api::SendTaskModelEventRes;
use crate::api::SendTaskProcessEventReq;
use crate::api::SendTaskProcessEventRes;
use crate::api::SendTaskStartedReq;
use crate::api::SendTaskStartedRes;
use crate::api::SendTaskStepReq;
use crate::api::SendTaskStepRes;
use crate::api::SendTaskToolCallReq;
use crate::api::SendTaskToolCallRes;
use crate::api::SendTypingStartedReq;
use crate::api::SendTypingStartedRes;
use crate::api::SendTypingStoppedReq;
use crate::api::SendTypingStoppedRes;
use crate::api::Session;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::TrustedConnectReq;
use crate::api::TrustedConnectRes;
use crate::tim_api::TimApi;

#[derive(Clone)]
pub struct TimGrpcApiService {
    api: Arc<TimApi>,
}

#[tonic::async_trait]
impl TimGrpcApi for TimGrpcApiService {
    type SubscribeToSpaceStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<SpaceEvent, Status>> + Send>>;

    async fn trusted_connect(
        &self,
        req: Request<TrustedConnectReq>,
    ) -> Result<Response<TrustedConnectRes>, Status> {
        let res = self
            .api
            .trusted_connect(&req.into_inner())
            .await
            .map(|r| Response::new(r));
        res.map_err(|e| Status::internal(e.to_string()))
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
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn list_abilities(
        &self,
        req: Request<ListAbilitiesReq>,
    ) -> Result<Response<ListAbilitiesRes>, Status> {
        self.require_session(&req)?;
        let res = self.api.list_abilities().await.map(|r| Response::new(r));
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn get_timeline(
        &self,
        req: Request<GetTimelineReq>,
    ) -> Result<Response<GetTimelineRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .get_timeline(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
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
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn subscribe_to_space(
        &self,
        req: Request<SubscribeToSpaceReq>,
    ) -> Result<Response<Self::SubscribeToSpaceStream>, Status> {
        let session = self.require_session(&req)?;
        let stream = self
            .api
            .subscribe(&req.into_inner(), &session)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(
            Box::pin(ReceiverStream::new(stream).map(Ok::<SpaceEvent, Status>))
                as Self::SubscribeToSpaceStream,
        ))
    }

    async fn send_call_ability(
        &self,
        req: Request<SendCallAbilityReq>,
    ) -> Result<Response<SendCallAbilityRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_call_ability(&req.into_inner(), &session)
            .await
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_call_ability_outcome(
        &self,
        req: Request<SendCallAbilityOutcomeReq>,
    ) -> Result<Response<SendCallAbilityOutcomeRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_call_ability_outcome(&req.into_inner(), &session)
            .await
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_typing_started(
        &self,
        req: Request<SendTypingStartedReq>,
    ) -> Result<Response<SendTypingStartedRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_typing_started(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_typing_stopped(
        &self,
        req: Request<SendTypingStoppedReq>,
    ) -> Result<Response<SendTypingStoppedRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_typing_stopped(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_started(
        &self,
        req: Request<SendTaskStartedReq>,
    ) -> Result<Response<SendTaskStartedRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_started(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_step(
        &self,
        req: Request<SendTaskStepReq>,
    ) -> Result<Response<SendTaskStepRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_step(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_tool_call(
        &self,
        req: Request<SendTaskToolCallReq>,
    ) -> Result<Response<SendTaskToolCallRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_tool_call(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_model_event(
        &self,
        req: Request<SendTaskModelEventReq>,
    ) -> Result<Response<SendTaskModelEventRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_model_event(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_process_event(
        &self,
        req: Request<SendTaskProcessEventReq>,
    ) -> Result<Response<SendTaskProcessEventRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_process_event(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
    }

    async fn send_task_finished(
        &self,
        req: Request<SendTaskFinishedReq>,
    ) -> Result<Response<SendTaskFinishedRes>, Status> {
        let session = self.require_session(&req)?;
        let res = self
            .api
            .send_task_finished(&req.into_inner(), &session)
            .map(Response::new);
        res.map_err(|e| Status::internal(e.to_string()))
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
