use std::time::Duration;

mod common;

use common::TimApiTestCtx;
use tim_code::api::space_update;
use tim_code::api::Ability;
use tim_code::api::CallAbility;
use tim_code::api::CallAbilityOutcome;
use tim_code::api::ClientInfo;
use tim_code::api::DeclareAbilitiesReq;
use tim_code::api::SendCallAbilityOutcomeReq;
use tim_code::api::SendCallAbilityReq;
use tim_code::api::SubscribeToSpaceReq;
use tim_code::api::TrustedRegisterReq;
use tokio::time::timeout;

fn client_info() -> ClientInfo {
    ClientInfo {
        platform: "cli-test".into(),
    }
}

fn sample_abilities() -> Vec<Ability> {
    vec![
        Ability {
            name: "echo".into(),
            description: "Echo input back to the caller".into(),
            params: Vec::new(),
        },
        Ability {
            name: "ping".into(),
            description: "Health check signal".into(),
            params: Vec::new(),
        },
    ]
}

#[tokio::test]
async fn tim_api_flow_abilities_list_declared_skills() -> Result<(), Box<dyn std::error::Error>> {
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

    let abilities = sample_abilities();
    api.declare_abilities(
        &DeclareAbilitiesReq {
            abilities: abilities.clone(),
        },
        &session,
    )
    .await?;

    let res = api.list_abilities().await?;
    assert_eq!(
        res.abilities.len(),
        1,
        "fresh store should only contain the declaring timite"
    );

    let entry = res
        .abilities
        .into_iter()
        .find(|ta| ta.timite.as_ref().map(|t| t.id) == Some(session.timite_id))
        .expect("timite abilities entry missing");

    let stored = entry.abilities;
    assert_eq!(stored.len(), abilities.len());

    for ability in &abilities {
        assert!(
            stored
                .iter()
                .any(|stored_ability| stored_ability.name == ability.name),
            "expected to find ability {} in the stored list",
            ability.name
        );
    }

    Ok(())
}

#[tokio::test]
async fn tim_api_flow_abilities_call_cycle() -> Result<(), Box<dyn std::error::Error>> {
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
            abilities: sample_abilities(),
        },
        &alpha_session,
    )
    .await?;

    let mut alpha_updates = api.subscribe(
        &SubscribeToSpaceReq {
            receive_own_messages: false,
        },
        &alpha_session,
    );

    let beta_session = api
        .trusted_register(&TrustedRegisterReq {
            nick: "beta".into(),
            client_info: Some(client_info()),
        })
        .await?
        .session
        .expect("missing beta session");

    let ability_payload = "ping beta -> alpha";
    let ability_name = "echo";
    let call_res = api
        .send_call_ability(
            &SendCallAbilityReq {
                call_ability: Some(CallAbility {
                    timite_id: alpha_session.timite_id,
                    sender_id: 0,
                    name: ability_name.into(),
                    payload: ability_payload.into(),
                    call_ability_id: None,
                }),
            },
            &beta_session,
        )
        .await?;

    let call_ability_id = call_res.call_ability_id;
    assert!(
        call_ability_id > 0,
        "call ability ids should be positive integers"
    );

    let alpha_call_update = timeout(Duration::from_secs(1), alpha_updates.recv())
        .await?
        .expect("alpha should receive call ability update");

    let call_event = match alpha_call_update.event {
        Some(space_update::Event::CallAbility(event)) => event,
        other => panic!("unexpected alpha update event: {:?}", other),
    };

    assert_eq!(
        call_event.call_ability_id,
        Some(call_ability_id),
        "call ability update should carry the stored id"
    );
    assert_eq!(
        call_event.timite_id, alpha_session.timite_id,
        "call ability target should match alpha timite"
    );
    assert_eq!(
        call_event.sender_id, beta_session.timite_id,
        "call ability sender should be rewritten to beta"
    );
    assert_eq!(call_event.name, ability_name);
    assert_eq!(call_event.payload, ability_payload);

    let mut beta_updates = api.subscribe(
        &SubscribeToSpaceReq {
            receive_own_messages: false,
        },
        &beta_session,
    );

    let outcome_payload = "echo-complete";
    api.send_call_ability_outcome(
        &SendCallAbilityOutcomeReq {
            outcome: Some(CallAbilityOutcome {
                call_ability_id,
                payload: Some(outcome_payload.into()),
                error: None,
            }),
        },
        &alpha_session,
    )
    .await?;

    let beta_outcome_update = timeout(Duration::from_secs(1), beta_updates.recv())
        .await?
        .expect("beta should receive call ability outcome");

    let outcome_event = match beta_outcome_update.event {
        Some(space_update::Event::CallAbilityOutcome(event)) => event,
        other => panic!("unexpected beta update event: {:?}", other),
    };

    assert_eq!(
        outcome_event.call_ability_id, call_ability_id,
        "outcome should reference the original call id"
    );
    assert_eq!(
        outcome_event.payload.as_deref(),
        Some(outcome_payload),
        "payload should match what alpha sent back"
    );
    assert!(
        outcome_event.error.is_none(),
        "success outcome must not include an error"
    );

    Ok(())
}
