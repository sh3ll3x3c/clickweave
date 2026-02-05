use crate::editor::WorkflowEditor;
use crate::executor::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use crate::theme::{
    self, ACCENT_CORAL, ACCENT_GREEN, BG_DARK, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use clickweave_core::{NodeCategory, NodeType, Workflow, validate_workflow};
use clickweave_llm::LlmConfig;
use eframe::egui::{self, Align, Align2, Button, Color32, Layout, RichText, TextureHandle, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

pub struct ClickweaveApp {
    workflow: Workflow,
    editor: WorkflowEditor,
    project_path: Option<PathBuf>,

    // Executor
    command_tx: Option<mpsc::Sender<ExecutorCommand>>,
    event_rx: Option<mpsc::Receiver<ExecutorEvent>>,
    executor_state: ExecutorState,

    // Settings
    llm_config: LlmConfig,
    mcp_command: String,

    // UI state
    show_settings: bool,
    logs: Vec<String>,
    selected_node: Option<uuid::Uuid>,
    active_node: Option<uuid::Uuid>,

    // n8n-style UI state
    sidebar_collapsed: bool,
    logs_drawer_open: bool,
    node_search: String,
    is_active: bool,

    // Image preview cache (used in setup tab)
    #[allow(dead_code)]
    texture_cache: HashMap<String, TextureHandle>,
}

impl ClickweaveApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let workflow = Workflow::default();
        let mut editor = WorkflowEditor::new();
        editor.sync_from_workflow(&workflow);

        Self {
            workflow,
            editor,
            project_path: None,
            command_tx: None,
            event_rx: None,
            executor_state: ExecutorState::Idle,
            llm_config: LlmConfig::default(),
            mcp_command: "npx".to_string(),
            show_settings: false,
            logs: vec!["Clickweave started".to_string()],
            selected_node: None,
            active_node: None,
            sidebar_collapsed: false,
            logs_drawer_open: false,
            node_search: String::new(),
            is_active: false,
            texture_cache: HashMap::new(),
        }
    }

    fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 1000 {
            self.logs.remove(0);
        }
    }

    fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::info!("{}", msg);
        self.push_log(msg);
    }

    fn save_workflow(&mut self) {
        if let Some(path) = &self.project_path {
            let workflow_path = path.join("workflow.json");
            match serde_json::to_string_pretty(&self.workflow) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&workflow_path, json) {
                        self.log(format!("Failed to save: {}", e));
                    } else {
                        self.log(format!("Saved to {:?}", workflow_path));
                    }
                }
                Err(e) => self.log(format!("Failed to serialize: {}", e)),
            }
        } else {
            self.save_workflow_as();
        }
    }

    fn save_workflow_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save Project Folder")
            .pick_folder()
        {
            let assets_dir = path.join("assets");
            let _ = std::fs::create_dir_all(&assets_dir);
            self.project_path = Some(path);
            self.save_workflow();
        }
    }

    fn open_workflow(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open Project Folder")
            .pick_folder()
        {
            let workflow_path = path.join("workflow.json");
            match std::fs::read_to_string(&workflow_path) {
                Ok(json) => match serde_json::from_str::<Workflow>(&json) {
                    Ok(workflow) => {
                        self.workflow = workflow;
                        self.project_path = Some(path);
                        self.editor.sync_from_workflow(&self.workflow);
                        self.log("Workflow loaded");
                    }
                    Err(e) => self.log(format!("Failed to parse workflow: {}", e)),
                },
                Err(e) => self.log(format!("Failed to read workflow: {}", e)),
            }
        }
    }

    fn run_workflow(&mut self) {
        self.editor.sync_to_workflow(&mut self.workflow);

        if let Err(e) = validate_workflow(&self.workflow) {
            self.log(format!("Validation failed: {}", e));
            return;
        }

        self.log("Starting workflow execution...");
        self.executor_state = ExecutorState::Running;
        self.active_node = None;
        self.logs_drawer_open = true;

        let workflow = self.workflow.clone();
        let llm_config = self.llm_config.clone();
        let mcp_command = self.mcp_command.clone();
        let project_path = self.project_path.clone();

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        self.command_tx = Some(cmd_tx);
        self.event_rx = Some(event_rx);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut executor = WorkflowExecutor::new(
                    workflow,
                    llm_config,
                    mcp_command,
                    project_path,
                    event_tx,
                );
                executor.run(cmd_rx).await;
            });
        });
    }

    fn stop_workflow(&mut self) {
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(ExecutorCommand::Stop);
        }
        self.executor_state = ExecutorState::Idle;
        self.active_node = None;
        self.log("Workflow stopped");
    }

    fn poll_executor_events(&mut self) {
        let Some(rx) = self.event_rx.take() else {
            return;
        };

        while let Ok(event) = rx.try_recv() {
            match event {
                ExecutorEvent::Log(msg) => self.push_log(msg),
                ExecutorEvent::Error(msg) => self.push_log(format!("ERROR: {}", msg)),
                ExecutorEvent::StateChanged(state) => {
                    self.executor_state = state;
                    if self.executor_state == ExecutorState::Idle {
                        self.active_node = None;
                        self.command_tx = None;
                        return;
                    }
                }
                ExecutorEvent::NodeStarted(id) => {
                    self.active_node = Some(id);
                }
                ExecutorEvent::NodeFailed(id, msg) => {
                    self.active_node = None;
                    self.push_log(format!("Node {} failed: {}", id, msg));
                }
                ExecutorEvent::NodeCompleted(_) | ExecutorEvent::WorkflowCompleted => {
                    self.active_node = None;
                }
            }
        }

        // Put the receiver back if we didn't go idle
        self.event_rx = Some(rx);
    }

    fn add_node(&mut self, node_type: NodeType) {
        let offset = self.workflow.nodes.len() as f32 * 50.0;
        let id = self.workflow.add_node(
            node_type,
            clickweave_core::Position {
                x: 300.0 + offset,
                y: 200.0 + offset,
            },
        );
        self.editor.sync_from_workflow(&self.workflow);
        self.selected_node = Some(id);
    }

    fn show_sidebar(&mut self, ctx: &egui::Context) {
        let sidebar_width = if self.sidebar_collapsed { 48.0 } else { 200.0 };

        egui::SidePanel::left("sidebar")
            .frame(theme::sidebar_frame())
            .exact_width(sidebar_width)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(8.0);

                // Navigation items
                let nav_items = [
                    ("🏠", "Home"),
                    ("📋", "Templates"),
                    ("📊", "Variables"),
                    ("📜", "Executions"),
                    ("❓", "Help"),
                ];

                let mut clicked_nav: Option<&str> = None;

                for (icon, label) in nav_items {
                    ui.add_space(2.0);
                    let btn = if self.sidebar_collapsed {
                        ui.add_sized(
                            [40.0, 36.0],
                            Button::new(RichText::new(icon).size(18.0)).frame(false),
                        )
                    } else {
                        ui.add_sized(
                            [sidebar_width - 16.0, 36.0],
                            Button::new(RichText::new(format!("{}  {}", icon, label)).size(14.0))
                                .frame(false),
                        )
                    };
                    if btn.on_hover_text(label).clicked() {
                        clicked_nav = Some(label);
                    }
                }

                match clicked_nav {
                    Some("Home") => {
                        self.selected_node = None;
                    }
                    Some("Executions") => {
                        self.logs_drawer_open = !self.logs_drawer_open;
                    }
                    Some("Help") => {
                        self.show_settings = true;
                    }
                    Some("Variables") => {
                        self.logs_drawer_open = true;
                        self.push_log("Variables panel not yet implemented".to_string());
                    }
                    Some("Templates") => {
                        self.push_log("Templates panel not yet implemented".to_string());
                    }
                    _ => {}
                }

                // Collapse toggle at bottom
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.add_space(12.0);
                    let collapse_icon = if self.sidebar_collapsed { "▶" } else { "◀" };
                    if ui
                        .add(Button::new(collapse_icon).frame(false))
                        .on_hover_text(if self.sidebar_collapsed {
                            "Expand sidebar"
                        } else {
                            "Collapse sidebar"
                        })
                        .clicked()
                    {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                    ui.add_space(8.0);

                    // Workflow stats
                    if !self.sidebar_collapsed {
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("{} nodes", self.workflow.nodes.len()))
                                .size(11.0)
                                .color(TEXT_MUTED),
                        );
                    }
                });
            });
    }

    fn show_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(theme::header_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Workflow name (editable)
                    ui.add(
                        egui::TextEdit::singleline(&mut self.workflow.name)
                            .font(egui::TextStyle::Heading)
                            .text_color(TEXT_PRIMARY)
                            .desired_width(200.0)
                            .frame(false),
                    );

                    ui.add_space(8.0);
                    ui.label(RichText::new("+ Add tag").size(12.0).color(TEXT_MUTED));

                    ui.add_space(32.0);

                    // Editor / Executions tabs
                    let _ = ui.selectable_label(!self.logs_drawer_open, "Editor");
                    if ui
                        .selectable_label(self.logs_drawer_open, "Executions")
                        .clicked()
                    {
                        self.logs_drawer_open = !self.logs_drawer_open;
                    }

                    // Right side controls
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);

                        // More menu (rightmost)
                        ui.menu_button(RichText::new("☰").size(16.0), |ui| {
                            if ui.button("Settings").clicked() {
                                self.show_settings = !self.show_settings;
                                ui.close();
                            }
                            if ui.button("New").clicked() {
                                self.workflow = Workflow::default();
                                self.editor.sync_from_workflow(&self.workflow);
                                self.project_path = None;
                                self.selected_node = None;
                                ui.close();
                            }
                            if ui.button("Open...").clicked() {
                                self.open_workflow();
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Reset Workflow").clicked() {
                                let name = self.workflow.name.clone();
                                self.workflow = Workflow::default();
                                self.workflow.name = name;
                                self.editor.sync_from_workflow(&self.workflow);
                                self.selected_node = None;
                                self.log("Workflow reset to initial state");
                                ui.close();
                            }
                        });

                        // Save button
                        let save_btn = Button::new(RichText::new("Save").color(Color32::WHITE))
                            .fill(ACCENT_CORAL)
                            .corner_radius(6.0);
                        if ui.add(save_btn).clicked() {
                            self.save_workflow();
                        }

                        ui.add_space(16.0);

                        // Active/Inactive toggle
                        let toggle_text = if self.is_active { "Active" } else { "Inactive" };
                        let toggle_color = if self.is_active {
                            ACCENT_GREEN
                        } else {
                            TEXT_MUTED
                        };
                        if ui
                            .add(
                                Button::new(RichText::new(toggle_text).color(toggle_color))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.is_active = !self.is_active;
                        }
                    });
                });
            });
    }

    fn show_inspector(&mut self, ctx: &egui::Context) {
        let mut should_delete_node: Option<uuid::Uuid> = None;

        egui::SidePanel::right("inspector")
            .frame(theme::inspector_frame())
            .exact_width(300.0)
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(node_id) = self.selected_node {
                    if let Some(node) = self.workflow.find_node_mut(node_id) {
                        let category = node.node_type.category();
                        let color = theme::category_color(category);
                        let icon = node.node_type.icon().to_string();
                        let type_name = node.node_type.display_name().to_string();

                        // Header with node type
                        ui.horizontal(|ui| {
                            ui.colored_label(color, RichText::new(&icon).size(20.0));
                            ui.add_space(8.0);
                            ui.heading(&node.name);
                        });

                        ui.add_space(4.0);
                        ui.label(RichText::new(&type_name).size(12.0).color(TEXT_MUTED));

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(12.0);

                        // Node name
                        ui.label(RichText::new("Name").size(12.0).color(TEXT_SECONDARY));
                        ui.add_space(4.0);
                        ui.text_edit_singleline(&mut node.name);

                        ui.add_space(16.0);

                        // Enabled toggle
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Enabled").size(12.0).color(TEXT_SECONDARY));
                            ui.checkbox(&mut node.enabled, "");
                        });

                        ui.add_space(12.0);

                        // Per-type basic fields
                        match &mut node.node_type {
                            NodeType::AiStep(params) => {
                                ui.label(RichText::new("Prompt").size(12.0).color(TEXT_SECONDARY));
                                ui.add_space(4.0);
                                ui.add(
                                    egui::TextEdit::multiline(&mut params.prompt)
                                        .desired_rows(5)
                                        .desired_width(ui.available_width()),
                                );

                                ui.add_space(16.0);

                                // Button text
                                ui.label(
                                    RichText::new("Button text (optional)")
                                        .size(12.0)
                                        .color(TEXT_SECONDARY),
                                );
                                ui.add_space(4.0);
                                let mut btn_text = params.button_text.clone().unwrap_or_default();
                                if ui.text_edit_singleline(&mut btn_text).changed() {
                                    params.button_text = if btn_text.is_empty() {
                                        None
                                    } else {
                                        Some(btn_text)
                                    };
                                }

                                ui.add_space(16.0);

                                // Max tool calls
                                ui.label(
                                    RichText::new("Max tool calls")
                                        .size(12.0)
                                        .color(TEXT_SECONDARY),
                                );
                                ui.add_space(4.0);
                                let mut max_calls = params.max_tool_calls.unwrap_or(10);
                                if ui
                                    .add(egui::DragValue::new(&mut max_calls).range(1..=100))
                                    .changed()
                                {
                                    params.max_tool_calls = Some(max_calls);
                                }
                            }
                            _ => {
                                ui.label(
                                    RichText::new(format!(
                                        "Setup for {} coming in detail view",
                                        type_name
                                    ))
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                                );
                            }
                        }

                        ui.add_space(16.0);

                        // Timeout
                        ui.label(
                            RichText::new("Timeout (ms, 0 = none)")
                                .size(12.0)
                                .color(TEXT_SECONDARY),
                        );
                        ui.add_space(4.0);
                        let mut timeout = node.timeout_ms.unwrap_or(0);
                        if ui
                            .add(
                                egui::DragValue::new(&mut timeout)
                                    .range(0..=300000)
                                    .speed(100),
                            )
                            .changed()
                        {
                            node.timeout_ms = if timeout == 0 { None } else { Some(timeout) };
                        }

                        // Delete button
                        ui.add_space(24.0);
                        let delete_btn =
                            Button::new(RichText::new("🗑 Delete Node").color(theme::NODE_END))
                                .frame(false);
                        if ui.add(delete_btn).clicked() {
                            should_delete_node = Some(node_id);
                        }
                    } else {
                        self.selected_node = None;
                    }
                } else {
                    // Node palette when nothing selected
                    self.show_node_palette_inline(ui);
                }
            });

        // Handle deferred deletion
        if let Some(node_id) = should_delete_node {
            self.workflow.remove_node(node_id);
            self.editor.sync_from_workflow(&self.workflow);
            self.selected_node = None;
        }
    }

    fn show_node_palette_inline(&mut self, ui: &mut egui::Ui) {
        ui.heading("Add Node");
        ui.add_space(8.0);

        // Search box
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.node_search).hint_text("Search nodes..."));
        });

        ui.add_space(16.0);

        let search = self.node_search.to_lowercase();
        let mut node_to_add: Option<NodeType> = None;

        let categories = [
            NodeCategory::Ai,
            NodeCategory::Vision,
            NodeCategory::Input,
            NodeCategory::Window,
            NodeCategory::AppDebugKit,
        ];

        for cat in categories {
            let defaults: Vec<NodeType> = NodeType::all_defaults()
                .into_iter()
                .filter(|nt| nt.category() == cat)
                .filter(|nt| {
                    search.is_empty() || nt.display_name().to_lowercase().contains(&search)
                })
                .collect();

            if defaults.is_empty() {
                continue;
            }

            let color = theme::category_color(cat);
            let header = format!("{} {}", cat.icon(), cat.display_name());
            ui.collapsing(RichText::new(header).color(color), |ui| {
                ui.add_space(4.0);
                for nt in defaults {
                    let label = format!("{} {}", nt.icon(), nt.display_name());
                    if ui
                        .add(
                            Button::new(label)
                                .frame(false)
                                .min_size(Vec2::new(ui.available_width(), 28.0)),
                        )
                        .clicked()
                    {
                        node_to_add = Some(nt);
                    }
                }
            });
        }

        if let Some(nt) = node_to_add {
            self.add_node(nt);
        }
    }

    fn show_floating_toolbar(&mut self, ctx: &egui::Context) {
        let mut node_to_add: Option<NodeType> = None;

        egui::Area::new(egui::Id::new("floating_toolbar"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
            .show(ctx, |ui| {
                theme::floating_toolbar_frame().show(ui, |ui| {
                    let btn_size = Vec2::new(32.0, 32.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        // Add node menu
                        ui.menu_button(RichText::new("➕").size(16.0), |ui| {
                            let categories = [
                                NodeCategory::Ai,
                                NodeCategory::Vision,
                                NodeCategory::Input,
                                NodeCategory::Window,
                                NodeCategory::AppDebugKit,
                            ];

                            for cat in categories {
                                let color = theme::category_color(cat);
                                let header = format!("{} {}", cat.icon(), cat.display_name());
                                ui.menu_button(RichText::new(header).color(color), |ui| {
                                    for nt in NodeType::all_defaults()
                                        .into_iter()
                                        .filter(|nt| nt.category() == cat)
                                    {
                                        let label = format!("{} {}", nt.icon(), nt.display_name());
                                        if ui.button(label).clicked() {
                                            node_to_add = Some(nt);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });

                        // Logs toggle
                        if ui
                            .add_sized(
                                btn_size,
                                Button::new(RichText::new("📜").size(16.0)).frame(false),
                            )
                            .on_hover_text("Toggle logs")
                            .clicked()
                        {
                            self.logs_drawer_open = !self.logs_drawer_open;
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Test workflow button
                        let is_running = matches!(self.executor_state, ExecutorState::Running);
                        if is_running {
                            let stop_btn = Button::new(
                                RichText::new("⏹ Stop").size(14.0).color(Color32::WHITE),
                            )
                            .fill(theme::NODE_END)
                            .corner_radius(6.0)
                            .min_size(Vec2::new(140.0, 32.0));
                            if ui.add(stop_btn).clicked() {
                                self.stop_workflow();
                            }
                        } else {
                            let test_btn = Button::new(
                                RichText::new("▶ Test workflow")
                                    .size(14.0)
                                    .color(Color32::WHITE),
                            )
                            .fill(ACCENT_CORAL)
                            .corner_radius(6.0)
                            .min_size(Vec2::new(140.0, 32.0));
                            if ui.add(test_btn).clicked() {
                                self.run_workflow();
                            }
                        }
                    });
                });
            });

        if let Some(nt) = node_to_add {
            self.add_node(nt);
        }
    }

    fn show_logs_drawer(&mut self, ctx: &egui::Context) {
        if !self.logs_drawer_open {
            return;
        }

        egui::TopBottomPanel::bottom("logs_drawer")
            .frame(theme::logs_drawer_frame())
            .resizable(true)
            .default_height(180.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Logs");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            self.logs_drawer_open = false;
                        }
                        if ui.button("Clear").clicked() {
                            self.logs.clear();
                        }
                    });
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for log in &self.logs {
                            ui.label(RichText::new(log).size(12.0).color(TEXT_SECONDARY));
                        }
                    });
            });
    }

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.heading("LLM Configuration");
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Base URL:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.llm_config.base_url)
                            .desired_width(250.0),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Model:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.llm_config.model).desired_width(250.0),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("API Key:");
                    let mut key = self.llm_config.api_key.clone().unwrap_or_default();
                    if ui
                        .add(egui::TextEdit::singleline(&mut key).desired_width(250.0))
                        .changed()
                    {
                        self.llm_config.api_key = if key.is_empty() { None } else { Some(key) };
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                ui.heading("MCP Configuration");
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Command:");
                    ui.add(egui::TextEdit::singleline(&mut self.mcp_command).desired_width(250.0));
                });
                ui.label(
                    RichText::new("Use 'npx' for npx -y native-devtools-mcp")
                        .size(11.0)
                        .color(TEXT_MUTED),
                );

                ui.add_space(16.0);

                if ui.button("Close").clicked() {
                    self.show_settings = false;
                }
            });
    }
}

