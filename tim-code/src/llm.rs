//! Runs coding tasks with a model from tim-llm calling the native tools in a loop.
//!
//! Each task starts from a clean context: cheap, and no session to keep. The model's
//! turns go back verbatim, so reasoning models keep their thinking across tool calls.

use std::sync::Arc;
use std::time::Duration;

use tim_llm::llm::build_llm;
use tim_llm::llm::Llm;
use tim_llm::llm::LlmConf;
use tim_llm::llm::LlmError;
use tim_llm::llm::LlmItem;
use tim_llm::llm::LlmReq;
use tim_llm::llm::LlmToolCall;
use tokio::time::timeout;
use tracing::info;

use crate::activity::Reporter;
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
            tools,
            max_steps,
        })
    }

    /// Runs the task, reporting each step, and returns the model's final answer.
    pub async fn task(
        &self,
        description: &str,
        reporter: &Reporter,
    ) -> Result<String, LlmCodeError> {
        let limit = self.tools.shell.timeout();
        timeout(limit, self.run(description, reporter))
            .await
            .map_err(|_| LlmCodeError::Timeout(limit))?
    }

    async fn run(&self, description: &str, reporter: &Reporter) -> Result<String, LlmCodeError> {
        let definitions = tools::definitions();
        let mut items = vec![LlmItem::User(description.to_string())];

        for step in 1..=self.max_steps {
            reporter.step("thinking".to_string());
            let req = LlmReq {
                sysp: PROMPT,
                inputs: &items,
                tools: &definitions,
            };
            let turn = self.llm.turn(&req).await?;
            items.push(LlmItem::Turn(turn.items));

            if turn.tool_calls.is_empty() {
                info!(steps = step, "llm task finished");
                return Ok(turn.text.trim().to_string());
            }
            for call in turn.tool_calls {
                reporter.step(step_label(&call));
                let output = self.call(&call).await;
                items.push(LlmItem::ToolOutput {
                    call_id: call.call_id,
                    output,
                });
            }
        }
        Err(LlmCodeError::StepLimit(self.max_steps))
    }

    /// Tool failures go back to the model as text: it decides what to do next.
    async fn call(&self, call: &LlmToolCall) -> String {
        let ability = tools::ability_name(&call.name);
        match tools::call(&self.tools, &ability, &call.arguments).await {
            Ok(output) if output.trim().is_empty() => NO_OUTPUT.to_string(),
            Ok(output) => output,
            Err(error) => format!("error: {error}"),
        }
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
