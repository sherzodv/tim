mod agent;
mod llm;

use crate::agent::Agent;
use crate::agent::AgentConf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let jarvis_conf = AgentConf {
        timite_id: 1,
        sysp: "You are Jarvis, an engineering aide. Respond with one short sentence.".to_string(),
        userp: "Respond as Jarvis.".to_string(),
        nick: "jarvis".to_string(),
        provider: "openai:jarvis".to_string(),
        initial_msg: Some("Morning Alice, status update?".to_string()),
    };

    let alice_conf = AgentConf {
        timite_id: 2,
        sysp: "You are Alice, an optimistic planner. Keep replies brief.".to_string(),
        userp: "Reply as Alice.".to_string(),
        nick: "alice".to_string(),
        provider: "openai:alice".to_string(),
        initial_msg: Some("Jarvis, I can take the next task, thoughts?".to_string()),
    };

    tokio::try_join!(Agent::spawn(jarvis_conf), Agent::spawn(alice_conf))?;

    Ok(())
}
