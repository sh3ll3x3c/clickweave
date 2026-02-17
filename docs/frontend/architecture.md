# Frontend Architecture

The Clickweave frontend is a React 19 application built with Vite, Tailwind CSS v4, and React Flow for the node graph editor. State management uses Zustand with a slice-based architecture.

## Stack

| Layer | Technology |
|-------|------------|
| Framework | React 19 |
| Build | Vite |
| Styling | Tailwind CSS v4 |
| Graph Editor | React Flow (`@xyflow/react`) |
| State | Zustand (slice pattern) |
| Type Safety | Auto-generated bindings via specta/tauri-specta |
| Testing | Vitest + Testing Library |
| Desktop Bridge | Tauri v2 (`@tauri-apps/api`) |

## Directory Structure

```
ui/src/
├── App.tsx                  # Root component, layout, event listeners
├── main.tsx                 # Entry point
├── bindings.ts              # Auto-generated Tauri commands + types
├── index.css                # Tailwind imports
├── components/
│   ├── Canvas.tsx           # React Flow canvas wrapper
│   ├── NodePalette.tsx      # Draggable node type picker
│   ├── AssistantPanel.tsx   # Chat interface for AI assistant
│   ├── LogPanel.tsx         # Execution log viewer
│   ├── RunPanel.tsx         # Run history viewer
│   ├── VerdictBar.tsx       # Check results banner
│   ├── SettingsModal.tsx    # Settings dialog
│   ├── Toolbar.tsx          # Top bar with actions
│   ├── nodes/               # Custom React Flow node components
│   │   ├── WorkflowNode.tsx # Standard node renderer
│   │   └── GroupNode.tsx    # Loop/group container node
│   └── node-detail/         # Node detail modal
│       ├── NodeDetailModal.tsx
│       └── tabs/            # Setup, Checks, Runs tabs
├── store/
│   ├── useAppStore.ts       # Main Zustand store
│   ├── state.ts             # State type definitions, defaults
│   ├── settings.ts          # Settings persistence
│   └── slices/
│       ├── types.ts         # StoreState = intersection of all slices
│       ├── projectSlice.ts  # Workflow, nodes, edges, file I/O
│       ├── executionSlice.ts # Run state, node status
│       ├── assistantSlice.ts # Chat messages, patch management
│       ├── settingsSlice.ts  # Endpoint configs, feature flags
│       ├── logSlice.ts       # Execution log entries
│       ├── verdictSlice.ts   # Check verdicts
│       └── uiSlice.ts        # Modal/panel visibility, selection
└── utils/                   # Shared utilities
```

## State Management

### Zustand Store

The app uses a single Zustand store composed from slices:

```typescript
type StoreState = AssistantSlice &
  ExecutionSlice &
  LogSlice &
  ProjectSlice &
  SettingsSlice &
  UiSlice &
  VerdictSlice;
```

Each slice manages a domain of state and its associated actions. Slices can read from each other via `get()` since they share a single store.

### Key Slices

**ProjectSlice** — The workflow model
- `workflow: Workflow` — current workflow (nodes + edges)
- `projectPath: string | null` — file path for saved projects
- `isDirty: boolean` — unsaved changes flag
- Actions: `addNode()`, `removeNode()`, `updateNode()`, `addEdge()`, `removeEdge()`, `openProject()`, `saveProject()`

**ExecutionSlice** — Runtime state
- `executorState: "idle" | "running"` — current executor state
- `activeNodeId: string | null` — currently executing node
- `nodeRuns: Map<string, NodeRun[]>` — run records per node
- Actions: `runWorkflow()`, `stopWorkflow()`

**AssistantSlice** — Chat interface
- `messages: ChatEntry[]` — conversation history
- `pendingPatch: WorkflowPatch | null` — unapplied patch from assistant
- `isAssistantLoading: boolean` — request in progress
- Actions: `sendMessage()`, `applyPendingPatch()`, `rejectPendingPatch()`, `cancelAssistantChat()`

**SettingsSlice** — Configuration
- `plannerConfig`, `agentConfig`, `vlmConfig: EndpointConfig` — LLM endpoints
- `vlmEnabled: boolean`
- `mcpCommand: string`
- `allowAiTransforms`, `allowAgentSteps: boolean` — feature flags
- `maxRepairAttempts: number` — validation retry limit

