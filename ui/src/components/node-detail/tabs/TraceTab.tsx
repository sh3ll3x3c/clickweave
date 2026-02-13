import { useEffect, useState } from "react";
import { commands } from "../../../bindings";
import type { Artifact, TraceEvent } from "../../../bindings";
import { useNodeRuns } from "../hooks";
import { EmptyState, StatusBadge } from "../fields";
import { eventTypeColor, formatEventPayload, runDuration } from "../formatters";

export function TraceTab({
  nodeName,
  projectPath,
  workflowId,
  workflowName,
  initialRunId,
}: {
  nodeName: string;
  projectPath: string | null;
  workflowId: string;
  workflowName: string;
  initialRunId?: string | null;
}) {
  const runs = useNodeRuns(projectPath, workflowId, workflowName, nodeName);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(initialRunId ?? null);
  const [events, setEvents] = useState<TraceEvent[]>([]);
  const [artifactPreviews, setArtifactPreviews] = useState<
    Record<string, string>
  >({});

  // Select initialRunId if provided, otherwise auto-select first run
  useEffect(() => {
    if (runs.length === 0) return;
    if (initialRunId && runs.some((r) => r.run_id === initialRunId)) {
      setSelectedRunId(initialRunId);
    } else if (!selectedRunId) {
      setSelectedRunId(runs[0].run_id);
    }
  }, [runs, selectedRunId, initialRunId]);

  // Load events for selected run
  useEffect(() => {
    if (!selectedRunId) {
      setEvents([]);
      return;
    }
    const run = runs.find((r) => r.run_id === selectedRunId);
    commands
      .loadRunEvents({
        project_path: projectPath,
        workflow_id: workflowId,
        workflow_name: workflowName,
        node_name: nodeName,
        execution_dir: run?.execution_dir ?? null,
        run_id: selectedRunId,
      })
      .then((result) => {
        if (result.status === "ok") {
          setEvents(result.data);
        }
      });
  }, [projectPath, workflowId, workflowName, nodeName, selectedRunId, runs]);

  const selectedRun = runs.find((r) => r.run_id === selectedRunId) ?? null;

  // Load artifact previews for selected run
  useEffect(() => {
    if (!selectedRun) return;
    const screenshots = selectedRun.artifacts.filter(
      (a) => a.kind === "Screenshot",
    );
    for (const art of screenshots) {
      if (artifactPreviews[art.artifact_id]) continue;
      commands.readArtifactBase64(art.path).then((result) => {
        if (result.status === "ok") {
          setArtifactPreviews((prev) => ({
            ...prev,
            [art.artifact_id]: result.data,
          }));
        }
      });
    }
  }, [selectedRun]); // eslint-disable-line react-hooks/exhaustive-deps

  if (runs.length === 0) {
    return <EmptyState message="No runs yet. Execute the workflow to see trace data." />;
  }

  const duration = selectedRun ? runDuration(selectedRun) : null;

  return (
    <div className="space-y-4">
      {/* Run selector */}
      <div className="flex items-center gap-2">
        <label className="text-xs text-[var(--text-secondary)]">Run:</label>
        <select
          value={selectedRunId ?? ""}
          onChange={(e) => setSelectedRunId(e.target.value)}
          className="flex-1 rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
        >
          {runs.map((run) => (
            <option key={run.run_id} value={run.run_id}>
              {new Date(run.started_at).toLocaleString()} — {run.status}
            </option>
          ))}
        </select>
      </div>

      {/* Run summary */}
      {selectedRun && (
        <div className="flex items-center gap-3 rounded bg-[var(--bg-input)] px-3 py-2">
          <StatusBadge status={selectedRun.status} />
          {duration && (
            <span className="text-xs text-[var(--text-secondary)]">
              {duration}s
            </span>
          )}
          <span className="text-xs text-[var(--text-muted)]">
            {events.length} events
          </span>
          <span className="text-xs text-[var(--text-muted)]">
            {selectedRun.artifacts.length} artifacts
          </span>
        </div>
      )}

      {/* Events timeline */}
      {events.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            Events
          </h4>
          <div className="max-h-48 space-y-1 overflow-y-auto">
            {events.map((event, i) => (
              <div
                key={i}
                className="flex items-start gap-2 rounded bg-[var(--bg-input)] px-2.5 py-1.5"
              >
                <span className="mt-px shrink-0 text-[10px] font-mono text-[var(--text-muted)]">
                  {new Date(event.timestamp).toLocaleTimeString()}
                </span>
                <span
                  className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${eventTypeColor(event.event_type)}`}
                >
                  {event.event_type}
                </span>
                <span className="text-[11px] text-[var(--text-secondary)] truncate">
                  {formatEventPayload(event.payload)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Artifacts */}
      {selectedRun && selectedRun.artifacts.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            Artifacts
          </h4>
          <div className="grid grid-cols-2 gap-2">
            {selectedRun.artifacts.map((art) => (
              <ArtifactCard
                key={art.artifact_id}
                artifact={art}
                preview={artifactPreviews[art.artifact_id]}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function ArtifactCard({
  artifact,
  preview,
}: {
  artifact: Artifact;
  preview?: string;
}) {
  const filename = artifact.path.split("/").pop() ?? artifact.path;
  const isImage = artifact.kind === "Screenshot" || artifact.kind === "TemplateMatch";

  return (
    <div className="rounded border border-[var(--border)] bg-[var(--bg-input)] p-2">
      {isImage && preview ? (
        <img
          src={`data:image/png;base64,${preview}`}
          alt={filename}
          className="mb-1.5 w-full rounded object-contain"
          style={{ maxHeight: 120 }}
        />
      ) : (
        <div className="mb-1.5 flex h-16 items-center justify-center rounded bg-[var(--bg-dark)] text-xs text-[var(--text-muted)]">
          {artifact.kind}
        </div>
      )}
      <div className="truncate text-[10px] text-[var(--text-secondary)]">
        {filename}
      </div>
    </div>
  );
}
