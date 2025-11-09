use std::collections::VecDeque;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::metadata::errors::InvalidMetadataValue;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;
use tracing::debug;

pub mod tim_api {
    tonic::include_proto!("tim.api.g1");
}

use tim_api::tim_api_client::TimApiClient;
use tim_api::{AuthenticateReq, ClientInfo, SendMessageReq, Timite};

use crate::agent::tim_api::space_update::Event;
use crate::agent::tim_api::{SpaceNewMessage, SubscribeToSpaceReq};
use crate::llm::{ChatGpt, Llm, LlmConf, LlmError, LlmReq};

const SESSION_METADATA_KEY: &str = "tim-session-id";

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("tim connect error: {0}")]
    TimConnect(#[from] tonic::transport::Error),

    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("llm error: {0}")]
    Llm(#[from] LlmError),

    #[error("missing session id in authenticate response")]
    MissingSession,

    #[error("invalid session metadata value: {0}")]
    SessionMetadata(#[from] InvalidMetadataValue),
}

pub struct Agent {
    sysp: String,
    userp: String,
    llm: Arc<dyn Llm>,
    history: VecDeque<DialogTurn>,
    history_limit: usize,
    response_delay: Duration,
}

#[derive(Clone)]
pub struct AgentConf {
    pub timite_id: u64,
    pub sysp: String,
    pub userp: String,
    pub nick: String,
    pub provider: String,
    pub initial_msg: Option<String>,
    pub history_limit: usize,
    pub response_delay_ms: u64,
    pub llm: LlmConf,
}

#[derive(Clone, Copy)]
enum DialogRole {
    Peer,
    Agent,
}

struct DialogTurn {
    role: DialogRole,
    content: String,
}

impl Agent {
    pub async fn spawn(conf: AgentConf) -> Result<(), AgentError> {
        let AgentConf {
            timite_id,
            sysp,
            userp,
            nick,
            provider,
            initial_msg,
            history_limit: history_capacity,
            response_delay_ms,
            llm: llm_conf,
        } = conf;

        let endpoint = Endpoint::from_static("http://localhost:8787");
        let channel = endpoint.connect().await?;
        let mut client = TimApiClient::new(channel);
        let llm: Arc<dyn Llm> = Arc::new(ChatGpt::new(llm_conf)?);
        let history_limit = history_capacity.max(1);
        let response_delay = Duration::from_millis(response_delay_ms);
        let mut agent = Agent {
            sysp,
            userp,
            llm,
            history: VecDeque::with_capacity(history_limit),
            history_limit,
            response_delay,
        };

        let auth_req = AuthenticateReq {
            timite: Some(Timite {
                id: timite_id,
                nick,
            }),
            client_info: Some(ClientInfo { platform: provider }),
        };

        let response = client
            .authenticate(tonic::Request::new(auth_req))
            .await?
            .into_inner();
        let session_id = response
            .session
            .as_ref()
            .map(|s| s.id)
            .ok_or(AgentError::MissingSession)?;
        let session_header_value = MetadataValue::try_from(session_id.to_string())?;

        let sub_req = SubscribeToSpaceReq {
            receive_own_messages: false,
        };

        let mut sub_request = tonic::Request::new(sub_req);
        sub_request
            .metadata_mut()
            .insert(SESSION_METADATA_KEY, session_header_value.clone());
        let mut stream = client.subscribe_to_space(sub_request).await?.into_inner();

        if let Some(initial) = initial_msg
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut init_req = tonic::Request::new(SendMessageReq {
                content: initial.to_string(),
            });
            init_req
                .metadata_mut()
                .insert(SESSION_METADATA_KEY, session_header_value.clone());
            client.send_message(init_req).await?;
        }

        while let Some(upd) = stream.message().await? {
            match upd.event {
                Some(Event::SpaceNewMessage(SpaceNewMessage {
                    message: Some(message),
                })) => {
                    if message.sender_id == timite_id {
                        continue;
                    }
                    let reply = agent.on_message(&message.content).await?;
                    let mut send_request = tonic::Request::new(SendMessageReq { content: reply });
                    send_request
                        .metadata_mut()
                        .insert(SESSION_METADATA_KEY, session_header_value.clone());
                    client.send_message(send_request).await?;
                }
                _ => {
                    eprintln!("Unhandled space update: {:?}", upd);
                }
            }
        }

        dbg!(response);

        Ok(())
    }

    pub async fn on_message(&mut self, msg: &str) -> Result<String, AgentError> {
        if !self.response_delay.is_zero() {
            sleep(self.response_delay).await;
        }
        self.push_history(DialogRole::Peer, msg);
        let context = self.render_history();
        let prompt_body = if context.is_empty() {
            msg.trim().to_string()
        } else {
            format!("Conversation so far:\n{context}\nRespond to the latest peer message.")
        };
        let req = LlmReq {
            sysp: &self.sysp,
            userp: &self.userp,
            msg: &prompt_body,
        };
        debug!(target: "tim_agent::llm", prompt = msg, "Dispatching LLM chat request");
        let answer = self.llm.chat(&req).await?;
        debug!(
            target: "tim_agent::llm",
            response = answer.message.as_str(),
            "Received LLM chat response"
        );
        self.push_history(DialogRole::Agent, &answer.message);
        Ok(answer.message)
    }

    fn push_history(&mut self, role: DialogRole, content: &str) {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.len() == self.history_limit {
            self.history.pop_front();
        }
        self.history.push_back(DialogTurn {
            role,
            content: trimmed.to_string(),
        });
    }

    fn render_history(&self) -> String {
        if self.history.is_empty() {
            return String::new();
        }
        let mut buf = String::new();
        for turn in &self.history {
            let role = match turn.role {
                DialogRole::Peer => "Peer",
                DialogRole::Agent => "Agent",
            };
            buf.push_str(role);
            buf.push_str(": ");
            buf.push_str(&turn.content);
            buf.push('\n');
        }
        buf.trim_end().to_string()
    }
}
