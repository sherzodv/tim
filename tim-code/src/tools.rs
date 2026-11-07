//! Native workspace tools: shell, files and git. Declared to the space as abilities and
//! offered to the model as functions, both served by [`call`].

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tim_client::api::Ability;
use tim_client::api::AbilityParameter;
use tim_llm::llm::LlmTool;

use crate::fs::Files;
use crate::fs::FsError;
use crate::git::Git;
use crate::git::GitError;
use crate::process::{ProcessError, Processes};
use crate::progress::Reporter;
use crate::shell::Shell;
use crate::shell::ShellError;

pub const SH_RUN: &str = "sh.run";
pub const SH_STOP: &str = "sh.stop";
pub const FS_READ: &str = "fs.read";
pub const FS_WRITE: &str = "fs.write";
pub const FS_REPLACE: &str = "fs.replace";
pub const FS_LIST: &str = "fs.list";
pub const FS_SEARCH: &str = "fs.search";
pub const GIT_STATUS: &str = "git.status";
pub const GIT_DIFF: &str = "git.diff";
pub const GIT_LOG: &str = "git.log";
pub const GIT_COMMIT: &str = "git.commit";

const DEFAULT_LOG_COUNT: usize = 10;

#[derive(Clone)]
pub struct Tools {
    pub shell: Shell,
    pub processes: Processes,
    pub files: Files,
    pub git: Git,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error("unknown tool: {0}")]
    Unknown(String),

    #[error("invalid arguments: {0}")]
    Arguments(#[from] serde_json::Error),

    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error(transparent)]
    Fs(#[from] FsError),

    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Deserialize)]
struct ShRunReq {
    cmd: String,
}

#[derive(Deserialize)]
struct ShStopReq {
    process_id: String,
}

#[derive(Deserialize)]
struct FsReadReq {
    path: String,
    from: Option<usize>,
    to: Option<usize>,
}

