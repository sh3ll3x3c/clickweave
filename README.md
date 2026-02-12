# Clickweave

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)
![Tauri](https://img.shields.io/badge/tauri-v2-blueviolet.svg)
![React](https://img.shields.io/badge/react-19-61dafb.svg)

Clickweave is an open-source desktop automation platform and computer use agent (CUA) built with Rust, Tauri, and React. It combines a visual workflow builder with AI-powered planning and execution, so you can automate repetitive UI tasks with drag-and-drop nodes or natural-language instructions.

Related terms people use to find tools like this: desktop automation, UI automation, RPA, and agentic workflow automation.

![Clickweave Banner](assets/banner.svg)

## Project Status

Clickweave is in **very early WIP**. Expect rapid changes, incomplete features, and breaking updates while core functionality is still being built.


![Clickweave desktop automation workflow builder screenshot](assets/app_screenshot.png)

## Features

### Visual Workflow Builder
An intuitive node-based interface (React Flow) for designing automation flows. Drag nodes from a categorized palette, connect them into linear chains, and configure each step through a detail modal.

### AI Workflow Planning
Describe your intent in natural language and Clickweave generates a complete workflow. The planner:
- Converts intent into a sequence of typed nodes using available MCP tools
- Generates deterministic **Tool** steps by default and optional **AiStep** nodes for agentic execution
- Planner output schema includes **Tool**, **AiStep**, and **AiTransform** formats (AiTransform runtime behavior is still evolving)
- Auto-repairs malformed LLM output with one-shot retry and error feedback
- Validates generated workflows against structural rules

### AI Workflow Patching
Modify existing workflows conversationally. Describe what to change and the assistant adds, removes, or updates nodes while preserving the rest of the workflow structure.

### Computer Use Agent (CUA)
"See" and "act" on the desktop via [`native-devtools-mcp`](https://github.com/sh3ll3x3c/native-devtools-mcp):
Clickweave’s protocol layer is based on the [Model Context Protocol (MCP) specification](https://modelcontextprotocol.io/specification).
- **Visual Perception:** Screenshots, OCR text detection, and image template matching.
- **Interaction:** Mouse clicks, keyboard input, scrolling, and window management.
- **AppDebugKit (experimental):** `AppDebugKitOp` nodes exist in the model and UI, with runtime support still in progress.

### Pluggable AI Backends
Three independently configurable endpoints (configurable via **Settings** in the UI):
- **Planner** — LLM for workflow generation and patching
- **Agent** — LLM for AI step execution during workflow runs
- **VLM** (optional) — Separate vision model for image analysis

All endpoints use the OpenAI-compatible `/v1/chat/completions` format. Works with local servers (LM Studio, vLLM, Ollama) or hosted providers (OpenRouter, OpenAI). No API key required for local endpoints.

### Node Configuration
Each node supports:
- **Retries** (UI range: 0–10) with automatic re-execution on failure
- **Timeout** (ms) for bounded execution
- **Trace level** (Off / Minimal / Full). Currently, `Off` disables artifact capture; `Minimal` and `Full` both capture traces/artifacts.
- **Expected outcome** — human-readable description of what the node should achieve
- **Enabled toggle** — skip nodes without removing them

### Checks (Workflow Metadata)
Attach check definitions to any node:
- **TextPresent / TextAbsent**, **TemplateFound**, **WindowTitleMatches**
- Check behavior policy can be set to **FailNode** or **WarnOnly**
- Checks are stored with workflow data and editable in the UI; runtime check enforcement is still in progress

### Run History & Tracing
Every node execution is persisted with full traceability:
```
.clickweave/runs/<workflow_id>/<node_id>/<run_id>/
  run.json           # Execution metadata and status
  events.jsonl       # Newline-delimited trace events
  artifacts/         # Screenshots, OCR results, template matches
```
Browse past runs in the UI, inspect trace events, and preview captured artifacts.

### Feature Flags
Control what the planner is allowed to generate (toggle in **Settings**):
- **Allow AI Transforms** (default: on) — enables AiTransform planner schema output (runtime semantics are evolving)
- **Allow Agent Steps** (default: off) — full agentic loops with tool access

### Local-First
Desktop tool actions run locally. LLM/VLM requests go only to the endpoints you configure (defaults to `localhost`).

### Cross-Platform
Built with Tauri v2, producing lightweight native applications for macOS, Windows, and Linux.

## Architecture

Clickweave is a hybrid application combining a Rust backend with a React frontend via Tauri.

### Project Structure

```
/
├── crates/                 # Rust backend workspace
│   ├── clickweave-core/    # Shared types, validation, storage
│   ├── clickweave-engine/  # Workflow execution engine
│   ├── clickweave-llm/     # LLM client & planner logic
│   └── clickweave-mcp/     # MCP client & protocol implementation
├── src-tauri/              # Tauri application shell & commands
├── ui/                     # React frontend (Vite + Tailwind)
│   ├── src/
│   │   ├── components/     # UI components (Graph, Nodes, Modals)
│   │   └── store/          # State management and app actions
└── assets/                 # Static assets
```

### Backend (`/crates` & `/src-tauri`)
- **`clickweave-core`** — Shared types, workflow validation, run storage, and bidirectional tool mapping (NodeType <-> MCP tool invocations).
- **`clickweave-engine`** — Workflow executor. Walks nodes linearly, dispatches deterministic tool calls or AI agentic loops, manages retries/timeouts, streams trace events and artifacts.
- **`clickweave-llm`** — OpenAI-compatible client and the planner module (plan generation, workflow patching, prompt engineering, JSON parsing with auto-repair).
- **`clickweave-mcp`** — MCP client that spawns [`native-devtools-mcp`](https://github.com/sh3ll3x3c/native-devtools-mcp) as a subprocess, converts MCP tool schemas to OpenAI format, and dispatches tool calls via JSON-RPC.
- **`src-tauri`** — Tauri application shell. Exposes commands to the frontend (plan, patch, run, stop, project I/O, run history) and manages event streaming.

### Frontend (`/ui`)
- **Framework:** React 19 + Vite
- **Styling:** Tailwind CSS v4
- **Graph Editor:** React Flow (`@xyflow/react`)
- **Testing:** Vitest + Testing Library
- **Type Safety:** Auto-generated TypeScript bindings from Rust types via specta

### Node Types

| Category | Node Type | Description |
|----------|-----------|-------------|
| AI | AiStep | Agentic loop with configurable tool access and max tool calls |
| Vision | TakeScreenshot | Capture screen/window/region with optional OCR |
| Vision | FindText | OCR-based text search (Contains or Exact match) |
| Vision | FindImage | Template matching with threshold and max results |
| Input | Click | Mouse click at coordinates (left/right/center, single/double) |
| Input | TypeText | Keyboard text input |
| Input | PressKey | Key press with modifiers (shift, control, option, command) |
| Input | Scroll | Scroll at position with delta |
| Window | ListWindows | Enumerate visible windows |
| Window | FocusWindow | Bring window to front by app name, window ID, or PID |
| AppDebugKit / Extensibility | McpToolCall | Generic invocation of any MCP tool by name |
| AppDebugKit | AppDebugKitOp | Experimental app debug operation node (runtime support is in progress) |

## Use Cases

- **Desktop RPA:** Automate repetitive back-office UI workflows across multiple desktop apps.
- **QA Automation:** Run smoke/regression flows with screenshots, OCR, and run traces for debugging.
- **AI-Assisted Operations:** Let AI plan or patch workflows, then keep deterministic execution where possible.
- **Local-First Automation:** Use local LLM/VLM endpoints for privacy-sensitive workflows.

## FAQ

### What is Clickweave?
Clickweave is a desktop automation and computer-use agent platform with a visual workflow editor and AI-assisted planning/execution.

### Does Clickweave require coding?
No. You can build workflows visually and optionally use natural-language prompts for planning and patching.

### Which model providers are supported?
OpenAI-compatible providers/endpoints such as LM Studio, vLLM, Ollama (OpenAI-compatible mode), OpenRouter, and OpenAI.

### Is Clickweave local-first?
Yes. Desktop actions run locally. LLM/VLM calls are sent only to endpoints you configure.

### Are node checks enforced at runtime today?
Check definitions are currently stored and editable in the UI/workflow model. Runtime check enforcement is still under active development.

## For AI Agents

This section is designed to help AI agents navigate and understand the codebase.

**Key Entry Points:**
- **Planner Logic:** `crates/clickweave-llm/src/planner/plan.rs` - See `plan_workflow` / `plan_workflow_with_backend`.
- **Patch Logic:** `crates/clickweave-llm/src/planner/patch.rs` - See workflow patch generation/application flow.
- **Execution Loop:** `crates/clickweave-engine/src/executor/run_loop.rs` - The core `run` executor loop.
- **MCP Protocol:** `crates/clickweave-mcp/src/protocol.rs` - JSON-RPC implementation for tool communication.
- **MCP Specification:** [modelcontextprotocol.io/specification](https://modelcontextprotocol.io/specification) - Canonical protocol reference.
- **Frontend State:** `ui/src/store/useAppStore.ts` - Main Zustand store for application state.
- **Tauri Commands:** `src-tauri/src/commands/` - Bridge between frontend and backend.

**Conventions:**
- **Error Handling:** Internal crates primarily use `anyhow::Result`; Tauri command boundaries often return `Result<_, String>`.
- **Async:** Heavily relies on `tokio` for async runtime.
- **Tracing:** Uses `tracing` crate for structured logging.

## Getting Started

### Prerequisites

1. **Rust**: [Install Rust](https://www.rust-lang.org/tools/install) (stable, `>= 1.85`).
2. **Node.js**: [Install Node.js](https://nodejs.org/) (LTS recommended).
3. **Tauri CLI**:
    ```bash
    cargo install tauri-cli --locked
    ```
4. **OS Dependencies**:
    - **macOS**: Xcode Command Line Tools.
        ```bash
        xcode-select --install
        ```
    - **Windows**: [Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
    - **Linux** (Ubuntu/Debian):
        ```bash
        sudo apt-get update
        sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
        ```

### Installation & Running

1. **Clone the repository:**
    ```bash
    git clone https://github.com/sh3ll3x3c/clickweave.git
    cd clickweave
    ```

2. **Install frontend dependencies:**
    ```bash
    npm install --prefix ui
    ```

3. **Run in development mode:**
    ```bash
    cargo tauri dev
    ```

4. **Build for production:**
    ```bash
    cargo tauri build
    ```
    Output bundles will be in `target/release/bundle/`.

### Running Tests

```bash
# Rust tests
cargo test

# Frontend tests
npm test --prefix ui
```

## Model Info Detection

At workflow startup, Clickweave queries the inference provider for model metadata (context length, architecture, quantization). This is logged alongside per-request token usage to help diagnose context exhaustion issues.

Context length detection is **provider-dependent**:

| Provider | Context length field | Endpoint |
|----------|---------------------|----------|
| LM Studio | `max_context_length`, `loaded_context_length` | `/api/v0/models` |
| vLLM | `max_model_len` | `/v1/models` |
| OpenRouter | `context_length` | `/v1/models` |
| Ollama | Not supported yet | — |
| OpenAI | Not available via API | — |

If the provider does not return context length, Clickweave logs `ctx=?`. Token usage (`prompt_tokens`, `completion_tokens`, `total_tokens`) is always logged when the provider returns it.

## Logs

Clickweave writes JSON-formatted trace logs to a daily rolling file. These contain full LLM request/response bodies and tool call details, useful for diagnosing workflow failures.

| Platform | Location |
|----------|----------|
| macOS | `~/Library/Logs/Clickweave/` |
| Windows / Linux | `./logs/` (relative to the working directory) |

Log files are named `clickweave.YYYY-MM-DD.txt`. The console log level defaults to `info` and can be changed with the `RUST_LOG` environment variable. The file layer always captures at `trace` level.

## License

Distributed under the MIT License. See `LICENSE` for more information.
