mod common;

use common::TimApiTestCtx;
use tim_code::api::space_event;
use tim_code::api::ClientInfo;
use tim_code::api::GetTimelineReq;
use tim_code::api::SendMessageReq;
use tim_code::api::TrustedRegisterReq;

fn client_info() -> ClientInfo {
    ClientInfo {
        platform: "timeline-test".into(),
    }
}

#[tokio::test]
async fn tim_api_get_timeline_returns_events() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = TimApiTestCtx::new()?;
    let api = ctx.api();

    let session = api
        .trusted_register(&TrustedRegisterReq {
            nick: "alpha".into(),
            client_info: Some(client_info()),
        })
        .await?
        .session
        .expect("missing alpha session");

    let first_content = "timeline ping one";
    let second_content = "timeline ping two";

    for content in [first_content, second_content] {
        api.send_message(
            &SendMessageReq {
                content: content.into(),
            },
            &session,
        )
        .await?;
    }

    let timeline = api.get_timeline(
        &GetTimelineReq {
            offset: 0,
            size: 10,
        },
        &session,
    )?;

    assert!(
        !timeline.events.is_empty(),
        "expected timeline to contain at least one event"
    );

    let mut message_events = timeline
        .events
        .iter()
        .filter_map(|event| match (&event.data, event.metadata.as_ref()) {
            (Some(space_event::Data::EventNewMessage(new_msg)), Some(metadata)) => {
                Some((metadata.id, new_msg))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        message_events.len() >= 2,
        "timeline should contain both message events"
    );

    message_events.sort_by_key(|(id, _)| *id);

    let (_, second_event) = message_events[1];
    let second_id = message_events[1].0;

    assert_eq!(
        second_event
            .message
            .as_ref()
            .expect("second event missing payload")
            .content,
        second_content
    );

    let filtered = api.get_timeline(
        &GetTimelineReq {
            offset: second_id,
            size: 10,
        },
        &session,
    )?;

    let first_filtered = filtered
        .events
        .first()
        .and_then(|event| match &event.data {
            Some(space_event::Data::EventNewMessage(new_msg)) => Some(new_msg),
            _ => None,
        })
        .expect("filtered timeline should contain events")
        .message
        .as_ref()
        .expect("filtered message missing payload");

    assert_eq!(first_filtered.content, second_content);

    Ok(())
}
