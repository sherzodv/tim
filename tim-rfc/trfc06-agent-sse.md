# Agent-side SSE streaming & tool calls

Goal: adopt the ChatGPT Responses SSE flow (see `sse.md`) inside `tim-agent` so we stream completions and drive tool calls without waiting for full responses.

## Scope
- Client-side only: `tim-agent` LLM client, turn runner, and ability/tool dispatch.
- Maintain current prompt/memory plumbing; no tim-api changes.
- Keep behavior simple: aggregate text for now; stream deltas to peers later.

## Proposal
1) SSE client in `tim-agent/src/llm/chatgpt_sse.rs`
- Build a thin wrapper over `reqwest` + `eventsource_stream::Eventsource`.
- POST chat/completions with `stream=true`, `model`, `messages`, `temperature`; reuse existing auth/env.
- Parse into `SseEvent { kind, response, item, delta, summary_index, content_index }` → map to `ResponseEvent` (`OutputTextDelta`, `OutputItemDone`, `Completed`, `Reasoning*`).
- Expose `stream_chat(req: LlmReq) -> impl Stream<Item = Result<ResponseEvent, LlmError>>`.

2) LLM trait and fallback aggregator
- Extend `Llm` with `chat_stream`; keep `chat` implemented by collecting text deltas until `Completed` (preserves current behavior).
- Keep API surface minimal: no new config knobs beyond stream flag baked into the SSE client.

3) Turn loop integration
- Add a small turn runner in `llm/agent.rs` that consumes `ResponseEvent`:
  - `OutputTextDelta` → append to buffer (future: forward partials to `TimClient::send_message`).
  - `OutputItemDone(ResponseItem::Message/Reasoning)` → update buffer or ignore.
  - `OutputItemDone(ResponseItem::FunctionCall { name, arguments, call_id, .. })` → build `ToolCall { tool_name, call_id, payload }` and dispatch.
  - On `Completed`, send the accumulated message via `TimClient::send_message`, then continue streaming if tools keep running.

4) Tool dispatch (minimal, synchronous-first)
- Introduce `ToolRuntime` alongside the LLM layer:
  - Support `FunctionCall` → map to a small registry (`send_message`, `render_space_abilities`, crawler tasks). Keep parallelism off initially; run one tool at a time.
  - Add `LocalShellCall` handler only for the crawler agent; treat others as unsupported.
  - Return `FunctionCallOutput` payloads back into the conversation as new user messages for the model to resume.
- Surface clear errors for unknown tool names/invalid JSON; keep them visible in logs but non-fatal to the stream.

5) Cancellation and timeouts
- Reuse the existing turn loop select; add an idle timeout on the SSE stream (5m like `sse.md`).
- Propagate `CancellationToken` to tool tasks so we can abort long shells/crawler jobs.

## Milestones
- M1: SSE client + `chat_stream` + text aggregation keeps current UX.
- M2: ToolRuntime with `FunctionCall` + `LocalShellCall` wired for crawler; end-to-end tool roundtrip.
- M3: Optional streaming of text deltas to peers/UI; parallel tool execution if/when needed.
