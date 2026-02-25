# Frontend Architecture (Reference)

Verified at commit: `f8e0d5b`

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
│   ├── SupervisionModal.tsx
│   └── node-detail/
│       ├── NodeDetailModal.tsx
│       └── tabs/
│           ├── SetupTab.tsx
│           ├── TraceTab.tsx
│           ├── ChecksTab.tsx
│           └── RunsTab.tsx
├── hooks/
│   ├── useEscapeKey.ts
│   ├── useUndoRedoKeyboard.ts
│   ├── useLoopGrouping.ts
│   ├── useNodeSync.ts
│   ├── useEdgeSync.ts
│   ├── useWorkflowActions.ts
│   └── test-helpers.ts
├── store/
│   ├── useAppStore.ts
│   ├── useWorkflowMutations.ts
│   ├── state.ts
│   ├── settings.ts
│   └── slices/
│       ├── projectSlice.ts
│       ├── executionSlice.ts
│       ├── assistantSlice.ts
│       ├── historySlice.ts
│       ├── settingsSlice.ts
│       ├── logSlice.ts
│       ├── verdictSlice.ts
│       ├── uiSlice.ts
│       └── types.ts
└── utils/
```

## State Model

`StoreState` is the intersection of 8 slices:

- `ProjectSlice`
- `ExecutionSlice`
- `AssistantSlice`
- `HistorySlice`
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

- `executorState: "idle" | "running"`, `executionMode: ExecutionMode`, `supervisionPause: SupervisionPause | null`
- actions: `setExecutorState`, `setExecutionMode`, `setSupervisionPause`, `clearSupervisionPause`, `supervisionRespond`, `runWorkflow`, `stopWorkflow`

**AssistantSlice** (`assistantSlice.ts`)

- `conversation`, `assistantOpen`, `assistantLoading`, `assistantRetrying`, `assistantError`
- `pendingPatch`, `pendingPatchWarnings`
- actions: `sendAssistantMessage`, `resendMessage`, `applyPendingPatch`, `discardPendingPatch`, `cancelAssistantChat`, `clearConversation`

**SettingsSlice** (`settingsSlice.ts`)

- `plannerConfig`, `agentConfig`, `vlmConfig`, `vlmEnabled`, `mcpCommand`, `maxRepairAttempts`
- persistence via `store/settings.ts` (`settings.json` through Tauri plugin-store)

**UiSlice** (`uiSlice.ts`)

- selection/panel state (`selectedNode`, `detailTab`, drawer/modal flags)
- feature toggles: `allowAiTransforms`, `allowAgentSteps`
- node type metadata (`nodeTypes`) loaded from backend

**HistorySlice** (`historySlice.ts`)

- `past: HistoryEntry[]`, `future: HistoryEntry[]` — undo/redo stacks (max 50 entries)
- actions: `pushHistory`, `undo`, `redo`, `clearHistory`
- Workflow mutations push snapshots via `useWorkflowMutations` before each change

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
- `executor://supervision_passed`
- `executor://supervision_paused`
- `assistant://repairing`

It also listens to menu events (`menu://new`, `menu://open`, etc.) and maps them to store actions.

## Graph Editor (`GraphCanvas`)

`GraphCanvas.tsx` is a thin composition shell that delegates to three hooks:

- `useLoopGrouping` — loop collapse state, hidden node tracking
- `useNodeSync` — RF node state, position tracking, selection sync
- `useEdgeSync` — RF edge filtering, change handling, connect

### Node type keys

Registered node types:

- `workflow` -> `WorkflowNode`
- `loopGroup` -> `LoopGroupNode`

### Behavior

- Palette click adds a node (not drag-and-drop)
- Handle-to-handle connect creates edges
- Delete key removes selected nodes/edges (multi-select supported; independently selected edges are removed silently via `removeEdgesOnly` without a separate history entry)
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

- `commands.*` typed Tauri wrappers (including `commands.supervisionRespond(action)` for resuming a paused supervision check)
- mirrored Rust types/unions
- command result wrappers

Notable types:

- `ExecutionMode` — `"Test" | "Run"`, selects whether the executor runs in supervised test mode or unattended run mode
- `SupervisionPause` — `{ nodeId, nodeName, finding, screenshot }`, defined in `executionSlice.ts`; represents a paused supervision check awaiting user decision

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
| `ui/src/store/useWorkflowMutations.ts` | node/edge mutation helpers with history push (`removeEdgesOnly` for silent edge removal) |
| `ui/src/store/slices/types.ts` | `StoreState` composition |
| `ui/src/store/slices/historySlice.ts` | undo/redo state and actions |
| `ui/src/store/settings.ts` | persisted settings I/O |
| `ui/src/components/SupervisionModal.tsx` | supervision pause modal (retry / skip / abort) |
| `ui/src/hooks/useLoopGrouping.ts` | loop collapse state, hidden node tracking |
| `ui/src/hooks/useNodeSync.ts` | RF node state, position tracking, selection sync |
| `ui/src/hooks/useEdgeSync.ts` | RF edge filtering, change handling |
| `ui/src/hooks/useWorkflowActions.ts` | workflow mutation dispatchers (wraps `useWorkflowMutations`) |
| `ui/src/hooks/useEscapeKey.ts` | global Escape key handler that closes panels in priority order |
| `ui/src/hooks/useUndoRedoKeyboard.ts` | Ctrl+Z / Ctrl+Shift+Z keyboard binding |
