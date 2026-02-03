use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Default for Workflow {
    fn default() -> Self {
        let start_id = Uuid::new_v4();
        let end_id = Uuid::new_v4();

        Self {
            id: Uuid::new_v4(),
            name: "New Workflow".to_string(),
            nodes: vec![
                Node {
                    id: start_id,
                    kind: NodeKind::Start,
                    position: Position { x: 100.0, y: 200.0 },
                    name: "Start".to_string(),
                    params: NodeParams::default(),
                },
                Node {
                    id: end_id,
                    kind: NodeKind::End,
                    position: Position { x: 500.0, y: 200.0 },
                    name: "End".to_string(),
                    params: NodeParams::default(),
                },
            ],
            edges: vec![Edge {
                from: start_id,
                to: end_id,
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: Uuid,
    pub kind: NodeKind,
    pub position: Position,
    pub name: String,
    pub params: NodeParams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Start,
    Step,
    End,
}

impl NodeKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            NodeKind::Start => "Start",
            NodeKind::Step => "Step",
            NodeKind::End => "End",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeParams {
    /// LLM instruction for this step
    pub prompt: String,
    /// Button text to find and click
    pub button_text: Option<String>,
    /// Path to image asset (relative to project assets/)
    pub image_path: Option<String>,
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Maximum tool calls before stopping
    pub max_tool_calls: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: Uuid,
    pub to: Uuid,
}

impl Workflow {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn add_node(
        &mut self,
        kind: NodeKind,
        position: Position,
        name: impl Into<String>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        self.nodes.push(Node {
            id,
            kind,
            position,
            name: name.into(),
            params: NodeParams::default(),
        });
        id
    }

    pub fn add_edge(&mut self, from: Uuid, to: Uuid) {
        self.edges.push(Edge { from, to });
    }

    pub fn find_node(&self, id: Uuid) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn find_node_mut(&mut self, id: Uuid) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn remove_node(&mut self, id: Uuid) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
    }

    pub fn remove_edge(&mut self, from: Uuid, to: Uuid) {
        self.edges.retain(|e| !(e.from == from && e.to == to));
    }

    /// Get execution order as a linear path from Start to End
    pub fn execution_order(&self) -> Vec<Uuid> {
        let start = self.nodes.iter().find(|n| n.kind == NodeKind::Start);
        let Some(start) = start else {
            return vec![];
        };

        let mut order = vec![start.id];
        let mut current = start.id;

        loop {
            let next = self.edges.iter().find(|e| e.from == current);
            match next {
                Some(edge) => {
                    order.push(edge.to);
                    current = edge.to;

                    // Stop if we hit End node
                    if self
                        .find_node(current)
                        .is_some_and(|n| n.kind == NodeKind::End)
                    {
                        break;
                    }
                }
                None => break,
            }
        }

        order
    }
}
