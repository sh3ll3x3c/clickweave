import { useState } from "react";
import { useStore } from "../store/useAppStore";
import type { NodeVerdict, CheckResult } from "../store/slices/verdictSlice";

export function VerdictBar() {
  const verdicts = useStore((s) => s.verdicts);
  const status = useStore((s) => s.verdictStatus);
  const visible = useStore((s) => s.verdictBarVisible);
  const dismiss = useStore((s) => s.dismissVerdictBar);
  const [expanded, setExpanded] = useState(false);

  if (!visible || status === "none") return null;

  const totalChecks = verdicts.reduce(
    (sum, v) => sum + v.check_results.length + (v.expected_outcome_verdict ? 1 : 0),
    0,
  );
  const passedChecks = verdicts.reduce(
    (sum, v) =>
      sum +
      v.check_results.filter((r) => r.verdict === "Pass").length +
      (v.expected_outcome_verdict?.verdict === "Pass" ? 1 : 0),
    0,
  );

  const bgColor =
    status === "passed"
      ? "bg-green-900/80 border-green-700"
      : status === "warned"
        ? "bg-yellow-900/80 border-yellow-700"
        : "bg-red-900/80 border-red-700";

  const label =
    status === "passed"
      ? `PASSED \u2014 ${passedChecks}/${totalChecks} checks`
      : status === "warned"
        ? `PASSED with warnings \u2014 ${passedChecks}/${totalChecks} checks`
        : `FAILED \u2014 ${passedChecks}/${totalChecks} checks passed`;

  return (
    <div className={`border-b ${bgColor}`}>
      <div className="flex items-center justify-between px-4 py-2">
        <button
          onClick={() => setExpanded(!expanded)}
          className="text-sm font-semibold text-white hover:underline"
        >
          {label}
        </button>
        <button
          onClick={dismiss}
          className="text-xs text-white/60 hover:text-white"
        >
          Dismiss
        </button>
      </div>
      {expanded && (
        <div className="space-y-3 border-t border-white/10 px-4 py-3">
          {verdicts.map((v, i) => (
            <VerdictNodeRow key={`${v.node_id}-${i}`} verdict={v} />
          ))}
        </div>
      )}
    </div>
  );
}

function VerdictNodeRow({ verdict }: { verdict: NodeVerdict }) {
  const [open, setOpen] = useState(false);
  const allResults: CheckResult[] = [
    ...verdict.check_results,
    ...(verdict.expected_outcome_verdict ? [verdict.expected_outcome_verdict] : []),
  ];

  return (
    <div>
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 text-xs text-white/90 hover:text-white"
      >
        <span>{open ? "\u25BC" : "\u25B6"}</span>
        <span className="font-medium">{verdict.node_name}</span>
        <span className="text-white/50">
          ({allResults.filter((r) => r.verdict === "Pass").length}/{allResults.length} passed)
        </span>
      </button>
      {open && (
        <div className="ml-5 mt-1 space-y-1">
          {allResults.map((r, i) => (
            <div key={i} className="text-xs">
              <span
                className={
                  r.verdict === "Pass"
                    ? "text-green-400"
                    : r.verdict === "Warn"
                      ? "text-yellow-400"
                      : "text-red-400"
                }
              >
                {r.verdict === "Pass" ? "\u2713" : r.verdict === "Warn" ? "\u26A0" : "\u2717"}
              </span>{" "}
              <span className="text-white/80">{r.check_name}</span>
              <span className="ml-2 text-white/40">&mdash; {r.reasoning}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
