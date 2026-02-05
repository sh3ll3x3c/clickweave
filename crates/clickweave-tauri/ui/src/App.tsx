import { useAppStore } from "./store/useAppStore";
import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { NodePalette } from "./components/NodePalette";
import { LogsDrawer } from "./components/LogsDrawer";
import { FloatingToolbar } from "./components/FloatingToolbar";
import { SettingsModal } from "./components/SettingsModal";

function App() {
  const [state, actions] = useAppStore();

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--bg-dark)]">
      {/* Sidebar */}
      <Sidebar
        collapsed={state.sidebarCollapsed}
        onToggle={actions.toggleSidebar}
      />

      {/* Main content area */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Header */}
        <Header
          workflowName={state.workflow.name}
          projectPath={state.projectPath}
          executorState={state.executorState}
          onSave={actions.saveProject}
          onOpen={actions.openProject}
          onNew={actions.newProject}
          onSettings={() => actions.setShowSettings(true)}
          onNameChange={(name) =>
            actions.setWorkflow({ ...state.workflow, name })
          }
        />

        {/* Content row: graph + palette */}
        <div className="flex flex-1 overflow-hidden">
          {/* Graph canvas area (placeholder until M5) */}
          <div className="relative flex-1 overflow-hidden bg-[var(--bg-dark)]">
            {/* Canvas placeholder */}
            <div className="flex h-full items-center justify-center">
              <div className="text-center">
                <p className="text-sm text-[var(--text-muted)]">
                  {state.workflow.nodes.length === 0
                    ? "Add nodes from the palette to get started"
                    : `${state.workflow.nodes.length} node${state.workflow.nodes.length !== 1 ? "s" : ""} in workflow`}
                </p>
                {state.workflow.nodes.length > 0 && (
                  <div className="mt-4 flex flex-wrap justify-center gap-2">
                    {state.workflow.nodes.map((node) => (
                      <button
                        key={node.id}
                        onClick={() => actions.selectNode(node.id)}
                        className={`rounded-lg border px-3 py-2 text-xs transition-colors ${
                          state.selectedNode === node.id
                            ? "border-[var(--accent-coral)] bg-[var(--accent-coral)]/10 text-[var(--text-primary)]"
                            : "border-[var(--border)] bg-[var(--bg-panel)] text-[var(--text-secondary)] hover:border-[var(--text-muted)]"
                        }`}
                      >
                        {node.name}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* Floating toolbar */}
            <FloatingToolbar
              executorState={state.executorState}
              logsOpen={state.logsDrawerOpen}
              onToggleLogs={actions.toggleLogsDrawer}
              onRunStop={() => {
                /* M7: executor integration */
              }}
            />
          </div>

          {/* Node palette */}
          <NodePalette
            nodeTypes={state.nodeTypes}
            search={state.nodeSearch}
            onSearchChange={actions.setNodeSearch}
            onAdd={actions.addNode}
          />
        </div>

        {/* Logs drawer */}
        <LogsDrawer
          open={state.logsDrawerOpen}
          logs={state.logs}
          onToggle={actions.toggleLogsDrawer}
          onClear={actions.clearLogs}
        />
      </div>

      {/* Settings modal */}
      <SettingsModal
        open={state.showSettings}
        llmConfig={state.llmConfig}
        mcpCommand={state.mcpCommand}
        onClose={() => actions.setShowSettings(false)}
        onLlmConfigChange={actions.setLlmConfig}
        onMcpCommandChange={actions.setMcpCommand}
      />
    </div>
  );
}

export default App;
