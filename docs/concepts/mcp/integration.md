# MCP Integration (Conceptual)

MCP is the runtime boundary between Clickweave and external automation capabilities.

## Role of MCP in the System

- Clickweave does not directly automate OS/browser surfaces.
- It delegates concrete operations to an MCP server subprocess, communicating via JSON-RPC 2.0 over stdio (stdin/stdout pipes).
- The executor stays focused on orchestration, retries, and state.

## Lifecycle Model

There are two distinct spawn lifecycles:

- **Planning**: MCP is spawned briefly to fetch tool schemas (`tools_as_openai()` converts MCP tool definitions to OpenAI function-calling format for use in LLM prompts), then torn down immediately.
- **Execution**: MCP is spawned once at the start of a workflow run, stays alive for all tool calls during the graph walk, and is terminated when the run completes (via Rust `Drop`, which ensures cleanup even on errors).

Within each lifecycle: initialize the connection, query available tools and schemas, call tools as needed, tear down.

## Design Benefits

- Backend stays provider-agnostic at the tool layer.
- Tool schemas are automatically converted to LLM-consumable format for planning and agentic steps.
- Request/response pairs are serialized (`io_lock`), so tool calls are safe from concurrent callers.
- Failures in external automation are isolated at a clear process boundary.

For protocol and exact command behavior, see `docs/reference/mcp/integration.md`.
