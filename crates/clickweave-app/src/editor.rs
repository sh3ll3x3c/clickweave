use crate::theme;
use clickweave_core::{NodeKind, Position, Workflow};
use eframe::egui::{self, Color32, RichText, Stroke};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer};
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use std::collections::HashMap;
use uuid::Uuid;

/// Node data for the snarl graph
#[derive(Clone)]
pub struct GraphNode {
    pub workflow_id: Uuid,
    pub kind: NodeKind,
    pub name: String,
}

pub struct WorkflowEditor {
    snarl: Snarl<GraphNode>,
    uuid_to_snarl: HashMap<Uuid, NodeId>,
    snarl_to_uuid: HashMap<NodeId, Uuid>,
    style: SnarlStyle,
    positions: HashMap<NodeId, egui::Pos2>,
}

pub struct EditorResponse {
    pub selected_node: Option<Uuid>,
}

impl WorkflowEditor {
    pub fn new() -> Self {
        Self {
            snarl: Snarl::new(),
            uuid_to_snarl: HashMap::new(),
            snarl_to_uuid: HashMap::new(),
            style: create_n8n_snarl_style(),
            positions: HashMap::new(),
        }
    }

    pub fn sync_from_workflow(&mut self, workflow: &Workflow) {
        self.snarl = Snarl::new();
        self.uuid_to_snarl.clear();
        self.snarl_to_uuid.clear();
        self.positions.clear();

        for node in &workflow.nodes {
            let graph_node = GraphNode {
                workflow_id: node.id,
                kind: node.kind,
                name: node.name.clone(),
            };

            let pos = egui::pos2(node.position.x, node.position.y);
            let snarl_id = self.snarl.insert_node(pos, graph_node);

            self.uuid_to_snarl.insert(node.id, snarl_id);
            self.snarl_to_uuid.insert(snarl_id, node.id);
            self.positions.insert(snarl_id, pos);
        }

        for edge in &workflow.edges {
            if let (Some(&from_snarl), Some(&to_snarl)) = (
                self.uuid_to_snarl.get(&edge.from),
                self.uuid_to_snarl.get(&edge.to),
            ) {
                let out_pin = OutPinId {
                    node: from_snarl,
                    output: 0,
                };
                let in_pin = InPinId {
                    node: to_snarl,
                    input: 0,
                };
                self.snarl.connect(out_pin, in_pin);
            }
        }
    }

    pub fn sync_to_workflow(&self, workflow: &mut Workflow) {
        for (&uuid, &snarl_id) in &self.uuid_to_snarl {
            if let Some(&pos) = self.positions.get(&snarl_id) {
                if let Some(workflow_node) = workflow.find_node_mut(uuid) {
                    workflow_node.position = Position { x: pos.x, y: pos.y };
                }
            }
        }

        workflow.edges.clear();
        for (&snarl_id, _) in &self.snarl_to_uuid {
            let out_pin = OutPinId {
                node: snarl_id,
                output: 0,
            };

            for in_pin in self.snarl.out_pin(out_pin).remotes.iter() {
                if let Some(&from_uuid) = self.snarl_to_uuid.get(&snarl_id) {
                    if let Some(&to_uuid) = self.snarl_to_uuid.get(&in_pin.node) {
                        workflow.add_edge(from_uuid, to_uuid);
                    }
                }
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workflow: &mut Workflow) -> EditorResponse {
        let mut selected = None;

        for (&uuid, &snarl_id) in &self.uuid_to_snarl {
            if let Some(workflow_node) = workflow.find_node(uuid) {
                if let Some(graph_node) = self.snarl.get_node_mut(snarl_id) {
                    graph_node.name = workflow_node.name.clone();
                }
            }
        }

        let mut viewer = WorkflowViewer {
            selected: &mut selected,
            snarl_to_uuid: &self.snarl_to_uuid,
            positions: &mut self.positions,
        };

        self.snarl
            .show(&mut viewer, &self.style, "workflow_graph", ui);

        self.sync_to_workflow(workflow);

        EditorResponse {
            selected_node: selected,
        }
    }
}

fn create_n8n_snarl_style() -> SnarlStyle {
    SnarlStyle::new()
}

struct WorkflowViewer<'a> {
    selected: &'a mut Option<Uuid>,
    snarl_to_uuid: &'a HashMap<NodeId, Uuid>,
    positions: &'a mut HashMap<NodeId, egui::Pos2>,
}

impl SnarlViewer<GraphNode> for WorkflowViewer<'_> {
    fn title(&mut self, node: &GraphNode) -> String {
        // Icon + name for header
        let icon = match node.kind {
            NodeKind::Start => "▶",
            NodeKind::Step => "⚡",
            NodeKind::End => "⏹",
        };
        format!("{} {}", icon, node.name)
    }

    fn outputs(&mut self, node: &GraphNode) -> usize {
        match node.kind {
            NodeKind::End => 0,
            _ => 1,
        }
    }

    fn inputs(&mut self, node: &GraphNode) -> usize {
        match node.kind {
            NodeKind::Start => 0,
            _ => 1,
        }
    }

    fn show_input(
        &mut self,
        pin: &egui_snarl::InPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<GraphNode>,
    ) -> PinInfo {
        let connected = !pin.remotes.is_empty();
        let fill = if connected {
            theme::ACCENT_GREEN
        } else {
            Color32::from_rgb(80, 80, 80)
        };
        PinInfo::circle()
            .with_fill(fill)
            .with_stroke(Stroke::new(2.0, Color32::from_rgb(60, 60, 60)))
    }

    fn show_output(
        &mut self,
        pin: &egui_snarl::OutPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<GraphNode>,
    ) -> PinInfo {
        let connected = !pin.remotes.is_empty();
        let fill = if connected {
            theme::ACCENT_CORAL
        } else {
            Color32::from_rgb(80, 80, 80)
        };
        PinInfo::circle()
            .with_fill(fill)
            .with_stroke(Stroke::new(2.0, Color32::from_rgb(60, 60, 60)))
    }

    fn has_body(&mut self, _node: &GraphNode) -> bool {
        true
    }

    fn show_body(
        &mut self,
        node_id: NodeId,
        _inputs: &[egui_snarl::InPin],
        _outputs: &[egui_snarl::OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphNode>,
    ) {
        let node = &snarl[node_id];

        let (type_label, color) = match node.kind {
            NodeKind::Start => ("Trigger", theme::NODE_START),
            NodeKind::Step => ("Action", theme::NODE_STEP),
            NodeKind::End => ("End", theme::NODE_END),
        };

        // Type label
        ui.label(RichText::new(type_label).size(11.0).color(color));

        // Clickable area for selection
        let response = ui.allocate_response(egui::vec2(120.0, 4.0), egui::Sense::click());

        if response.clicked() {
            if let Some(&uuid) = self.snarl_to_uuid.get(&node_id) {
                *self.selected = Some(uuid);
            }
        }
    }

    fn connect(
        &mut self,
        from: &egui_snarl::OutPin,
        to: &egui_snarl::InPin,
        snarl: &mut Snarl<GraphNode>,
    ) {
        for remote in to.remotes.iter() {
            snarl.disconnect(*remote, to.id);
        }
        snarl.connect(from.id, to.id);
    }

    fn disconnect(
        &mut self,
        from: &egui_snarl::OutPin,
        to: &egui_snarl::InPin,
        snarl: &mut Snarl<GraphNode>,
    ) {
        snarl.disconnect(from.id, to.id);
    }
}
