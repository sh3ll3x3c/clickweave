import { useState, useEffect } from "react";
import { commands } from "./bindings";
import type { NodeTypeInfo } from "./bindings";

function App() {
  const [response, setResponse] = useState<string>("");
  const [nodeTypes, setNodeTypes] = useState<NodeTypeInfo[]>([]);

  useEffect(() => {
    commands.nodeTypeDefaults().then(setNodeTypes);
  }, []);

  async function handlePing() {
    const result = await commands.ping();
    setResponse(result);
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[var(--bg-dark)]">
      <div className="text-center">
        <h1 className="mb-6 text-3xl font-bold text-[var(--text-primary)]">
          Clickweave
        </h1>
        <button
          onClick={handlePing}
          className="rounded-lg bg-[var(--accent-coral)] px-6 py-3 font-medium text-white transition-colors hover:bg-[var(--accent-coral)]/80"
        >
          Ping Backend
        </button>
        {response && (
          <p className="mt-4 text-lg text-[var(--accent-green)]">
            Response: {response}
          </p>
        )}
        {nodeTypes.length > 0 && (
          <div className="mt-6 text-left">
            <p className="mb-2 text-sm text-[var(--text-secondary)]">
              Available node types ({nodeTypes.length}):
            </p>
            <div className="flex flex-wrap gap-2">
              {nodeTypes.map((nt) => (
                <span
                  key={nt.name}
                  className="rounded bg-[var(--bg-panel)] px-2 py-1 text-xs text-[var(--text-primary)]"
                >
                  {nt.icon} {nt.name}
                </span>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
