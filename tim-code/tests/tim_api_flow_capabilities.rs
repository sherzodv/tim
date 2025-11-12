mod common;

use common::TimApiTestCtx;
use tim_code::api::Capability;
use tim_code::api::ClientInfo;
use tim_code::api::DeclareCapabilitiesReq;
use tim_code::api::TrustedRegisterReq;

fn client_info() -> ClientInfo {
    ClientInfo {
        platform: "cli-test".into(),
    }
}

fn sample_capabilities() -> Vec<Capability> {
    vec![
        Capability {
            name: "echo".into(),
            description: "Echo input back to the caller".into(),
            params: Vec::new(),
        },
        Capability {
            name: "ping".into(),
            description: "Health check signal".into(),
            params: Vec::new(),
        },
    ]
}

#[tokio::test]
async fn tim_api_flow_capabilities_lists_declared_skills() -> Result<(), Box<dyn std::error::Error>>
{
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

    let capabilities = sample_capabilities();
    api.declare_capabilities(
        &DeclareCapabilitiesReq {
            capabilities: capabilities.clone(),
        },
        &session,
    )
    .await?;

    let res = api.list_capabilities().await?;
    assert_eq!(
        res.capabilities.len(),
        1,
        "fresh store should only contain the declaring timite"
    );

    let entry = res
        .capabilities
        .into_iter()
        .find(|tc| tc.timite.as_ref().map(|t| t.id) == Some(session.timite_id))
        .expect("timite capabilities entry missing");

    let stored = entry.capabilities;
    assert_eq!(stored.len(), capabilities.len());

    for cap in &capabilities {
        assert!(
            stored.iter().any(|stored_cap| stored_cap.name == cap.name),
            "expected to find capability {} in the stored list",
            cap.name
        );
    }

    Ok(())
}
