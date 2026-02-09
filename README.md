# Clickweave

Clickweave is an all-in-one desktop automation agent that combines a visual, low-code workflow builder with AI-powered steps (local or hosted). It enables complex UI automation (clicks, screenshots, OCR, visual matching) without writing code, while remaining configurable for power users.

![Clickweave Banner](assets/banner.svg)

## 🚀 Features

-   **Visual Workflow Builder:** An intuitive, n8n-like node interface for designing automation flows.
-   **Pluggable AI Backends:** Works with OpenAI-compatible `/v1/chat/completions` endpoints (local servers like LM Studio / vLLM, or hosted providers). No API key is required when using a local endpoint.
-   **Computer Use Agent (CUA):** Capable of "seeing" and "acting" on your desktop using the `native-devtools-mcp` protocol.
    -   📸 **Visual Perception:** Screenshots, OCR, and image template matching.
    -   🖱️ **Interaction:** Mouse clicks, keyboard input, and window management.
-   **Local-First:** Desktop tool actions run locally; LLM/VLM requests go only to the endpoint you configure (defaults to localhost).
-   **Cross-Platform:** Built with Tauri, creating a lightweight native application for macOS, Windows, and Linux.

## 🏗 Architecture

Clickweave is built as a hybrid application using **Tauri**, combining a performant Rust backend with a modern React frontend.

### Frontend (`/ui`)
-   **Framework:** React 19 + Vite
-   **Styling:** Tailwind CSS v4
-   **Visuals:** React Flow (`@xyflow/react`) for the node graph editor.

### Backend (`/crates` & `/src-tauri`)
The backend is modularized into several Rust crates:
-   **`clickweave-core`**: Shared types, validation logic, and storage primitives.
-   **`clickweave-engine`**: The workflow execution runtime. It orchestrates the flow between nodes.
-   **`clickweave-llm`**: OpenAI-compatible client for orchestrator + (optional) vision model endpoints.
-   **`clickweave-mcp`**: Implements the Model Context Protocol (MCP) to communicate with desktop tools.
-   **`src-tauri`**: The application shell that glues everything together and manages the native window.

## 🛠 Getting Started

### Prerequisites

To build and run Clickweave from source, you need the standard Tauri development environment set up.

1.  **Rust**: [Install Rust](https://www.rust-lang.org/tools/install) (stable, `>= 1.85`).
2.  **Node.js**: [Install Node.js](https://nodejs.org/) (LTS recommended).
3.  **Tauri CLI**: Install if you don't already have it.
    ```bash
    cargo install tauri-cli --locked
    ```
4.  **OS Dependencies**:
    -   **macOS**: Install Xcode Command Line Tools.
        ```bash
        xcode-select --install
        ```
    -   **Windows**: Install [Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
    -   **Linux**: Install system dependencies (Ubuntu/Debian example).
        ```bash
        sudo apt-get update
        sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
        ```

### Installation & Running

1.  **Clone the repository:**
    ```bash
    git clone <repo-url>
    cd clickweave
    ```

2.  **Install Frontend Dependencies:**
    ```bash
    npm install --prefix ui
    ```

3.  **Run in Development Mode:**
    This starts the frontend server and the Tauri application window.
    ```bash
    cargo tauri dev
    ```

4.  **Build for Production:**
    ```bash
    cargo tauri build
    ```
    The output bundles will be located in `target/release/bundle/`.

## Model Info Detection

At workflow startup, Clickweave queries the inference provider for model metadata (context length, architecture, quantization). This is logged alongside per-request token usage to help diagnose context exhaustion issues.

Context length detection is **provider-dependent** — not all providers expose this information:

| Provider | Context length field | Endpoint |
|----------|---------------------|----------|
| LM Studio | `max_context_length`, `loaded_context_length` | `/api/v0/models` |
| vLLM | `max_model_len` | `/v1/models` |
| OpenRouter | `context_length` | `/v1/models` |
| Ollama | Not supported yet | — |
| OpenAI | Not available via API | — |

If the provider does not return context length, Clickweave logs `ctx=?`. Token usage (`prompt_tokens`, `completion_tokens`, `total_tokens`) is always logged when the provider returns it (most do).

## Logs

Clickweave writes JSON-formatted trace logs to a daily rolling file. These contain full LLM request/response bodies and tool call details, useful for diagnosing workflow failures.

| Platform | Location |
|----------|----------|
| macOS | `~/Library/Logs/Clickweave/` |
| Windows / Linux | `./logs/` (relative to the working directory) |

Log files are named `clickweave.YYYY-MM-DD.txt`. The console log level defaults to `info` and can be changed with the `RUST_LOG` environment variable. The file layer always captures at `trace` level.

## 📄 License

Distributed under the MIT License. See `LICENSE` for more information.
