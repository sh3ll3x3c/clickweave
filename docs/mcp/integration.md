# MCP Integration

Clickweave communicates with desktop automation tools via the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/specification). The MCP server is spawned as a subprocess and communicated with over JSON-RPC via stdio.

## Architecture

```
┌──────────────────┐     JSON-RPC (stdio)     ┌───────────────────┐
│  clickweave-mcp  │ ◄─────────────────────► │ native-devtools   │
│  McpClient       │                          │ MCP server        │
└──────────────────┘                          └───────────────────┘
        ▲                                              │
        │                                     Desktop automation
        │                                     (clicks, screenshots,
  clickweave-engine                            OCR, window mgmt)
```

## McpClient

**File:** `crates/clickweave-mcp/src/client.rs`

The `McpClient` manages the MCP server subprocess lifecycle and provides a high-level API for tool discovery and invocation.

### Spawning

Two spawn modes:

| Method | Used When | What It Does |
|--------|-----------|--------------|
| `McpClient::spawn_npx()` | `mcp_command == "npx"` | Runs `npx -y @anthropic/native-devtools-mcp` |
| `McpClient::spawn(cmd, args)` | Custom binary path | Runs the specified binary |

The spawn sequence:
1. Start the subprocess with stdio piped
2. Send `initialize` JSON-RPC request
3. Wait for `initialize` response (server capabilities)
4. Send `initialized` notification
5. Send `tools/list` request to discover available tools
6. Cache the tool list

### Tool Discovery

On initialization, the client fetches the complete tool list from the server. Tools are stored in two formats:

- **MCP format:** Raw tool schemas as returned by the server
- **OpenAI format:** Converted to OpenAI function-calling format for LLM consumption

The conversion (`tools_as_openai()`) wraps each tool in the standard OpenAI structure:

```json
{
  "type": "function",
  "function": {
    "name": "click",
    "description": "Click at screen coordinates...",
    "parameters": { ... }
  }
}
```

### Tool Invocation

```rust
mcp.call_tool(name: &str, arguments: Option<Value>) -> Result<ToolResult>
```

Sends a `tools/call` JSON-RPC request and returns the result. The `ToolResult` contains an array of content items:

| Content Type | Fields |
|-------------|--------|
| `text` | `text: String` |
| `image` | `data: String` (base64), `mimeType: String` |

### Lifecycle

The MCP server lives for the duration of a single workflow execution. It is spawned at the start of `WorkflowExecutor::run()` and dropped (subprocess killed) when the executor finishes or encounters a fatal error.

## Protocol Layer

**File:** `crates/clickweave-mcp/src/protocol.rs`

Implements the JSON-RPC 2.0 message format used by MCP:

### Message Types

| Type | Direction | Purpose |
|------|-----------|---------|
| `JsonRpcRequest` | Client → Server | Method call with optional params |
| `JsonRpcResponse` | Server → Client | Success result or error |
| `JsonRpcNotification` | Client → Server | Fire-and-forget (e.g., `initialized`) |

### Request/Response Flow

```
Client                              Server
  │                                    │
  │── initialize ────────────────────►│
  │◄── {capabilities, serverInfo} ────│
  │                                    │
  │── initialized (notification) ────►│
  │                                    │
  │── tools/list ────────────────────►│
  │◄── {tools: [...]} ───────────────│
  │                                    │
  │── tools/call {name, arguments} ──►│
  │◄── {content: [{type, ...}]} ─────│
  │                                    │
  │  (repeat tools/call as needed)     │
  │                                    │
  │── (drop / kill subprocess) ──────►│
```

## Tool Mapping

**File:** `crates/clickweave-core/src/tool_mapping.rs`

Bidirectional conversion between Clickweave's `NodeType` enum and MCP tool invocations:

### NodeType → Tool Invocation (Execution)

`node_type_to_tool_invocation(node_type)` converts a typed node into an MCP tool call:

| NodeType | Tool Name | Key Arguments |
|----------|-----------|---------------|
| `TakeScreenshot` | `take_screenshot` | `mode`, `app_name`, `include_ocr` |
| `FindText` | `find_text` | `text` |
| `FindImage` | `find_image` | `template_image_base64`, `threshold`, `max_results` |
| `Click` | `click` | `x`, `y`, `button`, `click_count` |
| `TypeText` | `type_text` | `text` |
| `PressKey` | `press_key` | `key`, `modifiers` |
| `Scroll` | `scroll` | `delta_y`, `x`, `y` |
| `ListWindows` | `list_windows` | `app_name` |
| `FocusWindow` | `focus_window` | `app_name` / `window_id` / `pid` |
| `McpToolCall` | *(dynamic)* | *(pass-through)* |

`AiStep`, `AppDebugKitOp`, `If`, `Switch`, `Loop`, and `EndLoop` return `Err(NotAToolNode)` — they are not direct MCP calls.

### Tool Invocation → NodeType (Planning)

`tool_invocation_to_node_type(name, args, known_tools)` converts an MCP tool call back to a typed node:

1. Known tool names (take_screenshot, click, etc.) map to specific `NodeType` variants
2. Unknown names that exist in the `known_tools` list map to `McpToolCall`
3. Completely unknown names return `Err(UnknownTool)`

This is used during planning to convert LLM-generated tool calls into typed workflow nodes.

## Configuration

The MCP command is configurable in the UI (Settings panel):

| Setting | Default | Description |
|---------|---------|-------------|
| `mcpCommand` | `"npx"` | How to start the MCP server. Set to `"npx"` for npx-based launch, or a binary path for direct execution. |

When set to `"npx"`, the server is started with `npx -y @anthropic/native-devtools-mcp`. Otherwise, the configured path is executed directly with no arguments.

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-mcp/src/client.rs` | `McpClient` — spawn, initialize, call tools |
| `crates/clickweave-mcp/src/protocol.rs` | JSON-RPC message types |
| `crates/clickweave-mcp/src/lib.rs` | Re-exports |
| `crates/clickweave-core/src/tool_mapping.rs` | NodeType ↔ tool invocation conversion |
