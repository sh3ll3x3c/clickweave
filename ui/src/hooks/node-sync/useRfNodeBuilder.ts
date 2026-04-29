import { type Dispatch, type SetStateAction, useEffect } from "react";
import type { Node as RFNode } from "@xyflow/react";
import type { Workflow } from "../../bindings";
import type { AppGroupMeta } from "../useAppGrouping";
import type { UserGroupMeta } from "../useUserGrouping";
import type { RunTrace } from "../../store/slices/assistantSlice";
import {
  APP_GROUP_HEADER_HEIGHT,
  APP_GROUP_PADDING,
  USER_GROUP_HEADER_HEIGHT,
  USER_GROUP_PADDING,
  APPROX_NODE_WIDTH,
  APPROX_NODE_HEIGHT,
  MIN_GROUP_WIDTH,
  MIN_GROUP_HEIGHT,
  groupConstants,
  buildAppKindMap,
  toRFNode,
} from "./nodeBuilders";

interface UseRfNodeBuilderParams {
  workflow: Workflow;
  selectedNode: string | null;
  activeNode: string | null;
  collapsedApps: Set<string>;
  appGroups: Map<string, string[]>;
  nodeToAppGroup: Map<string, string>;
  appGroupMeta: Map<string, AppGroupMeta>;
  toggleAppCollapse: (groupId: string) => void;
  agentRunCollapsed: Record<string, boolean>;
  runTraces: Record<string, RunTrace>;
  toggleAgentRunCollapsed: (runId: string) => void;
  collapsedUserGroups: Set<string>;
  nodeToUserGroup: Map<string, string>;
  userGroupMeta: Map<string, UserGroupMeta>;
  toggleUserGroupCollapse: (groupId: string) => void;
  renamingGroupId: string | null;
  onRenameConfirm: (groupId: string, newName: string) => void;
  onRenameCancel: () => void;
  onDeleteNodes: (ids: string[]) => void;
  setRfNodes: Dispatch<SetStateAction<RFNode[]>>;
}

export const AGENT_RUN_GROUP_PREFIX = "agent-run-";

export interface AgentRunProjectionContext {
  workflow: Workflow;
  collapsedApps: Set<string>;
  appGroups: Map<string, string[]>;
  appGroupMeta: Map<string, AppGroupMeta>;
  nodeToUserGroup: Map<string, string>;
  agentRunCollapsed: Record<string, boolean>;
  runTraces: Record<string, RunTrace>;
  toggleAgentRunCollapsed: (runId: string) => void;
  prevMap?: Map<string, RFNode>;
}

function truncate(text: string, maxLength: number): string {
  const trimmed = text.trim();
  if (trimmed.length <= maxLength) return trimmed;
  return `${trimmed.slice(0, maxLength - 1)}...`;
}

function agentRunGroupId(runId: string): string {
  return `${AGENT_RUN_GROUP_PREFIX}${runId}`;
}

function singleRunId(
  memberIds: string[],
  sourceRunByNodeId: Map<string, string | null | undefined>,
): string | null {
  let runId: string | null = null;
  for (const memberId of memberIds) {
    const memberRunId = sourceRunByNodeId.get(memberId);
    if (!memberRunId) return null;
    if (runId == null) {
      runId = memberRunId;
    } else if (runId !== memberRunId) {
      return null;
    }
  }
  return runId;
}

function childPosition(
  node: RFNode,
  runGroupId: string,
  containerPosition: { x: number; y: number },
  previous: RFNode | undefined,
): { x: number; y: number } {
  if (previous?.parentId === runGroupId) {
    return previous.position;
  }
  return {
    x: node.position.x - containerPosition.x + USER_GROUP_PADDING,
    y: node.position.y - containerPosition.y + USER_GROUP_HEADER_HEIGHT + USER_GROUP_PADDING,
  };
}

function runSummary(runId: string, trace: RunTrace | undefined): string {
  return truncate(
    trace?.terminalFrame?.detail || trace?.activeSubgoal || `Agent run ${runId}`,
    80,
  );
}

