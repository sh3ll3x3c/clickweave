import { describe, expect, it, vi } from "vitest";
import { projectSketchToCanvas } from "./SkillDetailView";
import type { ActionSketchStep, SkillSummary } from "../../store/slices/skillsSlice";

function skill(partial: Partial<SkillSummary> & { id: string; version: number }): SkillSummary {
  return {
    name: partial.id,
    description: "",
    state: "confirmed",
    scope: "project_local",
    occurrence_count: 1,
    success_rate: 1,
    edited_by_user: false,
    ...partial,
  };
}

describe("projectSketchToCanvas", () => {
  it("projects tool, sub-skill, and loop steps into read-only React Flow data", () => {
    const openSubSkill = vi.fn();
    const sketch: ActionSketchStep[] = [
      {
        kind: "tool_call",
        tool: "click",
        args: { target: "{{params.target}}" },
      },
      {
        kind: "sub_skill",
        skill_id: "child",
        version: 2,
        parameters: {},
        bind_outputs_as: { output: "captured_output" },
      },
      {
        kind: "loop",
        until: { kind: "world_model_delta", expr: "loading == false" },
        max_iterations: 3,
        iteration_delay_ms: 250,
        body: [
          {
            kind: "tool_call",
            tool: "wait",
            args: {},
          },
        ],
      },
    ];

    const canvas = projectSketchToCanvas(
      sketch,
      [skill({ id: "child", version: 2, name: "Child Skill" })],
      openSubSkill,
    );

    expect(canvas.readOnly).toBe(true);
    expect(canvas.nodes.map((n) => n.type)).toEqual([
      "skillToolCall",
      "skillSubSkill",
      "skillLoop",
      "skillToolCall",
    ]);
    expect(canvas.nodes[3].parentId).toBe(canvas.nodes[2].id);
    expect(canvas.edges.length).toBeGreaterThanOrEqual(2);

    (canvas.nodes[1].data.onOpen as () => void)();
    expect(openSubSkill).toHaveBeenCalledWith({ id: "child", version: 2 });
  });
});