impl eframe::App for ClickweaveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll executor events
        self.poll_executor_events();

        // Keyboard shortcuts
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::S) {
                self.editor.sync_to_workflow(&mut self.workflow);
                self.save_workflow();
            }
        });

        // Show panels in order
        self.show_header(ctx);
        self.show_sidebar(ctx);
        self.show_inspector(ctx);
        self.show_logs_drawer(ctx);

        // Center: Graph editor (must be last for CentralPanel)
        let active_node = self.active_node;
        let selected_node = self.selected_node;
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: BG_DARK,
                ..Default::default()
            })
            .show(ctx, |ui| {
                let response = self
                    .editor
                    .show(ui, &mut self.workflow, active_node, selected_node);
                if let Some(selected) = response.selected_node {
                    self.selected_node = Some(selected);
                }
                if let Some(deleted) = response.deleted_node {
                    self.workflow.remove_node(deleted);
                    self.editor.sync_from_workflow(&self.workflow);
                    if self.selected_node == Some(deleted) {
                        self.selected_node = None;
                    }
                }
                if response.canvas_clicked {
                    self.selected_node = None;
                }
            });

        // Floating toolbar (rendered on top)
        self.show_floating_toolbar(ctx);

        // Settings window
        self.show_settings_window(ctx);

        // Continuous repaint while running
        if matches!(self.executor_state, ExecutorState::Running) {
            ctx.request_repaint();
        }
    }
}
