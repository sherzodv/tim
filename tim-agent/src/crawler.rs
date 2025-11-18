use async_trait::async_trait;
use reqwest::Client;

use crate::agent::{Agent, AgentBuilder, AgentError};
use crate::tim_client::tim_api::{Ability, CallAbility, CallAbilityOutcome};
use crate::tim_client::TimClient;
use crate::tim_client::{Event, SpaceEvent};

#[derive(Clone)]
pub struct CrawlerConf {
    pub ability_name: String,
    pub max_snippet_chars: usize,
    pub user_agent: String,
}

impl Default for CrawlerConf {
    fn default() -> Self {
        Self {
            ability_name: "web.crawl".to_string(),
            max_snippet_chars: 480,
            user_agent: "tim-crawler/0.1".to_string(),
        }
    }
}

pub struct WebCrawlerAgent {
    client: TimClient,
    conf: CrawlerConf,
    http: Client,
}

impl WebCrawlerAgent {
    pub fn new(conf: &CrawlerConf, client: TimClient) -> Result<Self, AgentError> {
        let http = Client::builder()
            .user_agent(conf.user_agent.clone())
            .build()
            .map_err(|err| AgentError::Crawler(format!("failed to init http client: {err}")))?;

        Ok(Self {
            client,
            conf: conf.clone(),
            http,
        })
    }

    async fn crawl(&self, url: &str) -> Result<String, String> {
        let parsed = reqwest::Url::parse(url).map_err(|err| format!("invalid url: {err}"))?;
        match parsed.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(format!("unsupported scheme: {scheme}"));
            }
        }

        let response = self
            .http
            .get(parsed)
            .send()
            .await
            .map_err(|err| format!("network error: {err}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("http status {}", status.as_u16()));
        }

        let body = response
            .text()
            .await
            .map_err(|err| format!("failed to read body: {err}"))?;

        Ok(self.render_snippet(&body))
    }

    fn render_snippet(&self, body: &str) -> String {
        let mut snippet = String::new();
        for word in body.split_whitespace() {
            if !snippet.is_empty() {
                snippet.push(' ');
            }
            snippet.push_str(word);
            if snippet.len() >= self.conf.max_snippet_chars {
                snippet.truncate(self.conf.max_snippet_chars);
                snippet.push('…');
                break;
            }
        }
        if snippet.is_empty() {
            "page returned no readable content".to_string()
        } else {
            snippet
        }
    }

    async fn declare(&mut self) -> Result<(), AgentError> {
        self.client
            .declare_abilities(vec![Ability {
                name: self.conf.ability_name.clone(),
                description: "Fetches a web page and returns a short text snippet.".to_string(),
                params: Vec::new(),
            }])
            .await?;
        Ok(())
    }

    async fn respond_outcome(
        &mut self,
        call_id: u64,
        result: Result<String, String>,
    ) -> Result<(), AgentError> {
        let outcome = match result {
            Ok(payload) => CallAbilityOutcome {
                call_ability_id: call_id,
                payload: Some(payload),
                error: None,
            },
            Err(err) => CallAbilityOutcome {
                call_ability_id: call_id,
                payload: None,
                error: Some(err),
            },
        };
        self.client.send_call_ability_outcome(&outcome).await?;
        Ok(())
    }

    async fn handle_call(&mut self, call: &CallAbility) -> Result<(), AgentError> {
        if call.timite_id != self.client.timite_id() {
            return Ok(());
        }
        if call.name != self.conf.ability_name {
            return Ok(());
        }
        let call_id = call
            .call_ability_id
            .ok_or_else(|| AgentError::Crawler("call missing identifier".into()))?;
        let payload = call.payload.trim().to_string();
        if payload.is_empty() {
            self.respond_outcome(call_id, Err("payload must be a URL".into()))
                .await?;
            return Ok(());
        }
        let result = self.crawl(&payload).await;
        self.respond_outcome(call_id, result).await?;
        Ok(())
    }
}

#[async_trait]
impl Agent for WebCrawlerAgent {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        self.declare().await?;
        let announce = format!(
            "Crawler ready. Call `{}` ability with a URL payload.",
            self.conf.ability_name
        );
        self.client.send_message(&announce).await?;
        Ok(())
    }

    async fn on_space_update(&mut self, update: &SpaceEvent) -> Result<(), AgentError> {
        if let Some(Event::EventCallAbility(call_event)) = &update.event {
            if let Some(call) = call_event.call_ability.as_ref() {
                self.handle_call(call).await?;
            }
        }
        Ok(())
    }
}

impl AgentBuilder for CrawlerConf {
    type A = WebCrawlerAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        WebCrawlerAgent::new(self, client)
    }
}
