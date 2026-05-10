/**
 * `SkillView` — primary skill rendering surface for Phase 1.F.
 *
 * Renders `selectedSkill.sections` as a virtualized vertical scrolling list of
 * `SkillSectionCard` components via `react-window` `FixedSizeList`.
 *
 * Selection state is managed by `SkillSelectionContext`:
 * - Single click → `selectSingle`
 * - Shift+click → `extendRange`
 * - ⌘/Ctrl+click → `toggleMulti`
 */

import { useRef, useState, useEffect } from "react";
import { FixedSizeList, type ListChildComponentProps } from "react-window";
import type { Skill, SkillSection } from "../../bindings";
import { useStore } from "../../store/useAppStore";
import { SkillSectionCard } from "./SkillSectionCard";
import {
  SkillSelectionProvider,
  useSkillSelection,
} from "./SkillSelectionContext";

const ITEM_HEIGHT = 80; // px, fixed-height card row

interface SectionRowData {
  sections: SkillSection[];
  body: string;
  selectedIds: string[];
  onSectionClick: (section: SkillSection, e: React.MouseEvent) => void;
}

function SectionRow({
  index,
  style,
  data,
}: ListChildComponentProps<SectionRowData>) {
  const { sections, body, selectedIds, onSectionClick } = data;
  const section = sections[index];
  if (!section) return null;

  const [start, end] = section.body_range;
  const sectionBody = body.slice(start, end);
  const isSelected = selectedIds.includes(section.id);

  return (
    <div style={style} className="px-2 py-1">
      <SkillSectionCard
        section={section}
        sectionBody={sectionBody}
        selected={isSelected}
        onClick={(e) => onSectionClick(section, e)}
      />
    </div>
  );
}

interface SkillViewInnerProps {
  skill: Skill;
}

function SkillViewInner({ skill }: SkillViewInnerProps) {
  const { selectedSectionIds, selectSingle, extendRange, toggleMulti } =
    useSkillSelection();

  const containerRef = useRef<HTMLDivElement>(null);
  const [listHeight, setListHeight] = useState(400);

  // Observe the container's height to keep the FixedSizeList sized correctly.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const obs = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setListHeight(entry.contentRect.height);
      }
    });
    obs.observe(el);
    setListHeight(el.clientHeight);
    return () => obs.disconnect();
  }, []);

  const sections = skill.sections ?? [];
  const body = skill.body ?? "";

  const handleSectionClick = (section: SkillSection, e: React.MouseEvent) => {
    if (e.shiftKey) {
      extendRange(
        section.id,
        sections.map((s) => s.id),
      );
    } else if (e.metaKey || e.ctrlKey) {
      toggleMulti(section.id);
    } else {
      selectSingle(section.id);
    }
  };

  if (sections.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
        No sections in this skill.
      </div>
    );
  }

  const itemData: SectionRowData = {
    sections,
    body,
    selectedIds: selectedSectionIds,
    onSectionClick: handleSectionClick,
  };

  return (
    <div ref={containerRef} className="h-full w-full">
      <FixedSizeList
        width="100%"
        height={listHeight}
        itemCount={sections.length}
        itemSize={ITEM_HEIGHT}
        itemData={itemData}
        overscanCount={3}
      >
        {SectionRow}
      </FixedSizeList>
    </div>
  );
}

export function SkillView() {
  const selectedSkill = useStore((s) => s.selectedSkill);

  if (!selectedSkill) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--text-muted)]">
        Select a skill to view its sections.
      </div>
    );
  }

  return (
    <SkillSelectionProvider skillId={selectedSkill.id}>
      <div className="flex h-full flex-col bg-[var(--bg-dark)]">
        {/* Skill header */}
        <div className="shrink-0 border-b border-[var(--border)] px-4 py-3">
          <h2 className="text-sm font-semibold text-[var(--text-primary)]">
            {selectedSkill.name}
          </h2>
          {selectedSkill.description && (
            <p className="mt-0.5 text-xs text-[var(--text-secondary)]">
              {selectedSkill.description}
            </p>
          )}
        </div>

        {/* Section list */}
        <div className="min-h-0 flex-1">
          <SkillViewInner skill={selectedSkill} />
        </div>
      </div>
    </SkillSelectionProvider>
  );
}
