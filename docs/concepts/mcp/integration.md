# MCP Integration (Conceptual)

MCP is the runtime boundary between Clickweave and external automation capabilities.

## Role of MCP in the System

- Clickweave does not directly automate OS/browser surfaces.
- It delegates concrete operations to an MCP server.
- The executor stays focused on orchestration, retries, and state.

## Lifecycle Model

- Start MCP for planning tool discovery or for an execution run.
- Query available tools and schemas.
- Call tools as the workflow executes.
- Tear down MCP process when done.

## Design Benefits

- Backend stays provider-agnostic at the tool layer.
- Tool schemas can be exposed to LLMs for planning/agentic steps.
- Failures in external automation are isolated at a clear boundary.

For protocol and exact command behavior, see `docs/reference/mcp/integration.md`.
