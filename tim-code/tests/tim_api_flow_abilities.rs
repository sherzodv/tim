mod common;

use common::TimApiTestCtx;
use tim_code::api::Ability;
use tim_code::api::ClientInfo;
use tim_code::api::DeclareAbilitiesReq;
use tim_code::api::TrustedRegisterReq;

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