export function projectAgentRunGroups(
  rfNodes: RFNode[],
  ctx: AgentRunProjectionContext,
): RFNode[] {
  const sourceRunByNodeId = new Map(
    ctx.workflow.nodes.map((node) => [node.id, node.source_run_id]),
  );
  const candidateRunByNodeId = new Map<string, string>();
  const candidateIdsByRun = new Map<string, string[]>();
  const appGroupMemberIds = new Set<string>();

  const addCandidate = (nodeId: string, runId: string) => {
    if (candidateRunByNodeId.has(nodeId)) return;
    candidateRunByNodeId.set(nodeId, runId);
    const runCandidates = candidateIdsByRun.get(runId) ?? [];
    runCandidates.push(nodeId);
    candidateIdsByRun.set(runId, runCandidates);
  };

  for (const [groupId, memberIds] of ctx.appGroups) {
    for (const memberId of memberIds) appGroupMemberIds.add(memberId);
    if (memberIds.some((memberId) => ctx.nodeToUserGroup.has(memberId))) {
      continue;
    }
    const runId = singleRunId(memberIds, sourceRunByNodeId);
    if (!runId) continue;

    if (ctx.collapsedApps.has(groupId)) {
      const anchorId = ctx.appGroupMeta.get(groupId)?.anchorId;
      if (anchorId) addCandidate(anchorId, runId);
    } else {
      addCandidate(groupId, runId);
    }
  }

  for (const node of rfNodes) {
    if (node.type !== "workflow") continue;
    if (ctx.nodeToUserGroup.has(node.id)) continue;
    if (appGroupMemberIds.has(node.id)) continue;
    const runId = sourceRunByNodeId.get(node.id);
    if (!runId) continue;
    addCandidate(node.id, runId);
  }

  if (candidateRunByNodeId.size === 0) return rfNodes;

  const emittedRunIds = new Set<string>();
  const candidateIds = new Set(candidateRunByNodeId.keys());
  const output: RFNode[] = [];

  for (const node of rfNodes) {
    const runId = candidateRunByNodeId.get(node.id);
    if (!runId) {
      output.push(node);
      continue;
    }
    if (emittedRunIds.has(runId)) continue;
    emittedRunIds.add(runId);

    const runGroupId = agentRunGroupId(runId);
    const runCandidateIds = candidateIdsByRun.get(runId) ?? [];
    const runCandidates = rfNodes.filter((n) => runCandidateIds.includes(n.id));
    const existingGroup = ctx.prevMap?.get(runGroupId);
    const firstCandidate = runCandidates[0] ?? node;
    const containerPosition =
      existingGroup?.position ?? firstCandidate.position ?? { x: 0, y: 0 };
    const isCollapsed = !!ctx.agentRunCollapsed[runId];
    const runNodeIds = ctx.workflow.nodes
      .filter((wfNode) => wfNode.source_run_id === runId)
      .map((wfNode) => wfNode.id);

    let maxX = 0;
    let maxY = 0;
    for (const child of runCandidates) {
      const previous = ctx.prevMap?.get(child.id);
      const position = childPosition(
        child,
        runGroupId,
        containerPosition,
        previous,
      );
      const measured = previous?.measured;
      const childW =
        measured?.width ?? (child.style?.width as number | undefined) ?? APPROX_NODE_WIDTH;
      const childH =
        measured?.height ?? (child.style?.height as number | undefined) ?? APPROX_NODE_HEIGHT;
      maxX = Math.max(maxX, position.x + childW);
      maxY = Math.max(maxY, position.y + childH);
    }

    output.push({
      id: runGroupId,
      type: "agent_run_group",
      position: containerPosition,
      draggable: true,
      selected: existingGroup?.selected ?? false,
      data: {
        runId,
        summary: runSummary(runId, ctx.runTraces[runId]),
        stepCount: runNodeIds.length,
        isCollapsed,
        onToggleCollapse: () => ctx.toggleAgentRunCollapsed(runId),
      },
      style: {
        ...existingGroup?.style,
        width: Math.max(MIN_GROUP_WIDTH, maxX + USER_GROUP_PADDING),
        height: Math.max(MIN_GROUP_HEIGHT, maxY + USER_GROUP_PADDING),
      },
    });

    for (const member of runCandidates) {
      const previous = ctx.prevMap?.get(member.id);
      output.push({
        ...member,
        parentId: runGroupId,
        extent: "parent" as const,
        position: childPosition(member, runGroupId, containerPosition, previous),
        hidden: isCollapsed,
        style: { ...member.style, transition: "opacity 150ms ease 50ms" },
      });
    }
  }

  const collapsedRunIds = new Set(
    Object.entries(ctx.agentRunCollapsed)
      .filter(([, collapsed]) => collapsed)
      .map(([runId]) => runId),
  );
  return output.map((node) => {
    if (candidateIds.has(node.id) || !node.parentId) return node;
    const runId = sourceRunByNodeId.get(node.id);
    if (!runId || !collapsedRunIds.has(runId)) return node;
    return { ...node, hidden: true };
  });
}

