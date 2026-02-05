use serde::{Deserialize, Serialize};
use serde_json::Value;
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
        Self {
            id: Uuid::new_v4(),
            name: "New Workflow".to_string(),
            nodes: vec![],
            edges: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: Uuid,
    pub node_type: NodeType,
    pub position: Position,
    pub name: String,
    pub enabled: bool,
    pub timeout_ms: Option<u64>,
    pub retries: u32,
    pub trace_level: TraceLevel,
    pub expected_outcome: Option<String>,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
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

    pub fn add_node(&mut self, node_type: NodeType, position: Position) -> Uuid {
        let id = Uuid::new_v4();
        let name = node_type.default_name().to_string();
        self.nodes.push(Node {
            id,
            node_type,
            position,
            name,
            enabled: true,
            timeout_ms: None,
            retries: 0,
            trace_level: TraceLevel::Minimal,
            expected_outcome: None,
            checks: vec![],
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

    /// Find entry points: nodes with no incoming edges.
    fn entry_points(&self) -> Vec<Uuid> {
        let targets: std::collections::HashSet<Uuid> = self.edges.iter().map(|e| e.to).collect();
        self.nodes
            .iter()
            .filter(|n| !targets.contains(&n.id))
            .map(|n| n.id)
            .collect()
    }

    /// Get execution order by walking edges from entry points linearly.
    pub fn execution_order(&self) -> Vec<Uuid> {
        let entries = self.entry_points();
        if entries.is_empty() {
            return self.nodes.iter().map(|n| n.id).collect();
        }

        let mut order = Vec::new();
        let mut visited = std::collections::HashSet::new();

        for entry in entries {
            let mut current = entry;
            loop {
                if !visited.insert(current) {
                    break;
                }
                order.push(current);

                let next = self.edges.iter().find(|e| e.from == current);
                match next {
                    Some(edge) => current = edge.to,
                    None => break,
                }
            }
        }

        order
    }
}

// =============================================================================
// New Node Type System
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeCategory {
    Ai,
    Vision,
    Input,
    Window,
    AppDebugKit,
}

impl NodeCategory {
    pub fn display_name(&self) -> &'static str {
        match self {
            NodeCategory::Ai => "AI",
            NodeCategory::Vision => "Vision / Discovery",
            NodeCategory::Input => "Input",
            NodeCategory::Window => "Window",
            NodeCategory::AppDebugKit => "AppDebugKit",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            NodeCategory::Ai => "🤖",
            NodeCategory::Vision => "👁",
            NodeCategory::Input => "🖱",
            NodeCategory::Window => "🪟",
            NodeCategory::AppDebugKit => "🔧",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeType {
    AiStep(AiStepParams),
    TakeScreenshot(TakeScreenshotParams),
    FindText(FindTextParams),
    FindImage(FindImageParams),
    Click(ClickParams),
    TypeText(TypeTextParams),
    Scroll(ScrollParams),
    ListWindows(ListWindowsParams),
    FocusWindow(FocusWindowParams),
    AppDebugKitOp(AppDebugKitParams),
}

impl NodeType {
    pub fn category(&self) -> NodeCategory {
        match self {
            NodeType::AiStep(_) => NodeCategory::Ai,
            NodeType::TakeScreenshot(_) | NodeType::FindText(_) | NodeType::FindImage(_) => {
                NodeCategory::Vision
            }
            NodeType::Click(_) | NodeType::TypeText(_) | NodeType::Scroll(_) => NodeCategory::Input,
            NodeType::ListWindows(_) | NodeType::FocusWindow(_) => NodeCategory::Window,
            NodeType::AppDebugKitOp(_) => NodeCategory::AppDebugKit,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            NodeType::AiStep(_) => "AI Step",
            NodeType::TakeScreenshot(_) => "Take Screenshot",
            NodeType::FindText(_) => "Find Text",
            NodeType::FindImage(_) => "Find Image",
            NodeType::Click(_) => "Click",
            NodeType::TypeText(_) => "Type Text",
            NodeType::Scroll(_) => "Scroll",
            NodeType::ListWindows(_) => "List Windows",
            NodeType::FocusWindow(_) => "Focus Window",
            NodeType::AppDebugKitOp(_) => "AppDebugKit Op",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            NodeType::AiStep(_) => "🤖",
            NodeType::TakeScreenshot(_) => "📸",
            NodeType::FindText(_) => "🔍",
            NodeType::FindImage(_) => "🖼",
            NodeType::Click(_) => "🖱",
            NodeType::TypeText(_) => "⌨",
            NodeType::Scroll(_) => "📜",
            NodeType::ListWindows(_) => "📋",
            NodeType::FocusWindow(_) => "🪟",
            NodeType::AppDebugKitOp(_) => "🔧",
        }
    }

    pub fn default_name(&self) -> &'static str {
        self.display_name()
    }

    pub fn is_deterministic(&self) -> bool {
        !matches!(self, NodeType::AiStep(_))
    }

    /// All available node types with default parameters.
    pub fn all_defaults() -> Vec<NodeType> {
        vec![
            NodeType::AiStep(AiStepParams::default()),
            NodeType::TakeScreenshot(TakeScreenshotParams::default()),
            NodeType::FindText(FindTextParams::default()),
            NodeType::FindImage(FindImageParams::default()),
            NodeType::Click(ClickParams::default()),
            NodeType::TypeText(TypeTextParams::default()),
            NodeType::Scroll(ScrollParams::default()),
            NodeType::ListWindows(ListWindowsParams::default()),
            NodeType::FocusWindow(FocusWindowParams::default()),
            NodeType::AppDebugKitOp(AppDebugKitParams::default()),
        ]
    }
}

// =============================================================================
// Parameter structs
// =============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiStepParams {
    pub prompt: String,
    pub button_text: Option<String>,
    pub template_image: Option<String>,
    pub max_tool_calls: Option<u32>,
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeScreenshotParams {
    pub mode: ScreenshotMode,
    pub target: Option<String>,
    pub include_ocr: bool,
}

impl Default for TakeScreenshotParams {
    fn default() -> Self {
        Self {
            mode: ScreenshotMode::Screen,
            target: None,
            include_ocr: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotMode {
    Screen,
    Window,
    Region,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindTextParams {
    pub search_text: String,
    pub match_mode: MatchMode,
    pub scope: Option<String>,
    pub select_result: Option<String>,
}

impl Default for FindTextParams {
    fn default() -> Self {
        Self {
            search_text: String::new(),
            match_mode: MatchMode::Contains,
            scope: None,
            select_result: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchMode {
    Contains,
    Exact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindImageParams {
    pub template_image: Option<String>,
    pub threshold: f64,
    pub max_results: u32,
}

impl Default for FindImageParams {
    fn default() -> Self {
        Self {
            template_image: None,
            threshold: 0.88,
            max_results: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickParams {
    pub target: Option<String>,
    pub button: MouseButton,
    pub click_count: u32,
}

impl Default for ClickParams {
    fn default() -> Self {
        Self {
            target: None,
            button: MouseButton::Left,
            click_count: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeTextParams {
    pub text: String,
    pub press_enter: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrollParams {
    pub delta_y: i32,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListWindowsParams {
    pub app_name: Option<String>,
    pub title_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusWindowParams {
    pub method: FocusMethod,
    pub value: Option<String>,
    pub bring_to_front: bool,
}

impl Default for FocusWindowParams {
    fn default() -> Self {
        Self {
            method: FocusMethod::AppName,
            value: None,
            bring_to_front: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusMethod {
    WindowId,
    AppName,
    TitlePattern,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppDebugKitParams {
    pub operation_name: String,
    pub parameters: Value,
}

// =============================================================================
// Trace & Check types
// =============================================================================

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceLevel {
    Off,
    #[default]
    Minimal,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckType {
    TextPresent,
    TextAbsent,
    TemplateFound,
    WindowTitleMatches,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnCheckFail {
    #[default]
    FailNode,
    WarnOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub check_type: CheckType,
    pub params: Value,
    pub on_fail: OnCheckFail,
}

// =============================================================================
// Run types
// =============================================================================

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    #[default]
    Ok,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRun {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub status: RunStatus,
    pub trace_level: TraceLevel,
    pub events: Vec<TraceEvent>,
    pub artifacts: Vec<Artifact>,
    pub observed_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub timestamp: u64,
    pub event_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    Screenshot,
    Ocr,
    TemplateMatch,
    Log,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub artifact_id: Uuid,
    pub kind: ArtifactKind,
    pub path: String,
    pub metadata: Value,
    pub overlays: Vec<Value>,
}