**VerdictSlice** — Check results
- `verdicts: NodeVerdict[]` — per-node check results
- `verdictState: "idle" | "passed" | "warned" | "failed"`

## Event Handling

The frontend subscribes to backend events in `App.tsx` using Tauri's event API:

```typescript
import { listen } from "@tauri-apps/api/event";

listen("executor://log", (event) => { ... });
listen("executor://state", (event) => { ... });
listen("executor://node_started", (event) => { ... });
// etc.
```

Events update the Zustand store, which triggers React re-renders.

## React Flow Integration

The workflow canvas uses React Flow (`@xyflow/react`) with custom node types:

### Node Types

| Type Key | Component | Used For |
|----------|-----------|----------|
| `workflowNode` | `WorkflowNode` | All standard nodes (Click, FindText, AiStep, etc.) |
| `groupNode` | `GroupNode` | Loop groups (visual container) |

Nodes are registered via the `nodeTypes` prop on the `<ReactFlow>` component.

### Node Rendering

`WorkflowNode` renders each node with:
- Category icon and color
- Node name
- Status indicator (idle, running, completed, failed)
- Handles for edge connections
- Double-click opens `NodeDetailModal`

### Edge Rendering

Edges show labeled connections. Control flow edges (`IfTrue`, `IfFalse`, `LoopBody`, `LoopDone`, `SwitchCase`) display their label on the edge.

### Canvas Interactions

- **Drag from palette:** Creates a new node at the drop position
- **Connect handles:** Creates an edge between nodes
- **Select + Delete:** Removes nodes or edges
- **Double-click node:** Opens the detail modal

## Node Detail Modal

The `NodeDetailModal` provides tabbed configuration for each node:

### Setup Tab
- Node name
- Node-type-specific parameters (e.g., click coordinates, text to type, AI prompt)
- Retries (0-10)
- Timeout (ms)
- Trace level (Off / Minimal / Full)
- Expected outcome (free text)
- Enabled toggle

### Checks Tab
- Add/remove typed checks (TextPresent, TextAbsent, TemplateFound, WindowTitleMatches)
- Configure check parameters
- Set on_fail policy (FailNode / WarnOnly)

### Runs Tab
- Historical run list for the node
- Trace event viewer
- Artifact previewer (screenshots, OCR results)

## Settings

Settings are persisted using Tauri's `plugin-store` in `settings.json`:

| Setting | Default | Description |
|---------|---------|-------------|
| `plannerConfig` | localhost:1234 | LLM endpoint for planning |
| `agentConfig` | localhost:1234 | LLM endpoint for AI step execution |
| `vlmConfig` | localhost:1234 | LLM endpoint for vision analysis |
| `vlmEnabled` | false | Whether to use a separate VLM |
| `mcpCommand` | "npx" | MCP server launch command |
| `maxRepairAttempts` | 3 | Validation retry limit for assistant |

All endpoints use the OpenAI-compatible `/v1/chat/completions` format.

## Auto-Generated Bindings

TypeScript types and command wrappers are generated from Rust types:

**File:** `ui/src/bindings.ts`

This file is auto-generated by tauri-specta and should not be edited manually. It contains:
- `commands` object with typed async functions for each Tauri command
- TypeScript type definitions mirroring Rust structs and enums
- `Result<T, E>` type for fallible commands

Generation happens automatically when the app runs in debug mode (`cargo tauri dev`).

## Key Files

| File | Role |
|------|------|
| `ui/src/App.tsx` | Root component, event listeners, layout |
| `ui/src/bindings.ts` | Auto-generated Tauri types + commands |
| `ui/src/store/useAppStore.ts` | Main Zustand store |
| `ui/src/store/slices/types.ts` | Store type composition |
| `ui/src/components/Canvas.tsx` | React Flow canvas |
| `ui/src/components/AssistantPanel.tsx` | AI chat interface |
| `ui/src/components/nodes/WorkflowNode.tsx` | Custom node renderer |
| `ui/src/components/node-detail/NodeDetailModal.tsx` | Node configuration modal |
| `ui/src/store/settings.ts` | Settings persistence |
