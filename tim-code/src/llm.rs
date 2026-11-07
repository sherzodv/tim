//! Runs coding tasks with a model from tim-llm calling the native tools in a loop.
//!
//! Branch history restores model turns verbatim, including reasoning and tool calls.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tim_llm::llm::build_llm;
use tim_llm::llm::collect;
use tim_llm::llm::Llm;
use tim_llm::llm::LlmConf;
use tim_llm::llm::LlmError;
use tim_llm::llm::LlmItem;
use tim_llm::llm::LlmReq;
use tim_llm::llm::LlmToolCall;
use tokio::time::timeout;
use tracing::info;

use crate::llm_call::LlmCall;
use crate::progress::Reporter;
use crate::text::head;
use crate::tools;
use crate::tools::Tools;

const PROMPT: &str = include_str!("../prompts/llm.md");
const STEP_CHARS: usize = 80;
const NO_OUTPUT: &str = "(no output)";

#[derive(Debug, thiserror::Error)]
pub enum LlmCodeError {
    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error("task not finished in {0} steps")]
    StepLimit(usize),

    #[error("task timed out after {0:?}")]
    Timeout(Duration),
}

#[derive(Clone)]
pub struct LlmCode {
    llm: Arc<dyn Llm>,
    provider: String,
    tools: Tools,
    /// Model turns one task may take.
    max_steps: usize,
}

impl LlmCode {
    pub fn new(
        provider: &str,
        conf: &LlmConf,
        tools: Tools,
        max_steps: usize,
    ) -> Result<Self, LlmError> {
        Ok(Self {
            llm: build_llm(provider, conf)?,
            provider: provider.to_string(),
            tools,
            max_steps,
        })
    }

    /// Runs the task, reporting each step, and returns the model's final answer.
    pub async fn task(
        &self,
        description: &str,
        history: Vec<LlmItem>,
        reporter: &Reporter,
    ) -> Result<String, LlmCodeError> {
        let limit = self.tools.shell.timeout();
        timeout(limit, self.run(description, history, reporter))
            .await
            .map_err(|_| LlmCodeError::Timeout(limit))?
    }

    async fn run(
        &self,
        description: &str,
        mut items: Vec<LlmItem>,
        reporter: &Reporter,
    ) -> Result<String, LlmCodeError> {
        let definitions = tools::definitions();
        items.push(LlmItem::User(description.to_string()));

        for step in 1..=self.max_steps {
            reporter.step("thinking".to_string());
            let req = LlmReq {
                sysp: PROMPT,
                inputs: &items,
                tools: &definitions,
            };
            let stream = self.llm.chat_stream(&req).await?;
            let mut call = LlmCall::new(reporter, step as u64, &self.provider);
            let result = {
                let mut stream = stream.inspect(|event| {
                    if let Ok(event) = event {
                        call.record(event);
                    }
                });
                collect(&mut stream).await
            };
            call.finish(result.as_ref().err());
            let turn = result?;
            items.push(LlmItem::Turn(turn.items));

            if turn.tool_calls.is_empty() {
                info!(steps = step, "llm task finished");
                return Ok(turn.text.trim().to_string());
            }
            for call in turn.tool_calls {
                reporter.step(step_label(&call));
                let ability = tools::ability_name(&call.name);
                let result = tools::call(&self.tools, &ability, &call.arguments, reporter)
                    .await
                    .map_err(|error| error.to_string());
                reporter.tool_call(
                    call.call_id.clone(),
                    ability,
                    call.arguments.clone(),
                    result.clone(),
                );
                items.push(LlmItem::ToolOutput {
                    call_id: call.call_id,
                    output: tool_output(result),
                });
            }
        }
        Err(LlmCodeError::StepLimit(self.max_steps))
    }
}

/// Tool failures go back to the model as text: it decides what to do next.
pub(crate) fn tool_output(result: Result<String, String>) -> String {
    match result {
        Ok(output) if output.trim().is_empty() => NO_OUTPUT.to_string(),
        Ok(output) => output,
        Err(error) => format!("error: {error}"),
    }
}

fn step_label(call: &LlmToolCall) -> String {
    let ability = tools::ability_name(&call.name);
    let label = match tools::detail(&call.arguments) {
        Some(detail) => format!("{ability} {}", detail.lines().next().unwrap_or("")),
        None => ability,
    };
    head(&label, STEP_CHARS)
}