/**
 * Syncs workflow nodes into ReactFlow node state.
 *
 * Handles app groups, user groups, collapsed/expanded states, and parent-child
 * relationships. Runs whenever workflow structure or grouping state changes.
 */
export function useRfNodeBuilder({
  workflow,
  selectedNode,
  activeNode,
  collapsedApps,
  appGroups,
  nodeToAppGroup,
  appGroupMeta,
  toggleAppCollapse,
  agentRunCollapsed,
  runTraces,
  toggleAgentRunCollapsed,
  collapsedUserGroups,
  nodeToUserGroup,
  userGroupMeta,
  toggleUserGroupCollapse,
  renamingGroupId,
  onRenameConfirm,
  onRenameCancel,
  onDeleteNodes,
  setRfNodes,
}: UseRfNodeBuilderParams) {
  useEffect(() => {
    setRfNodes((prev) => {
      const prevMap = new Map(prev.map((n) => [n.id, n]));
      const wfNodeMap = new Map(workflow.nodes.map((n) => [n.id, n]));
      const appKindMap = buildAppKindMap(workflow);

      // Build set of anchor IDs for app groups
      const appGroupAnchors = new Set<string>();
      for (const meta of appGroupMeta.values()) {
        appGroupAnchors.add(meta.anchorId);
      }

      const nodes: RFNode[] = [];
      const groupNodeIndices = new Map<string, number>();
      const expandedGroupChildren = new Map<string, RFNode[]>();

      for (const node of workflow.nodes) {
        const existing = prevMap.get(node.id);

        // App group anchor nodes
        if (appGroupAnchors.has(node.id)) {
          const groupId = nodeToAppGroup.get(node.id);
          if (!groupId) continue;
          const meta = appGroupMeta.get(groupId);
          if (!meta) continue;
          const memberIds = appGroups.get(groupId) ?? [];
          const visibleMemberIds = memberIds;

          if (collapsedApps.has(groupId)) {
            // Collapsed — render as workflow pill using anchor's real ID
            const base = toRFNode(node, selectedNode, activeNode, () => {
              onDeleteNodes(memberIds);
            }, appKindMap.get(node.id), existing);
            nodes.push({
              ...base,
              type: "workflow",
              data: {
                ...base.data,
                label: meta.appName,
                color: meta.color,
                icon: "AG",
                bodyCount: visibleMemberIds.length,
                hideSourceHandle: true,
                onToggleCollapse: () => toggleAppCollapse(groupId),
              },
            });
          } else {
            // Expanded — emit synthetic parent + anchor as child
            const parentPosition = existing?.position ?? { x: node.position.x, y: node.position.y };

            // Synthetic group parent node
            const existingGroup = prevMap.get(groupId);
            const parentIdx = nodes.length;
            nodes.push({
              id: groupId,
              type: "appGroup",
              position: existingGroup?.position ?? parentPosition,
              draggable: true,
              selected: false,
              data: {
                appName: meta.appName,
                color: meta.color,
                memberCount: visibleMemberIds.length,
                isActive: node.id === activeNode,
                onToggleCollapse: () => toggleAppCollapse(groupId),
              },
            });
            groupNodeIndices.set(groupId, parentIdx);
            expandedGroupChildren.set(groupId, []);

            // Anchor as child inside the group
            const anchorBase = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), appKindMap.get(node.id), existing);
            const relativePosition = existing?.parentId === groupId
              ? existing.position
              : { x: APP_GROUP_PADDING, y: APP_GROUP_HEADER_HEIGHT + APP_GROUP_PADDING };
            const childNode = {
              ...anchorBase,
              parentId: groupId,
              extent: "parent" as const,
              position: relativePosition,
              style: { ...anchorBase.style, transition: "opacity 150ms ease 50ms" },
            };
            nodes.push(childNode);
            expandedGroupChildren.get(groupId)?.push(childNode);
          }
          continue;
        }

        // App group member nodes (non-anchor)
        const appGroup = nodeToAppGroup.get(node.id);
        if (appGroup && !appGroupAnchors.has(node.id)) {
          const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), appKindMap.get(node.id), existing);

          if (collapsedApps.has(appGroup)) {
            nodes.push({ ...base, hidden: true });
          } else {
            const meta = appGroupMeta.get(appGroup);
            const anchorNode = meta ? wfNodeMap.get(meta.anchorId) : undefined;

            let relativePosition = base.position;
            if (existing?.parentId === appGroup) {
              relativePosition = existing.position;
            } else if (anchorNode) {
              relativePosition = {
                x: node.position.x - anchorNode.position.x + APP_GROUP_PADDING,
                y: node.position.y - anchorNode.position.y + APP_GROUP_HEADER_HEIGHT + APP_GROUP_PADDING,
              };
            }

            const childNode = {
              ...base,
              parentId: appGroup,
              extent: "parent" as const,
              position: relativePosition,
              style: { ...base.style, transition: "opacity 150ms ease 50ms" },
            };
            nodes.push(childNode);
            expandedGroupChildren.get(appGroup)?.push(childNode);
          }
          continue;
        }

        // Regular node
        const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), appKindMap.get(node.id), existing);
        nodes.push(base);
      }

      // Size each expanded group node to contain all its children, then center them
      for (const [groupId, children] of expandedGroupChildren) {
        const idx = groupNodeIndices.get(groupId);
        if (idx === undefined) continue;
        const groupNode = nodes[idx];
        const gc = groupConstants(groupNode.type ?? "appGroup");

        let maxX = 0;
        let maxY = 0;
        let maxChildW = 0;
        for (const child of children) {
          const measured = prevMap.get(child.id)?.measured;
          const childW = measured?.width ?? APPROX_NODE_WIDTH;
          const childH = measured?.height ?? APPROX_NODE_HEIGHT;
          maxX = Math.max(maxX, child.position.x + childW);
          maxY = Math.max(maxY, child.position.y + childH);
          maxChildW = Math.max(maxChildW, childW);
        }

        const containerW = Math.max(MIN_GROUP_WIDTH, maxX + gc.padding);
        groupNode.style = {
          ...groupNode.style,
          width: containerW,
          height: Math.max(MIN_GROUP_HEIGHT, maxY + gc.padding),
        };

        // Center children horizontally within the container
        const centerX = (containerW - maxChildW) / 2;
        if (centerX > gc.padding) {
          for (const child of children) {
            // Only center on initial layout (when child hasn't been manually positioned)
            if (!prevMap.get(child.id)?.parentId) {
              child.position = { x: centerX, y: child.position.y };
            }
          }
        }
      }

      const projectedNodes = projectAgentRunGroups(nodes, {
        workflow,
        collapsedApps,
        appGroups,
        appGroupMeta,
        nodeToUserGroup,
        agentRunCollapsed,
        runTraces,
        toggleAgentRunCollapsed,
        prevMap,
      });
      nodes.splice(0, nodes.length, ...projectedNodes);

      // ── Second pass: user group rendering ──────────────────────────
      // Runs after auto-groups (app groups) are resolved.
      // Reassigns rendered nodes into user group containers or collapses them into pills.
      const nodeIndexById = new Map<string, number>();
      for (let i = 0; i < nodes.length; i++) nodeIndexById.set(nodes[i].id, i);

      // Pre-build reverse map: anchor node ID → app group ID
      const anchorToAppGroup = new Map<string, string>();
      for (const [agId, agMeta] of appGroupMeta) {
        anchorToAppGroup.set(agMeta.anchorId, agId);
      }

      for (const group of workflow.groups ?? []) {
        const meta = userGroupMeta.get(group.id);
        if (!meta) continue;
        if (group.node_ids.length === 0) continue;

        // Skip collapsed groups whose parent user group is also collapsed
        if (meta.parentGroupId && collapsedUserGroups.has(meta.parentGroupId)) continue;

        const anchorId = meta.anchorId;
        const anchorIdx = nodeIndexById.get(anchorId);

        if (collapsedUserGroups.has(group.id)) {
          // ── Collapsed: convert anchor to pill, hide all other members ──
          if (anchorIdx !== undefined) {
            const anchorNode = nodes[anchorIdx];
            nodes[anchorIdx] = {
              ...anchorNode,
              type: "workflow",
              data: {
                ...anchorNode.data,
                label: meta.name,
                color: meta.color,
                icon: "\uD83D\uDCC1",
                bodyCount: meta.flatMemberCount,
                isUserGroupPill: true,
                userGroupId: group.id,
                isRenaming: renamingGroupId === group.id,
                onRenameConfirm: (newName: string) => onRenameConfirm(group.id, newName),
                onRenameCancel,
                onToggleCollapse: () => toggleUserGroupCollapse(group.id),
              },
            };
            // Preserve the anchor's existing parentId (e.g., if inside an auto-group)
          }

          // Hide all non-anchor members
          for (const nodeId of group.node_ids) {
            if (nodeId === anchorId) continue;
            const idx = nodeIndexById.get(nodeId);
            if (idx !== undefined) {
              nodes[idx] = { ...nodes[idx], hidden: true };
            }
            // Also hide synthetic app group containers whose anchor is a member
            const agId = anchorToAppGroup.get(nodeId);
            if (agId) {
              const agIdx = nodeIndexById.get(agId);
              if (agIdx !== undefined) nodes[agIdx] = { ...nodes[agIdx], hidden: true };
            }
          }
        } else {
          // ── Expanded: create synthetic container, reparent members ──
          const anchorNode = anchorIdx !== undefined ? nodes[anchorIdx] : undefined;
          const existingGroupNode = prevMap.get(group.id);

          // Compute anchor's absolute position: if anchor is inside an auto-group
          // (has parentId pointing to an app group), its position is relative —
          // add the parent's position. Skip when parent is a user group (set by
          // a previous iteration) to avoid double-offset when subgroup is
          // reparented back into that user group.
          let anchorAbsPosition = anchorNode?.position ?? { x: 0, y: 0 };
          const anchorParentIsAutoGroup = anchorNode?.parentId
            ? appGroups.has(anchorNode.parentId)
            : false;
          if (anchorParentIsAutoGroup && !existingGroupNode) {
            const parentIdx = nodeIndexById.get(anchorNode!.parentId!);
            const parentNode = parentIdx !== undefined ? nodes[parentIdx] : undefined;
            if (parentNode) {
              anchorAbsPosition = {
                x: anchorAbsPosition.x + parentNode.position.x,
                y: anchorAbsPosition.y + parentNode.position.y,
              };
            }
          }

          const containerPosition = existingGroupNode?.position
            ?? anchorAbsPosition;

          // Determine if the user group should be inside an auto-group.
          // Only check actual auto-group IDs (appGroups keys), NOT user group parents
          // which may have been set by a previous iteration of this second pass.
          const anchorAutoParent = anchorNode?.parentId;
          const isAutoGroupParent = anchorAutoParent ? appGroups.has(anchorAutoParent) : false;

          let containerParentId: string | undefined;
          if (anchorAutoParent && isAutoGroupParent) {
            const autoGroupMembers = appGroups.get(anchorAutoParent) ?? [];
            const userGroupNodeSet = new Set(group.node_ids);
            const autoGroupFullyWrapped = autoGroupMembers.every((m) => userGroupNodeSet.has(m));
            if (autoGroupFullyWrapped) {
              // User group wraps the auto-group — user group is the outer container
              const autoGroupIdx = nodeIndexById.get(anchorAutoParent!);
              const autoGroupNode = autoGroupIdx !== undefined ? nodes[autoGroupIdx] : undefined;
              containerParentId = autoGroupNode?.parentId;
            } else {
              // User group is inside the auto-group
              containerParentId = anchorAutoParent;
            }
          } else if (anchorAutoParent && !isAutoGroupParent) {
            // Parent is a user group (set by earlier iteration) — subgroup stays inside parent
            containerParentId = anchorAutoParent;
          }

          const containerIdx = nodes.length;
          nodes.push({
            id: group.id,
            type: "userGroup",
            position: containerPosition,
            parentId: containerParentId,
            extent: containerParentId ? "parent" as const : undefined,
            draggable: true,
            selected: false,
            data: {
              name: meta.name,
              color: meta.color,
              memberCount: meta.flatMemberCount,
              isRenaming: renamingGroupId === group.id,
              onRenameConfirm: (newName: string) => onRenameConfirm(group.id, newName),
              onRenameCancel,
              onToggleCollapse: () => toggleUserGroupCollapse(group.id),
            },
          });
          nodeIndexById.set(group.id, containerIdx);

          // Reparent all member nodes to the user group container
          const userGroupChildren: RFNode[] = [];
          for (const nodeId of group.node_ids) {
            const idx = nodeIndexById.get(nodeId);
            if (idx === undefined) continue;
            const memberNode = nodes[idx];
            if (memberNode.hidden) continue;

            let relativePosition: { x: number; y: number };
            if (memberNode.parentId === group.id) {
              // Already parented to this group in a previous render — keep position
              relativePosition = memberNode.position;
            } else if (anchorNode) {
              // Compute relative position from the anchor
              const memberAbsX = memberNode.position.x;
              const memberAbsY = memberNode.position.y;
              const anchorAbsX = anchorNode.position.x;
              const anchorAbsY = anchorNode.position.y;
              relativePosition = {
                x: memberAbsX - anchorAbsX + USER_GROUP_PADDING,
                y: memberAbsY - anchorAbsY + USER_GROUP_HEADER_HEIGHT + USER_GROUP_PADDING,
              };
            } else {
              relativePosition = { x: USER_GROUP_PADDING, y: USER_GROUP_HEADER_HEIGHT + USER_GROUP_PADDING };
            }

            nodes[idx] = {
              ...memberNode,
              parentId: group.id,
              extent: "parent" as const,
              position: relativePosition,
              style: { ...memberNode.style, transition: "opacity 150ms ease 50ms" },
            };
            userGroupChildren.push(nodes[idx]);

            // Also reparent any synthetic auto-group container whose anchor is this member
            const agId = anchorToAppGroup.get(nodeId);
            if (agId && !collapsedApps.has(agId)) {
              const agIdx = nodeIndexById.get(agId);
              if (agIdx !== undefined) {
                const agNode = nodes[agIdx];

                let agRelPos: { x: number; y: number };
                if (agNode.parentId === group.id) {
                  agRelPos = agNode.position;
                } else {
                  agRelPos = {
                    x: agNode.position.x - anchorAbsPosition.x + USER_GROUP_PADDING,
                    y: agNode.position.y - anchorAbsPosition.y + USER_GROUP_HEADER_HEIGHT + USER_GROUP_PADDING,
                  };
                }

                nodes[agIdx] = {
                  ...agNode,
                  parentId: group.id,
                  extent: "parent" as const,
                  position: agRelPos,
                };
                userGroupChildren.push(nodes[agIdx]);
              }
            }
          }

          // Size the user group container to fit its children
          let maxX = 0;
          let maxY = 0;
          for (const child of userGroupChildren) {
            const measured = prevMap.get(child.id)?.measured;
            const childW = measured?.width ?? (child.style?.width as number | undefined) ?? APPROX_NODE_WIDTH;
            const childH = measured?.height ?? (child.style?.height as number | undefined) ?? APPROX_NODE_HEIGHT;
            maxX = Math.max(maxX, child.position.x + childW);
            maxY = Math.max(maxY, child.position.y + childH);
          }

          nodes[containerIdx] = {
            ...nodes[containerIdx],
            style: {
              ...nodes[containerIdx].style,
              width: Math.max(MIN_GROUP_WIDTH, maxX + USER_GROUP_PADDING),
              height: Math.max(MIN_GROUP_HEIGHT, maxY + USER_GROUP_PADDING),
            },
          };
        }
      }

      // React Flow requires parent nodes before children in the array
      nodes.sort((a, b) => {
        const aHasParent = a.parentId ? 1 : 0;
        const bHasParent = b.parentId ? 1 : 0;
        return aHasParent - bHasParent;
      });

      return nodes;
    });
  }, [
    workflow.nodes,
    workflow.edges,
    workflow.groups,
    activeNode,
    onDeleteNodes,
    collapsedApps,
    appGroups,
    nodeToAppGroup,
    appGroupMeta,
    toggleAppCollapse,
    agentRunCollapsed,
    runTraces,
    toggleAgentRunCollapsed,
    collapsedUserGroups,
    nodeToUserGroup,
    userGroupMeta,
    toggleUserGroupCollapse,
    renamingGroupId,
    onRenameConfirm,
    onRenameCancel,
    setRfNodes,
  ]);
}
