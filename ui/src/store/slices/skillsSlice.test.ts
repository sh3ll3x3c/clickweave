import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    static async getByLabel() {
      return null;
    }
  },
}));
vi.mock("@tauri-apps/api/window", () => ({
  currentMonitor: async () => null,
}));

import { useStore } from "../useAppStore";
import type { SkillSummary } from "./skillsSlice";

function reset() {
  useStore.setState({
    drafts: [],
    confirmed: [],
    promoted: [],
    selectedSkill: null,
    breadcrumb: [],
  });
}

function summary(
  partial: Partial<SkillSummary> & { id: string; version: number },
): SkillSummary {
  return {
    name: partial.id,
    description: "",
    state: "draft",
    scope: "project_local",
    occurrence_count: 1,
    success_rate: 1,
    edited_by_user: false,
    ...partial,
  };
}

describe("skillsSlice.setSkillsList", () => {
  beforeEach(reset);

  it("buckets skills into drafts / confirmed / promoted by state", () => {
    useStore.getState().setSkillsList([
      summary({ id: "a", version: 1, state: "draft" }),
      summary({ id: "b", version: 1, state: "confirmed" }),
      summary({ id: "c", version: 2, state: "promoted" }),
      summary({ id: "d", version: 1, state: "draft" }),
    ]);
    const s = useStore.getState();
    expect(s.drafts.map((x) => x.id)).toEqual(["a", "d"]);
    expect(s.confirmed.map((x) => x.id)).toEqual(["b"]);
    expect(s.promoted.map((x) => x.id)).toEqual(["c"]);
  });
});

describe("skillsSlice.applySkillExtracted", () => {
  beforeEach(reset);

  it("inserts a stub draft entry on first extraction", () => {
    useStore.getState().applySkillExtracted({
      run_id: "r1",
      event_run_id: "r1",
      skill_id: "new-skill",
      version: 1,
      state: "draft",
      scope: "project_local",
    });
    expect(useStore.getState().drafts).toHaveLength(1);
    expect(useStore.getState().drafts[0].id).toBe("new-skill");
  });

  it("upserts on a second extraction with the same id+version", () => {
    useStore.getState().applySkillExtracted({
      run_id: "r1",
      event_run_id: "r1",
      skill_id: "skill-a",
      version: 1,
      state: "draft",
      scope: "project_local",
    });
    useStore.getState().applySkillExtracted({
      run_id: "r1",
      event_run_id: "r1",
      skill_id: "skill-a",
      version: 1,
      state: "draft",
      scope: "project_local",
    });
    expect(useStore.getState().drafts).toHaveLength(1);
  });
});

describe("skillsSlice.applySkillConfirmed", () => {
  beforeEach(reset);

  it("moves a skill from drafts to confirmed", () => {
    useStore
      .getState()
      .setSkillsList([summary({ id: "x", version: 1, state: "draft" })]);
    useStore.getState().applySkillConfirmed({
      run_id: "r1",
      event_run_id: "r1",
      skill_id: "x",
      version: 1,
    });
    const s = useStore.getState();
    expect(s.drafts).toHaveLength(0);
    expect(s.confirmed.map((x) => x.id)).toEqual(["x"]);
  });
});

describe("skillsSlice.breadcrumb", () => {
  beforeEach(reset);

  it("push then pop maintains stack order", () => {
    useStore
      .getState()
      .pushSkillBreadcrumb({ id: "parent", version: 1, name: "parent" });
    useStore
      .getState()
      .pushSkillBreadcrumb({ id: "child", version: 1, name: "child" });
    expect(useStore.getState().breadcrumb).toHaveLength(2);
    useStore.getState().popSkillBreadcrumb();
    expect(useStore.getState().breadcrumb.map((x) => x.id)).toEqual(["parent"]);
  });
});
