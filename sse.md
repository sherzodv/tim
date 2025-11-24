# SSE Event Streaming & Tool Call Handling in Codex

This document describes how codex handles Server-Sent Events (SSE) from the ChatGPT API, parses them, and initiates tool calls on-the-fly.

## Table of Contents

- [SSE Stream Structure](#sse-stream-structure)
- [SSE Event Types](#sse-event-types)
- [SSE Parsing Loop](#sse-parsing-loop)
- [Response Data Structures](#response-data-structures)
- [Event-to-ResponseEvent Conversion](#event-to-responseevent-conversion)
- [Turn Loop Processing](#turn-loop-processing)
- [Tool Call Construction](#tool-call-construction)
- [Tool Execution](#tool-execution)
- [Tool Dispatch & Execution Flow](#tool-dispatch--execution-flow)
- [Key Design Patterns](#key-design-patterns)

## SSE Stream Structure

**Library**: Uses `eventsource_stream` crate for SSE parsing

**File**: `codex-rs/core/src/client.rs:17`
```rust
use eventsource_stream::Eventsource;
```

### SSE Event Format

**File**: `codex-rs/core/src/client.rs:556-565`

```rust
#[derive(Debug, Deserialize, Serialize)]
struct SseEvent {
    #[serde(rename = "type")]
    kind: String,              // e.g., "response.output_item.done"
    response: Option<Value>,   // For response.completed, response.failed
    item: Option<Value>,       // For output_item events (tool calls, messages)
    delta: Option<String>,     // For text streaming deltas
    summary_index: Option<i64>,   // For reasoning summaries
    content_index: Option<i64>,   // For reasoning content
}
```

## SSE Event Types

Key event types that codex processes:

| Event Type | Purpose | Contains |
|------------|---------|----------|
| `response.created` | Stream started | Response ID |
| `response.output_item.added` | Item started (streaming begins) | Partial `ResponseItem` |
| `response.output_text.delta` | Text streaming chunk | Delta text |
| `response.output_item.done` | Item completed | Complete `ResponseItem` |
| `response.function_call_arguments.delta` | Function args streaming | Delta JSON |
| `response.custom_tool_call_input.delta` | Custom tool streaming | Delta text |
| `response.reasoning_text.delta` | Reasoning streaming (o1/o3) | Delta text |
| `response.reasoning_summary_text.delta` | Reasoning summary | Delta text |
| `response.completed` | Turn finished | Token usage, response ID |
| `response.failed` | Error occurred | Error details |

## SSE Parsing Loop

**File**: `codex-rs/core/src/client.rs:694-928`

```rust
async fn process_sse<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent>>,
    idle_timeout: Duration,
    otel_event_manager: OtelEventManager,
) where
    S: Stream<Item = Result<Bytes>> + Unpin,
{
    let mut stream = stream.eventsource();  // Convert byte stream to SSE events

    let mut response_completed: Option<ResponseCompleted> = None;
    let mut response_error: Option<CodexErr> = None;

    loop {
        // Wait for next SSE event with timeout (default: 5 minutes)
        let response = timeout(idle_timeout, stream.next()).await;

        let sse = match response {
            Ok(Some(Ok(sse))) => sse,      // Got valid SSE event
            Ok(Some(Err(e))) => { /* handle error */ },
            Ok(None) => {                   // Stream ended
                if let Some(ResponseCompleted { id, usage }) = response_completed {
                    // Send final completion event with token usage
                    tx_event.send(Ok(ResponseEvent::Completed {
                        response_id: id,
                        token_usage: usage
                    })).await;
                } else {
                    // Stream ended without completion - error!
                    tx_event.send(Err(CodexErr::Stream(
                        "stream closed before response.completed".into(), None
                    ))).await;
                }
                return;
            }
            Err(_) => { /* idle timeout */ }
        };

        // Parse SSE data as JSON
        let event: SseEvent = match serde_json::from_str(&sse.data) {
            Ok(event) => event,
            Err(e) => continue,  // Skip malformed events
        };

        // Dispatch based on event type
        match event.kind.as_str() {
            "response.output_item.done" => { /* ... */ }
            "response.output_text.delta" => { /* ... */ }
            "response.completed" => { /* ... */ }
            // ... more event types
        }
    }
}
```

### Key Points

- **Idle timeout**: Default 5 minutes (300 seconds) of inactivity before treating as disconnected
- **Error handling**: Malformed JSON events are logged but don't crash the stream
- **Completion tracking**: Stores `response.completed` data until stream ends

## Response Data Structures

**File**: `codex-rs/protocol/src/models.rs:49-130`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseItem {
    Message {
        id: Option<String>,
        role: String,
        content: Vec<ContentItem>,
    },
    Reasoning {
        id: String,
        summary: Vec<ReasoningItemReasoningSummary>,
        content: Option<Vec<ReasoningItemContent>>,
        encrypted_content: Option<String>,
    },
    FunctionCall {
        id: Option<String>,
        name: String,          // Tool name
        arguments: String,     // JSON string (not pre-parsed!)
        call_id: String,       // For matching with output
    },
    FunctionCallOutput {
        call_id: String,
        output: FunctionCallOutputPayload,
    },
    CustomToolCall {
        id: Option<String>,
        call_id: String,
        name: String,
        input: String,
    },
    CustomToolCallOutput {
        call_id: String,
        output: String,
    },
    LocalShellCall {
        id: Option<String>,
        call_id: Option<String>,
        status: LocalShellStatus,
        action: LocalShellAction,  // Contains command, workdir, etc.
    },
    WebSearchCall {
        id: Option<String>,
        status: Option<String>,
        action: WebSearchAction,
    },
    GhostSnapshot { /* ... */ },
    Other,
}
```

### Important Notes

- **Tagged enum**: Uses `#[serde(tag = "type")]` for JSON deserialization
- **Arguments as string**: `FunctionCall.arguments` is a JSON string, not pre-parsed Value
- **Call ID tracking**: `call_id` links function calls to their outputs in conversation history

## Event-to-ResponseEvent Conversion

**File**: `codex-rs/core/src/client.rs:803-926`

```rust
match event.kind.as_str() {
    "response.output_item.done" => {
        let Some(item_val) = event.item else { continue };

        // Deserialize ResponseItem from JSON
        let Ok(item) = serde_json::from_value::<ResponseItem>(item_val) else {
            debug!("failed to parse ResponseItem from output_item.done");
            continue;
        };

        // Send to turn loop
        let event = ResponseEvent::OutputItemDone(item);
        if tx_event.send(Ok(event)).await.is_err() {
            return;
        }
    }

    "response.output_text.delta" => {
        if let Some(delta) = event.delta {
            let event = ResponseEvent::OutputTextDelta(delta);
            if tx_event.send(Ok(event)).await.is_err() {
                return;
            }
        }
    }

    "response.completed" => {
        if let Some(resp_val) = event.response {
            match serde_json::from_value::<ResponseCompleted>(resp_val) {
                Ok(r) => {
                    response_completed = Some(r);  // Store for stream end
                }
                Err(e) => {
                    response_error = Some(CodexErr::Stream(error, None));
                }
            };
        };
    }

    // Ignored events (no action needed)
    "response.function_call_arguments.delta" |
    "response.in_progress" |
    "response.output_text.done" => {}

    _ => {}  // Unknown events ignored
}
```

### ResponseEvent Enum

**File**: `codex-rs/core/src/client_common.rs:197-218`

```rust
#[derive(Debug)]
pub enum ResponseEvent {
    Created,
    OutputItemDone(ResponseItem),        // Complete item (tool call, message, etc.)
    OutputItemAdded(ResponseItem),       // Item started streaming
    Completed {
        response_id: String,
        token_usage: Option<TokenUsage>,
    },
    OutputTextDelta(String),             // Text streaming chunk
    ReasoningSummaryDelta {
        delta: String,
        summary_index: i64,
    },
    ReasoningContentDelta {
        delta: String,
        content_index: i64,
    },
    ReasoningSummaryPartAdded {
        summary_index: i64,
    },
    RateLimits(RateLimitSnapshot),
}
```

## Turn Loop Processing

**File**: `codex-rs/core/src/codex.rs:2095-2269`

```rust
let event = match stream.next().await {
    Some(Ok(event)) => event,
    Some(Err(e)) => return Err(e),
    None => break,
};

match event {
    ResponseEvent::OutputItemDone(item) => {
        // Try to build a tool call from the ResponseItem
        match ToolRouter::build_tool_call(sess.as_ref(), item.clone()) {
            Ok(Some(call)) => {
                // It's a tool call! Execute it.
                let payload_preview = call.payload.log_payload().into_owned();
                tracing::info!("ToolCall: {} {}", call.tool_name, payload_preview);

                // Dispatch to tool runtime (parallel or sequential)
                let response = tool_runtime
                    .handle_tool_call(call, cancellation_token.child_token());

                // Wait for tool execution result
                output.push_back(async move {
                    Ok(ProcessedResponseItem {
                        item,
                        response: Some(response.await?),
                    })
                }.boxed());
            }
            Ok(None) => {
                // Not a tool call (e.g., assistant message, reasoning)
                if let Some(turn_item) = handle_non_tool_response_item(&item).await {
                    sess.emit_turn_item_started(&turn_context, &turn_item).await;
                    sess.emit_turn_item_completed(&turn_context, turn_item).await;
                }

                output.push_back(/* item without tool execution */);
            }
            Err(err) => {
                // Tool call error (missing ID, denied, etc.)
                // Return error response to model
            }
        }
    }

    ResponseEvent::OutputTextDelta(delta) => {
        // Stream text to UI in real-time
        sess.send_event(&turn_context, EventMsg::AgentMessageContentDelta(event)).await;
    }

    ResponseEvent::Completed { response_id, token_usage } => {
        // Turn finished
        sess.update_token_usage_info(&turn_context, token_usage.as_ref()).await;

        // Wait for all tool executions to complete
        let processed_items = output.try_collect().await?;

        return Ok(TurnRunResult {
            processed_items,
            total_token_usage: token_usage,
        });
    }

    // ... other events
}
```

### Key Behaviors

- **On-the-fly execution**: Tool calls are executed immediately as they arrive, not batched
- **Async processing**: Multiple tools can execute concurrently
- **Streaming to UI**: Text deltas are forwarded to the user interface in real-time
- **Completion waiting**: Turn doesn't finish until all tool executions complete

## Tool Call Construction

**File**: `codex-rs/core/src/tools/router.rs:57-130`

```rust
pub fn build_tool_call(
    session: &Session,
    item: ResponseItem,
) -> Result<Option<ToolCall>, FunctionCallError> {
    match item {
        // Standard function calls (Read, Write, Edit, Bash, etc.)
        ResponseItem::FunctionCall { name, arguments, call_id, .. } => {
            // Check if it's an MCP tool (format: "server__tool")
            if let Some((server, tool)) = session.parse_mcp_tool_name(&name) {
                Ok(Some(ToolCall {
                    tool_name: name,
                    call_id,
                    payload: ToolPayload::Mcp {
                        server,
                        tool,
                        raw_arguments: arguments,
                    },
                }))
            } else {
                // Built-in tool or unified_exec
                let payload = if name == "unified_exec" {
                    ToolPayload::UnifiedExec { arguments }
                } else {
                    ToolPayload::Function { arguments }
                };
                Ok(Some(ToolCall {
                    tool_name: name,
                    call_id,
                    payload,
                }))
            }
        }

        // Custom tools (experimental)
        ResponseItem::CustomToolCall { name, input, call_id, .. } => {
            Ok(Some(ToolCall {
                tool_name: name,
                call_id,
                payload: ToolPayload::Custom { input },
            }))
        }

        // Local shell execution (Responses API native)
        ResponseItem::LocalShellCall { id, call_id, action, .. } => {
            let call_id = call_id
                .or(id)
                .ok_or(FunctionCallError::MissingLocalShellCallId)?;

            match action {
                LocalShellAction::Exec(exec) => {
                    let params = ShellToolCallParams {
                        command: exec.command,
                        workdir: exec.working_directory,
                        timeout_ms: exec.timeout_ms,
                        with_escalated_permissions: None,
                        justification: None,
                    };
                    Ok(Some(ToolCall {
                        tool_name: "local_shell".to_string(),
                        call_id,
                        payload: ToolPayload::LocalShell { params },
                    }))
                }
            }
        }

        // Not a tool call (Message, Reasoning, etc.)
        _ => Ok(None),
    }
}
```

### ToolCall Structure

**File**: `codex-rs/core/src/tools/router.rs:21-25`

```rust
#[derive(Clone)]
pub struct ToolCall {
    pub tool_name: String,   // e.g., "Read", "Bash", "Write", "mcp__server__tool"
    pub call_id: String,     // For matching call with output
    pub payload: ToolPayload,
}
```

### ToolPayload Enum

**File**: `codex-rs/core/src/tools/context.rs`

```rust
pub enum ToolPayload {
    Function { arguments: String },          // JSON string
    Custom { input: String },                // Raw string
    LocalShell { params: ShellToolCallParams },
    UnifiedExec { arguments: String },
    Mcp {
        server: String,
        tool: String,
        raw_arguments: String,
    },
}
```

## Tool Execution

**File**: `codex-rs/core/src/tools/parallel.rs:44-89`

```rust
pub(crate) fn handle_tool_call(
    &self,
    call: ToolCall,
    cancellation_token: CancellationToken,
) -> impl Future<Output = Result<ResponseInputItem, CodexErr>> {
    let supports_parallel = self.router.tool_supports_parallel(&call.tool_name);

    let router = Arc::clone(&self.router);
    let session = Arc::clone(&self.session);
    let turn = Arc::clone(&self.turn_context);
    let tracker = Arc::clone(&self.tracker);
    let lock = Arc::clone(&self.parallel_execution);

    // Spawn task with cancellation support
    let handle = AbortOnDropHandle::new(tokio::spawn(async move {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                // User interrupted - return aborted response
                Ok(Self::aborted_response(&call, elapsed_secs))
            },
            res = async {
                // Acquire lock (read for parallel, write for sequential)
                let _guard = if supports_parallel {
                    Either::Left(lock.read().await)   // Multiple tools can run
                } else {
                    Either::Right(lock.write().await)  // Exclusive execution
                };

                // Dispatch to tool registry
                router.dispatch_tool_call(session, turn, tracker, call.clone()).await
            } => res,
        }
    }));

    async move {
        match handle.await {
            Ok(Ok(response)) => Ok(response),  // Tool succeeded
            Ok(Err(FunctionCallError::Fatal(message))) => Err(CodexErr::Fatal(message)),
            Ok(Err(other)) => Err(CodexErr::Fatal(other.to_string())),
            Err(err) => Err(CodexErr::Fatal(format!("tool task failed: {err:?}"))),
        }
    }
}
```

### Parallel vs Sequential Execution

**Tools that support parallel execution** (can run concurrently):
- `Read` - Reading files
- `Glob` - File pattern matching
- `Grep` - Code search
- View-only operations

**Tools that require sequential execution** (run one at a time):
- `Write` - Creating files
- `Edit` - Modifying files
- `Bash` - Shell command execution
- Any operation that modifies filesystem state

### Cancellation Support

All tool calls support cancellation via `CancellationToken`:
- User can interrupt long-running operations
- Aborted tools return a special "aborted by user" message
- Elapsed time is included in abort message for shell commands

## Tool Dispatch & Execution Flow

```
1. SSE bytes arrive from ChatGPT API
   ↓
2. eventsource_stream parses into SSE events
   ↓
3. process_sse() deserializes JSON from event.data
   ↓
4. Match event.kind:
   - "response.output_item.done" → Deserialize ResponseItem
   - "response.output_text.delta" → Extract delta string
   - "response.completed" → Extract token usage
   ↓
5. Send ResponseEvent to turn loop via mpsc channel
   ↓
6. Turn loop receives ResponseEvent::OutputItemDone(item)
   ↓
7. ToolRouter::build_tool_call(item) pattern matches:
   - ResponseItem::FunctionCall → ToolPayload::Function
   - ResponseItem::LocalShellCall → ToolPayload::LocalShell
   - ResponseItem::CustomToolCall → ToolPayload::Custom
   - ResponseItem::Message → None (not a tool call)
   ↓
8. If tool call: ToolCallRuntime::handle_tool_call()
   ↓
9. Spawn tokio task with:
   - Parallel lock (RwLock::read) for parallel-safe tools
   - Sequential lock (RwLock::write) for sequential tools
   ↓
10. ToolRouter::dispatch_tool_call() → ToolRegistry::dispatch()
    ↓
11. Match tool name:
    - "Read" → ReadHandler::handle()
    - "Bash" → BashHandler::handle()
    - "Write" → WriteHandler::handle()
    - "mcp__*" → McpHandler::handle()
    - etc.
    ↓
12. Tool handler executes (reads file, runs command, etc.)
    ↓
13. Return ResponseInputItem::FunctionCallOutput
    ↓
14. Turn loop collects all tool outputs
    ↓
15. On ResponseEvent::Completed:
    - Send all tool outputs back to API
    - Update token usage
    - Emit turn completion event
```

## Key Design Patterns

### 1. Streaming-First Architecture
Events are processed incrementally as they arrive, not buffered until completion. This enables:
- Real-time UI updates
- Immediate tool execution
- Lower latency user experience

### 2. Type Safety
SSE JSON is deserialized into strongly-typed Rust structs using serde:
- Compile-time guarantees about data structure
- Pattern matching for exhaustive event handling
- Automatic validation of incoming data

### 3. Error Recovery
- Malformed events are logged but don't crash the stream
- Unknown event types are safely ignored
- Stream continues processing after recoverable errors

### 4. Parallel Execution
Read-only tools (Read, Glob, Grep) run concurrently using `RwLock::read()`:
- Multiple read operations can execute simultaneously
- Write tools acquire exclusive lock with `RwLock::write()`
- Maximizes throughput for independent operations

### 5. Cancellation Support
All tool calls can be interrupted mid-execution:
- Uses `tokio::select!` with `CancellationToken`
- Returns meaningful abort messages with elapsed time
- Prevents zombie processes from continuing after user cancellation

### 6. Call ID Matching
`call_id` links `FunctionCall` → `FunctionCallOutput`:
- Maintains conversation history integrity
- Allows API to correlate requests with responses
- Supports multiple concurrent tool calls in single turn

### 7. Async All The Way
SSE parsing, tool execution, and event emission are all async/await:
- Non-blocking I/O throughout the stack
- Efficient resource utilization
- Natural composition of async operations

### 8. Channel-Based Communication
Uses `mpsc::channel` for SSE → Turn Loop communication:
- Decouples SSE parsing from tool execution
- Backpressure handling if turn loop is slow
- Clean separation of concerns

## Related Files

### Core SSE Handling
- `codex-rs/core/src/client.rs` - SSE parsing and event conversion
- `codex-rs/core/src/client_common.rs` - ResponseEvent enum definition
- `codex-rs/core/src/chat_completions.rs` - Chat Completions API SSE handling

### Tool System
- `codex-rs/core/src/tools/router.rs` - Tool call construction and routing
- `codex-rs/core/src/tools/parallel.rs` - Parallel/sequential execution runtime
- `codex-rs/core/src/tools/registry.rs` - Tool registration and dispatch

### Data Models
- `codex-rs/protocol/src/models.rs` - ResponseItem and related types
- `codex-rs/protocol/src/protocol.rs` - Event types for client communication

### Turn Loop
- `codex-rs/core/src/codex.rs` - Main turn loop and event processing

## Token Usage Tracking

Token usage is extracted from the `response.completed` event:

**File**: `codex-rs/core/src/client.rs:568-608`

```rust
#[derive(Debug, Deserialize)]
struct ResponseCompleted {
    id: String,
    usage: Option<ResponseCompletedUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponseCompletedUsage {
    input_tokens: i64,
    input_tokens_details: Option<ResponseCompletedInputTokensDetails>,
    output_tokens: i64,
    output_tokens_details: Option<ResponseCompletedOutputTokensDetails>,
    total_tokens: i64,
}

#[derive(Debug, Deserialize)]
struct ResponseCompletedInputTokensDetails {
    cached_tokens: i64,  // Prompt caching saves API costs
}

#[derive(Debug, Deserialize)]
struct ResponseCompletedOutputTokensDetails {
    reasoning_tokens: i64,  // For o1/o3 reasoning models
}
```

Token usage is sent to the turn loop in the `ResponseEvent::Completed` event and then:
1. Updated in the session's context manager
2. Displayed to the user via UI events
3. Used for auto-compaction decisions

## Rate Limiting

Rate limit information can arrive via HTTP headers on the initial response:

**File**: `codex-rs/core/src/client.rs:637-688`

```rust
fn parse_rate_limit_snapshot(headers: &HeaderMap) -> Option<RateLimitSnapshot> {
    let primary = parse_rate_limit_window(
        headers,
        "x-codex-primary-used-percent",
        "x-codex-primary-window-minutes",
        "x-codex-primary-reset-at",
    );

    let secondary = parse_rate_limit_window(
        headers,
        "x-codex-secondary-used-percent",
        "x-codex-secondary-window-minutes",
        "x-codex-secondary-reset-at",
    );

    if primary.is_some() || secondary.is_some() {
        Some(RateLimitSnapshot { primary, secondary })
    } else {
        None
    }
}
```

Rate limits are also fetched periodically via `/api/codex/usage` endpoint (see `codex-rs/backend-client/src/client.rs:158-167`).
