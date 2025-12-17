use std::time::Duration;

mod common;

use common::TimApiTestCtx;
use tim_code::api::space_event;
use tim_code::api::ClientInfo;
use tim_code::api::DeclareAbilitiesReq;
use tim_code::api::SendMessageReq;
use tim_code::api::SubscribeToSpaceReq;
use tim_code::api::Timite;
use tim_code::api::TrustedConnectReq;
use tim_code::api::TrustedRegisterReq;
use tokio::time::timeout;

fn client_info() -> ClientInfo {
    ClientInfo {
        platform: "cli-test".into(),
    }
}

#[tokio::test]
async fn trusted_flow_sends_events() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = TimApiTestCtx::new()?;
    let api = ctx.api();

    let alpha_session = api
        .trusted_register(&TrustedRegisterReq {
            nick: "alpha".into(),
            client_info: Some(client_info()),
        })
        .await?
        .session
        .expect("missing alpha session");

    api.declare_abilities(
        &DeclareAbilitiesReq {
            abilities: Vec::new(),
        },
        &alpha_session,
    )
    .await?;

    let reconnect_session = api
        .trusted_connect(&TrustedConnectReq {
            timite: Some(Timite {
                id: alpha_session.timite_id,
                nick: "alpha".into(),
            }),
            client_info: Some(client_info()),
        })
        .await?
        .session
        .expect("missing reconnect session");

    assert_ne!(
        reconnect_session.key, alpha_session.key,
        "new session should have a distinct key"
    );
    assert_eq!(
        reconnect_session.timite_id, alpha_session.timite_id,
        "reconnect must keep the same timite id"
    );

    let beta_session = api
        .trusted_register(&TrustedRegisterReq {
            nick: "beta".into(),
            client_info: Some(client_info()),
        })
        .await?
        .session
        .expect("missing beta session");

    let mut beta_events = api
        .subscribe(
            &SubscribeToSpaceReq {
                receive_own_messages: false,
            },
            &beta_session,
        )
        .await?;

    let content = "ping from alpha";
    let send_res = api
        .send_message(
            &SendMessageReq {
                content: content.into(),
            },
            &reconnect_session,
        )
        .await?;
    assert!(
        send_res.error.is_none(),
        "send_message should not include an error"
    );

    let message = loop {
        let event = timeout(Duration::from_secs(1), beta_events.recv())
            .await?
            .expect("beta subscriber should receive an event");

        match event.data {
            Some(space_event::Data::EventNewMessage(event)) => {
                break event.message.expect("space event missing message");
            }
            Some(space_event::Data::EventTimiteConnected(_)) => continue,
            Some(space_event::Data::EventTimiteDisconnected(_)) => continue,
            other => panic!("unexpected event event {:?}", other),
        }
    };

    assert_eq!(message.content, content);
    assert_eq!(
        message.sender_id, reconnect_session.timite_id,
        "sender id should match the session used to send the message"
    );

    Ok(())
}
