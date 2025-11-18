mod common;

use std::time::Duration;

use common::TimApiTestCtx;
use tim_code::api::space_event;
use tim_code::api::tim_grpc_api_server::TimGrpcApi;
use tim_code::api::ClientInfo;
use tim_code::api::SendMessageReq;
use tim_code::api::Session;
use tim_code::api::SubscribeToSpaceReq;
use tim_code::api::TrustedRegisterReq;
use tim_code::tim_grpc_api::TimGrpcApiService;
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tonic::Request;

fn client_info() -> ClientInfo {
    ClientInfo {
        platform: "grpc-test".into(),
    }
}

fn request_with_session<T>(payload: T, session: &Session) -> Request<T> {
    let mut req = Request::new(payload);
    req.extensions_mut().insert(session.clone());
    req
}

#[tokio::test]
async fn grpc_send_message_notifies_subscribers() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = TimApiTestCtx::new()?;
    let service = TimGrpcApiService::new(ctx.api());

    let alpha_session = service
        .trusted_register(Request::new(TrustedRegisterReq {
            nick: "alpha".into(),
            client_info: Some(client_info()),
        }))
        .await?
        .into_inner()
        .session
        .expect("missing alpha session");

    let beta_session = service
        .trusted_register(Request::new(TrustedRegisterReq {
            nick: "beta".into(),
            client_info: Some(client_info()),
        }))
        .await?
        .into_inner()
        .session
        .expect("missing beta session");

    let mut beta_updates = service
        .subscribe_to_space(request_with_session(
            SubscribeToSpaceReq {
                receive_own_messages: false,
            },
            &beta_session,
        ))
        .await?
        .into_inner();

    let send_res = service
        .send_message(request_with_session(
            SendMessageReq {
                content: "grpc ping".into(),
            },
            &alpha_session,
        ))
        .await?
        .into_inner();
    assert!(
        send_res.error.is_none(),
        "send_message should indicate success"
    );

    let update = timeout(Duration::from_secs(1), beta_updates.next())
        .await
        .expect("timed out waiting for grpc update")
        .expect("subscription ended unexpectedly")?;

    let message = match update.data {
        Some(space_event::Data::EventNewMessage(event)) => {
            event.message.expect("space update missing message")
        }
        _ => panic!("unexpected update event {:?}", update.data),
    };

    assert_eq!(message.content, "grpc ping");
    assert_eq!(
        message.sender_id, alpha_session.timite_id,
        "sender id should match the publishing session"
    );

    Ok(())
}