#[derive(Deserialize)]
struct FsWriteReq {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct FsReplaceReq {
    path: String,
    old: String,
    new: String,
}

#[derive(Deserialize)]
struct FsListReq {
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct FsSearchReq {
    pattern: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct GitDiffReq {
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[derive(Deserialize)]
struct GitLogReq {
    #[serde(default = "default_log_count")]
    count: usize,
}

#[derive(Deserialize)]
struct GitCommitReq {
    message: String,
    #[serde(default)]
    paths: Vec<String>,
}

fn default_log_count() -> usize {
    DEFAULT_LOG_COUNT
}

impl Tools {
    pub fn new(workdir: PathBuf, timeout: Duration, max_output_chars: usize) -> Self {
        let shell = Shell::new(workdir, timeout, max_output_chars);
        Self {
            processes: Processes::default(),
            files: Files::new(shell.clone(), max_output_chars),
            git: Git::new(shell.clone(), max_output_chars),
            shell,
        }
    }
}

/// Runs the named tool with JSON arguments; empty arguments mean none.
pub async fn call(
    tools: &Tools,
    name: &str,
    arguments: &str,
    reporter: &Reporter,
) -> Result<String, ToolError> {
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    match name {
        SH_RUN => {
            let req: ShRunReq = serde_json::from_str(arguments)?;
            Ok(tools
                .processes
                .run(&tools.shell, &req.cmd, reporter)
                .await?)
        }
        SH_STOP => {
            let req: ShStopReq = serde_json::from_str(arguments)?;
            Ok(tools
                .processes
                .stop(&req.process_id, reporter.branch_id())?)
        }
        FS_READ => {
            let req: FsReadReq = serde_json::from_str(arguments)?;
            Ok(tools.files.read(&req.path, req.from, req.to)?)
        }
        FS_WRITE => {
            let req: FsWriteReq = serde_json::from_str(arguments)?;
            Ok(tools.files.write(&req.path, &req.content)?)
        }
        FS_REPLACE => {
            let req: FsReplaceReq = serde_json::from_str(arguments)?;
            Ok(tools.files.replace(&req.path, &req.old, &req.new)?)
        }
        FS_LIST => {
            let req: FsListReq = serde_json::from_str(arguments)?;
            Ok(tools.files.list(&req.path)?)
        }
        FS_SEARCH => {
            let req: FsSearchReq = serde_json::from_str(arguments)?;
            Ok(tools.files.search(&req.pattern, &req.path).await?)
        }
        GIT_STATUS => Ok(tools.git.status().await?),
        GIT_DIFF => {
            let req: GitDiffReq = serde_json::from_str(arguments)?;
            Ok(tools.git.diff(req.path.as_deref(), req.staged).await?)
        }
        GIT_LOG => {
            let req: GitLogReq = serde_json::from_str(arguments)?;
            Ok(tools.git.log(req.count).await?)
        }
        GIT_COMMIT => {
            let req: GitCommitReq = serde_json::from_str(arguments)?;
            Ok(tools.git.commit(&req.message, &req.paths).await?)
        }
        _ => Err(ToolError::Unknown(name.to_string())),
    }
}

/// The argument that tells a call apart, for progress reports.
pub fn detail(arguments: &str) -> Option<String> {
    let arguments: serde_json::Value = serde_json::from_str(arguments).ok()?;
    ["path", "cmd", "pattern", "message", "process_id"]
        .iter()
        .find_map(|field| arguments[field].as_str())
        .map(str::to_string)
}

/// The ability behind a function name the model used: `fs_read` is `fs.read`.
pub fn ability_name(function: &str) -> String {
    function.replacen('_', ".", 1)
}

/// The name the model sees: providers allow no dots in function names.
fn function_name(ability: &str) -> String {
    ability.replace('.', "_")
}

/// The tools as the space sees them: names, descriptions and parameter names.
pub fn abilities() -> Vec<Ability> {
    definitions()
        .into_iter()
        .map(|tool| Ability {
            name: ability_name(&tool.name),
            description: format!("{} Payload: JSON object.", tool.description),
            params: parameters(&tool),
        })
        .collect()
}

fn parameters(tool: &LlmTool) -> Vec<AbilityParameter> {
    let Some(properties) = tool.parameters["properties"].as_object() else {
        return Vec::new();
    };
    properties
        .iter()
        .map(|(name, schema)| AbilityParameter {
            name: name.clone(),
            description: schema["description"].as_str().unwrap_or("").to_string(),
        })
        .collect()
}

/// The tools as the model sees them.
pub fn definitions() -> Vec<LlmTool> {
    vec![
        tool(
            SH_RUN,
            "Runs a task-owned shell command. Streams full output to the space; returns JSON with process_id, reason, exit_code, signal and output tails. Background/daemon processes are not supported.",
            json!({ "cmd": { "type": "string", "description": "shell command" } }),
            &["cmd"],
        ),
        tool(
            SH_STOP,
            "Requests immediate termination of a running command and its process group in this branch. Completion is reported separately in the space.",
            json!({ "process_id": { "type": "string", "description": "opaque ID from the process event, not an OS PID" } }),
            &["process_id"],
        ),
        tool(
            FS_READ,
            "Reads a workspace file as numbered lines.",
            json!({
                "path": { "type": "string", "description": "relative path" },
                "from": { "type": "integer", "description": "first line, 1-based, optional" },
                "to": { "type": "integer", "description": "last line, inclusive, optional" }
            }),
            &["path"],
        ),
        tool(
            FS_WRITE,
            "Creates or overwrites a workspace file.",
            json!({
                "path": { "type": "string", "description": "relative path" },
                "content": { "type": "string", "description": "full file content" }
            }),
            &["path", "content"],
        ),
        tool(
            FS_REPLACE,
            "Replaces one exact occurrence of old text in a workspace file; old must be unique.",
            json!({
                "path": { "type": "string", "description": "relative path" },
                "old": { "type": "string", "description": "exact text to find" },
                "new": { "type": "string", "description": "replacement text" }
            }),
            &["path", "old", "new"],
        ),
        tool(
            FS_LIST,
            "Lists one directory of the workspace, directories end with a slash.",
            json!({
                "path": { "type": "string", "description": "relative path, optional, workspace root by default" }
            }),
            &[],
        ),
        tool(
            FS_SEARCH,
            "Searches workspace files by regex, returns path:line:text.",
            json!({
                "pattern": { "type": "string", "description": "regex" },
                "path": { "type": "string", "description": "relative path to search in, optional" }
            }),
            &["pattern"],
        ),
        tool(
            GIT_STATUS,
            "Shows the branch and changed files.",
            json!({}),
            &[],
        ),
        tool(
            GIT_DIFF,
            "Shows the diff of the workspace.",
            json!({
                "path": { "type": "string", "description": "relative path, optional" },
                "staged": { "type": "boolean", "description": "true for the staged diff, optional" }
            }),
            &[],
        ),
        tool(
            GIT_LOG,
            "Shows recent commits, one per line.",
            json!({
                "count": { "type": "integer", "description": "number of commits, optional, 10 by default" }
            }),
            &[],
        ),
        tool(
            GIT_COMMIT,
            "Stages the given paths, or all changes, and commits them.",
            json!({
                "message": { "type": "string", "description": "commit message" },
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "relative paths to stage, optional, everything by default"
                }
            }),
            &["message"],
        ),
    ]
}

fn tool(
    ability: &str,
    description: &str,
    properties: serde_json::Value,
    required: &[&str],
) -> LlmTool {
    LlmTool {
        name: function_name(ability),
        description: description.to_string(),
        parameters: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }),
    }
}
