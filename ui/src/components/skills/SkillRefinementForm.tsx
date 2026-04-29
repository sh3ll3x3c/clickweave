import { useState, type FormEvent } from "react";
import type {
  BindingCorrection,
  ParameterSlot,
  SkillRefinementProposal,
} from "../../store/slices/skillsSlice";

export type { SkillRefinementProposal };

interface SkillRefinementFormProps {
  initial: SkillRefinementProposal;
  onAccept: (proposal: SkillRefinementProposal) => void;
  onReject: () => void;
}

/// Minimal review form scaffolding. User edits the LLM-proposed
/// parameter schema; submit calls onAccept with the edited proposal.
/// Binding-correction rows show keep/reject toggles; precise
/// per-binding edit affordance is deferred to a Phase 5 polish pass.
export function SkillRefinementForm({
  initial,
  onAccept,
  onReject,
}: SkillRefinementFormProps) {
  const [params, setParams] = useState<ParameterSlot[]>(
    initial.parameter_schema,
  );
  const [corrections, setCorrections] = useState<BindingCorrection[]>(
    initial.binding_corrections,
  );
  const [description, setDescription] = useState(initial.description);
  const [nameSuggestion, setNameSuggestion] = useState(
    initial.name_suggestion ?? "",
  );

  const updateParam = (idx: number, patch: Partial<ParameterSlot>) => {
    setParams((prev) =>
      prev.map((p, i) => (i === idx ? { ...p, ...patch } : p)),
    );
  };

  const toggleCorrection = (idx: number) => {
    setCorrections((prev) =>
      prev.map((c, i) => (i === idx ? { ...c, keep: !c.keep } : c)),
    );
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    onAccept({
      parameter_schema: params,
      binding_corrections: corrections,
      description,
      name_suggestion: nameSuggestion.trim() ? nameSuggestion : null,
    });
  };

  const updateDefault = (idx: number, value: string) => {
    const parsed = value.trim() === "" ? null : parseDefaultValue(value);
    updateParam(idx, { default: parsed });
  };

  return (
    <form onSubmit={submit} className="space-y-3 p-3 text-xs">
      <section>
        <label className="mb-1 block text-[10px] uppercase tracking-wider text-[var(--text-muted)]">
          Name
        </label>
        <input
          type="text"
          value={nameSuggestion}
          onChange={(e) => setNameSuggestion(e.target.value)}
          className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs"
        />
      </section>

      <section>
        <label className="mb-1 block text-[10px] uppercase tracking-wider text-[var(--text-muted)]">
          Description
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={3}
          className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs"
        />
      </section>

      <section>
        <h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">
          Parameters ({params.length})
        </h3>
        <ul className="space-y-2">
          {params.map((p, idx) => (
            <li key={idx} className="flex items-center gap-2">
              <input
                type="text"
                value={p.name}
                onChange={(e) => updateParam(idx, { name: e.target.value })}
                placeholder="name"
                className="flex-1 rounded bg-[var(--bg-input)] px-2 py-1"
              />
              <input
                type="text"
                value={p.type_tag}
                onChange={(e) => updateParam(idx, { type_tag: e.target.value })}
                placeholder="type"
                className="w-24 rounded bg-[var(--bg-input)] px-2 py-1"
              />
              <input
                type="text"
                value={formatDefaultValue(p.default)}
                onChange={(e) => updateDefault(idx, e.target.value)}
                placeholder="default"
                className="w-28 rounded bg-[var(--bg-input)] px-2 py-1"
              />
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">
          Binding corrections ({corrections.length})
        </h3>
        <ul className="space-y-2">
          {corrections.map((c, idx) => (
            <li key={idx} className="flex items-center gap-2">
              <span className="flex-1 text-[var(--text-secondary)]">
                step {c.step_index} -&gt; {c.capture_name}
              </span>
              <label className="flex items-center gap-1">
                <input
                  type="checkbox"
                  checked={c.keep}
                  onChange={() => toggleCorrection(idx)}
                />
                keep
              </label>
            </li>
          ))}
        </ul>
      </section>

      <div className="flex justify-end gap-2 pt-2">
        <button
          type="button"
          onClick={onReject}
          className="rounded border border-[var(--border)] px-3 py-1 text-xs"
        >
          Reject
        </button>
        <button
          type="submit"
          className="rounded bg-[var(--accent-coral)] px-3 py-1 text-xs text-white"
        >
          Confirm
        </button>
      </div>
    </form>
  );
}

function formatDefaultValue(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

function parseDefaultValue(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}
