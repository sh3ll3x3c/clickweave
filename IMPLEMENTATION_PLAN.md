# Clickweave - Implementation Plan

## Goals
- Build a simple, single-path workflow tool for UI computer use.
- Visual graph editor (n8n-like) where nodes define steps the LLM executes via MCP.
- Nodes support image upload (for image find/click) and button names (text find/click).
- OpenAI-compatible LLM API (chat/completions + tool-calling), non-streaming first.
- Cross-platform desktop app (macOS + Windows) using Rust + egui (eframe).
- The app spawns a native-devtools-mcp instance and kills it when done.

## Non-goals (initial scope)
- Branching/loops/conditions in the workflow (single linear path only).
- Multi-workflow scheduling or background workers.
- Streaming UI tokens (add later).
- Cloud storage or collaboration.

## Key assumptions
- The MCP tool set is provided by the local native-devtools-mcp server.
- The app can read and write a local project folder with assets.
- LLM supports OpenAI chat/completions tool-calling format.

## Deliverables
- New repo at /Users/x0/Work/clickweave with a Rust workspace.
- A desktop app crate that opens a graph editor and runs workflows.
- Local project files: workflow.json + assets/ for uploaded images.
- Working end-to-end: graph -> LLM tool calls -> MCP -> UI automation.

## Recommended repo structure
- clickweave/
  - Cargo.toml (workspace)
  - crates/
    - clickweave-app/ (egui/eframe UI)
    - clickweave-core/ (workflow model, execution engine)
    - clickweave-llm/ (OpenAI-compatible API client)
    - clickweave-mcp/ (MCP client and process manager)
  - projects/ (example project)
  - docs/ (optional)

## Architecture overview
- UI (egui) holds the graph editor + inspector + log console.
- Core holds workflow data model and validation.
- Engine executes linear steps, calling LLM and MCP in a tool loop.
- MCP client spawns and manages native-devtools-mcp process.
- LLM client sends chat/completions with tool schemas derived from MCP.

## 1) Initialize workspace and crates
- Create a workspace with the crates listed above.
- Add dependencies:
  - clickweave-app: eframe, egui, egui_extras (optional), rfd, image, tokio, crossbeam-channel.
  - clickweave-core: serde, serde_json, uuid, anyhow, thiserror.
  - clickweave-llm: reqwest, serde, serde_json, anyhow.
  - clickweave-mcp: serde_json, anyhow, tokio, tokio-util (codec), uuid.
- Configure common features: "serde", "tokio" as needed.

## 2) Data model (clickweave-core)
- Define Workflow:
  - id, name, version, nodes: Vec<Node>, edges: Vec<Edge>, start_node_id.
- Node:
  - id: Uuid
  - kind: NodeKind
  - position: { x, y }
  - name: String
  - params: NodeParams
- Edge:
  - from: NodeId, to: NodeId
- NodeKind (initial set): Start, Step, End.
- NodeParams:
  - prompt: String (LLM instruction for this step)
  - button_text: Option<String>
  - image_path: Option<String>
  - timeout_ms: Option<u64>
  - max_tool_calls: Option<u32>
- Validation rules:
  - Exactly one Start node.
  - Exactly one End node.
  - Single path: each node (except End) has max one outgoing edge.
  - Graph is connected from Start and terminates at End.

## 3) Project and assets
- Project folder layout:
  - project.json or workflow.json at root.
  - assets/ for uploaded images.
- Asset import:
  - Copy user-selected image into assets/ with unique name.
  - Store relative path in NodeParams.image_path.
- Ensure path handling works on macOS and Windows (use PathBuf).

## 4) Graph editor (clickweave-app)
- Use egui + a node graph library (egui_snarl or egui_node_graph).
- UI layout:
  - Top toolbar: New, Open, Save, Run, Stop, Step.
  - Left panel: node palette (Start, Step, End).
  - Center: graph canvas.
  - Right panel: node inspector (fields).
  - Bottom panel: log console.
- Node inspector fields:
  - Name
  - Prompt (multiline)
  - Button name (text input)
  - Image upload (rfd file picker) and preview
  - Timeout and max tool calls
- Image preview:
  - Load via image crate, convert to egui::ColorImage.
  - Cache textures to avoid reloading every frame.

## 5) MCP client and process manager (clickweave-mcp)
- Implement McpProcess:
  - Spawn native-devtools-mcp using Command.
  - Track Child handle and kill on drop or explicit stop.
  - Capture stdin/stdout for JSON-RPC.
- Implement McpClient:
  - JSON-RPC 2.0 messages over stdio.
  - Methods: list_tools, call_tool.
  - Deserialize tool schemas from MCP.
  - Use request id counter.
- Provide config:
  - Binary path (user input or default: "native-devtools-mcp" in PATH).
  - Args and env overrides.

## 6) LLM client (clickweave-llm)
- Implement OpenAI-compatible chat/completions:
  - base_url, api_key (optional), model, temperature, max_tokens.
  - tool calling: map MCP tool schemas to OpenAI "tools".
- Define messages:
  - system: describes tool usage + safety + step completion.
  - user: step prompt + button_text + image_path (if any).
- Parse tool calls from LLM response and send to MCP.

## 7) Execution engine (clickweave-core or new crate)
- Build linear execution order by walking edges from Start.
- For each node:
  - Build step context (prompt + params + last tool outputs).
  - Start a tool loop:
    - Send chat/completions with MCP tool schema.
    - If tool calls returned, execute via MCP, append tool results.
    - Stop when model returns a final assistant message OR max_tool_calls.
  - Record logs and per-node status.
- Provide explicit step completion rule:
  - Use a special "final" response marker, e.g. a JSON object:
    - {"step_complete": true, "summary": "..."}
  - Validate and move to next node.

## 8) UI run controls
- Add run state machine: Idle, Running, Paused, Stopped, Error.
- Provide Run, Stop, Step (advance one node), Resume buttons.
- Surface errors in bottom log and status bar.

## 9) Logging and debugging
- In-app log panel:
  - MCP requests/responses.
  - LLM requests/responses (redact API key).
  - Per-node start/finish timestamps.
- Optional log file to project folder.

## 10) Cross-platform considerations
- Use rfd for file dialogs on macOS/Windows.
- Avoid platform-specific window APIs.
- Make sure paths use PathBuf and display with to_string_lossy.

## 11) Testing plan
- Unit tests:
  - Workflow validation and path building.
  - Serialization round-trip.
- Integration test:
  - Mock MCP client (fake tool list + responses).
  - Mock LLM client (pre-canned tool calls).
- Manual smoke tests:
  - Load project, connect to MCP, run simple flow.

## 12) Milestones
1) Workspace + core data model + serialization.
2) Basic egui UI with node editing and save/load.
3) MCP process and client (list_tools, call_tool).
4) LLM client and tool mapping.
5) Execution engine and run controls.
6) End-to-end manual test with local OSS model.

## Notes for the implementing agent
- Verify tool names and schemas by calling MCP list_tools at runtime.
- When passing image_path to LLM, instruct it to call load_image/find_image.
- Enforce single-path graph validation to keep execution deterministic.
- Keep UI responsive by running LLM/MCP calls on a background thread.

