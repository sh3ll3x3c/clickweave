import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [response, setResponse] = useState<string>("");

  async function handlePing() {
    const result = await invoke<string>("ping");
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
      </div>
    </div>
  );
}

export default App;
