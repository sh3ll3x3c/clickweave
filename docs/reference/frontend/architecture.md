# Frontend Architecture (Reference)

Verified at commit: `0e907fc`

The UI is a React 19 + Vite app using Zustand for app state and React Flow for graph editing.

## Stack

| Layer | Technology |
|------|------------|
| Framework | React 19 |
| Build | Vite 6 |
| Styling | Tailwind CSS v4 |
| Graph Editor | `@xyflow/react` |
| State | Zustand (slice composition) |
| Desktop bridge | Tauri v2 (`@tauri-apps/api`) |
| Types/commands | generated `ui/src/bindings.ts` (Specta/tauri-specta) |
| Tests | Vitest + Testing Library |

## Directory Structure

```
ui/src/
├── App.tsx
├── main.tsx
├── bindings.ts
├── components/
│   ├── GraphCanvas.tsx
│   ├── WorkflowNode.tsx
│   ├── LoopGroupNode.tsx
│   ├── NodePalette.tsx
│   ├── AssistantPanel.tsx
│   ├── LogsDrawer.tsx
│   ├── FloatingToolbar.tsx
│   ├── Header.tsx
│   ├── Sidebar.tsx
│   ├── VerdictBar.tsx
│   ├── SettingsModal.tsx
│   └── node-detail/
│       ├── NodeDetailModal.tsx
│       └── tabs/
│           ├── SetupTab.tsx
│           ├── TraceTab.tsx
│           ├── ChecksTab.tsx
│           └── RunsTab.tsx
├── store/
│   ├── useAppStore.ts
│   ├── state.ts
│   ├── settings.ts
│   └── slices/
│       ├── projectSlice.ts
│       ├── executionSlice.ts
│       ├── assistantSlice.ts
│       ├── settingsSlice.ts
│       ├── logSlice.ts
│       ├── verdictSlice.ts
│       ├── uiSlice.ts
│       └── types.ts
└── utils/
```

## State Model

`StoreState` is the intersection of 7 slices:

- `ProjectSlice`
- `ExecutionSlice`
- `AssistantSlice`
- `SettingsSlice`
- `LogSlice`
- `VerdictSlice`
- `UiSlice`

Type is defined in `ui/src/store/slices/types.ts` and store composition in `ui/src/store/useAppStore.ts`.

### Slice Summary

**ProjectSlice** (`projectSlice.ts`)

- `workflow`, `projectPath`, `isNewWorkflow`
- actions: `openProject`, `saveProject`, `newProject`, `setWorkflow`, `skipIntentEntry`

**ExecutionSlice** (`executionSlice.ts`)

- `executorState: "idle" | "running"`
- actions: `setExecutorState`, `runWorkflow`, `stopWorkflow`

**AssistantSlice** (`assistantSlice.ts`)

- `conversation`, `assistantOpen`, `assistantLoading`, `assistantError`
- `pendingPatch`, `pendingPatchWarnings`
- actions: `sendAssistantMessage`, `resendMessage`, `applyPendingPatch`, `discardPendingPatch`, `cancelAssistantChat`, `clearConversation`

**SettingsSlice** (`settingsSlice.ts`)

- `plannerConfig`, `agentConfig`, `vlmConfig`, `vlmEnabled`, `mcpCommand`, `maxRepairAttempts`
- persistence via `store/settings.ts` (`settings.json` through Tauri plugin-store)

**UiSlice** (`uiSlice.ts`)

- selection/panel state (`selectedNode`, `detailTab`, drawer/modal flags)
- feature toggles: `allowAiTransforms`, `allowAgentSteps`
- node type metadata (`nodeTypes`) loaded from backend

**LogSlice / VerdictSlice**

- log buffer and check verdict state used by Logs drawer and Verdict bar

## App Event Wiring

`ui/src/App.tsx` subscribes to backend events:

- `executor://log`
- `executor://state`
- `executor://node_started`
- `executor://node_completed`
- `executor://node_failed`
- `executor://checks_completed`
- `executor://workflow_completed`

It also listens to menu events (`menu://new`, `menu://open`, etc.) and maps them to store actions.

## Graph Editor (`GraphCanvas`)

`GraphCanvas.tsx` wraps React Flow and maps workflow nodes/edges into RF nodes/edges.

### Node type keys

Registered node types:

- `workflow` -> `WorkflowNode`
- `loopGroup` -> `LoopGroupNode`

### Behavior

- Palette click adds a node (not drag-and-drop)
- Handle-to-handle connect creates edges
- Delete key removes selected nodes/edges
- Node selection drives detail modal visibility
- Loop groups support collapsed/expanded rendering and child containment

Control-flow edge labels shown in canvas:

- `IfTrue`, `IfFalse`
- `SwitchCase(name)`, `SwitchDefault`
- `LoopBody`, `LoopDone`

## Node Detail Modal

`NodeDetailModal` has 4 tabs:

- `Setup`: node params, enabled flag, timeout, settle delay, retries, trace level, expected outcome
- `Trace`: trace events + artifact preview/lightbox for selected run
- `Checks`: check definitions and `on_fail` policy
- `Runs`: run history list (can jump to Trace tab)

## Settings Defaults

From `ui/src/store/state.ts` and `settings.ts`:

- endpoint default: `http://localhost:1234/v1`, model `local`, empty API key
- `vlmEnabled`: `false`
- `mcpCommand`: `"npx"`
- `maxRepairAttempts`: `3`

`maxRepairAttempts` is clamped to `0..10` in `settingsSlice.ts`.

## Generated Bindings

`ui/src/bindings.ts` is generated in debug mode from Rust command/type definitions.

Contains:

- `commands.*` typed Tauri wrappers
- mirrored Rust types/unions
- command result wrappers

Do not edit manually.

## Key Files

| File | Role |
|------|------|
| `ui/src/App.tsx` | top-level layout and event listeners |
| `ui/src/components/GraphCanvas.tsx` | React Flow graph editor |
| `ui/src/components/WorkflowNode.tsx` | standard node renderer |
| `ui/src/components/LoopGroupNode.tsx` | expanded loop group renderer |
| `ui/src/components/node-detail/NodeDetailModal.tsx` | node detail shell |
| `ui/src/components/node-detail/tabs/TraceTab.tsx` | trace + artifact viewer |
| `ui/src/store/useAppStore.ts` | composed Zustand store hook |
| `ui/src/store/slices/types.ts` | `StoreState` composition |
| `ui/src/store/settings.ts` | persisted settings I/O |
