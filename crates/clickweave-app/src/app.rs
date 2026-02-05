use crate::editor::WorkflowEditor;
use crate::executor::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use crate::theme::{
    self, ACCENT_CORAL, ACCENT_GREEN, BG_DARK, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use clickweave_core::storage::RunStorage;
use clickweave_core::{
    Check, CheckType, FocusMethod, MatchMode, MouseButton, NodeCategory, NodeRun, NodeType,
    OnCheckFail, RunStatus, ScreenshotMode, TraceLevel, Workflow, validate_workflow,
};
use clickweave_llm::LlmConfig;
use eframe::egui::{self, Align, Align2, Button, Color32, Layout, RichText, TextureHandle, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Setup,
    Trace,
    Checks,
    Runs,
}

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
    detail_tab: DetailTab,

    // n8n-style UI state
    sidebar_collapsed: bool,
    logs_drawer_open: bool,
    node_search: String,
    is_active: bool,

    // Image preview cache (used in setup tab)
    #[allow(dead_code)]
    texture_cache: HashMap<String, TextureHandle>,

    // Run data (cached per selected node)
    cached_runs_node: Option<uuid::Uuid>,
    cached_runs: Vec<NodeRun>,
    selected_run_index: Option<usize>,
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
            detail_tab: DetailTab::Setup,
            sidebar_collapsed: false,
            logs_drawer_open: false,
            node_search: String::new(),
            is_active: false,
            texture_cache: HashMap::new(),
            cached_runs_node: None,
            cached_runs: Vec::new(),
            selected_run_index: None,
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
                        self.event_rx = Some(rx);
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
                ExecutorEvent::RunCreated(node_id, run) => {
                    self.push_log(format!("Run {} created for node {}", run.run_id, node_id));
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

    fn show_node_palette(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("node_palette")
            .frame(theme::inspector_frame())
            .exact_width(220.0)
            .resizable(false)
            .show(ctx, |ui| {
                self.show_node_palette_inline(ui);
            });
    }

    fn show_node_detail_overlay(&mut self, ctx: &egui::Context) {
        let Some(node_id) = self.selected_node else {
            return;
        };

        // Check the node exists
        if self.workflow.find_node(node_id).is_none() {
            self.selected_node = None;
            return;
        }

        let mut should_delete = false;
        let mut should_close = false;

        egui::Window::new("node_detail")
            .title_bar(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(600.0, 500.0))
            .frame(theme::node_detail_overlay_frame())
            .show(ctx, |ui| {
                // We need to extract info before borrowing mutably
                let node = self.workflow.find_node(node_id).unwrap();
                let category = node.node_type.category();
                let color = theme::category_color(category);
                let icon = node.node_type.icon().to_string();
                let type_name = node.node_type.display_name().to_string();
                let node_name = node.name.clone();
                let enabled = node.enabled;

                // Header
                ui.horizontal(|ui| {
                    ui.colored_label(color, RichText::new(&icon).size(24.0));
                    ui.add_space(8.0);
                    ui.heading(&node_name);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(&type_name)
                            .size(11.0)
                            .color(color)
                            .background_color(color.linear_multiply(0.15)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(Button::new(RichText::new("✕").size(16.0)).frame(false))
                            .on_hover_text("Close")
                            .clicked()
                        {
                            should_close = true;
                        }

                        let delete_btn =
                            Button::new(RichText::new("🗑").size(14.0).color(theme::NODE_END))
                                .frame(false);
                        if ui.add(delete_btn).on_hover_text("Delete node").clicked() {
                            should_delete = true;
                        }

                        let enabled_label = if enabled { "Enabled" } else { "Disabled" };
                        let enabled_color = if enabled { ACCENT_GREEN } else { TEXT_MUTED };
                        ui.label(RichText::new(enabled_label).size(11.0).color(enabled_color));
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Tab bar
                ui.horizontal(|ui| {
                    let tabs = [
                        (DetailTab::Setup, "Setup"),
                        (DetailTab::Trace, "Trace"),
                        (DetailTab::Checks, "Checks"),
                        (DetailTab::Runs, "Runs"),
                    ];
                    for (tab, label) in tabs {
                        if ui.selectable_label(self.detail_tab == tab, label).clicked() {
                            self.detail_tab = tab;
                        }
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Tab content
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| match self.detail_tab {
                        DetailTab::Setup => {
                            self.show_setup_tab(ui, node_id);
                        }
                        DetailTab::Trace => {
                            self.show_trace_tab(ui, node_id);
                        }
                        DetailTab::Checks => {
                            self.show_checks_tab(ui, node_id);
                        }
                        DetailTab::Runs => {
                            self.show_runs_tab(ui, node_id);
                        }
                    });
            });

        if should_close {
            self.selected_node = None;
        }
        if should_delete {
            self.workflow.remove_node(node_id);
            self.editor.sync_from_workflow(&self.workflow);
            self.selected_node = None;
        }
    }

    fn show_setup_tab(&mut self, ui: &mut egui::Ui, node_id: uuid::Uuid) {
        let Some(node) = self.workflow.find_node_mut(node_id) else {
            return;
        };

        // === Common fields ===
        ui.label(RichText::new("Name").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        ui.text_edit_singleline(&mut node.name);
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Enabled").size(12.0).color(TEXT_SECONDARY));
            ui.checkbox(&mut node.enabled, "");
        });
        ui.add_space(12.0);

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
        ui.add_space(12.0);

        ui.label(RichText::new("Retries").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        ui.add(egui::DragValue::new(&mut node.retries).range(0..=10));
        ui.add_space(12.0);

        // Trace level
        ui.label(
            RichText::new("Trace level")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("trace_level")
            .selected_text(match node.trace_level {
                TraceLevel::Off => "Off",
                TraceLevel::Minimal => "Minimal",
                TraceLevel::Full => "Full",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut node.trace_level, TraceLevel::Off, "Off");
                ui.selectable_value(&mut node.trace_level, TraceLevel::Minimal, "Minimal");
                ui.selectable_value(&mut node.trace_level, TraceLevel::Full, "Full");
            });
        ui.add_space(12.0);

        // Expected outcome
        ui.label(
            RichText::new("Expected outcome (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut outcome = node.expected_outcome.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut outcome).changed() {
            node.expected_outcome = if outcome.is_empty() {
                None
            } else {
                Some(outcome)
            };
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // === Per-type fields ===
        match &mut node.node_type {
            NodeType::AiStep(params) => {
                Self::show_ai_step_fields(ui, params);
            }
            NodeType::TakeScreenshot(params) => {
                Self::show_take_screenshot_fields(ui, params);
            }
            NodeType::FindText(params) => {
                Self::show_find_text_fields(ui, params);
            }
            NodeType::FindImage(params) => {
                Self::show_find_image_fields(ui, params);
            }
            NodeType::Click(params) => {
                Self::show_click_fields(ui, params);
            }
            NodeType::TypeText(params) => {
                Self::show_type_text_fields(ui, params);
            }
            NodeType::Scroll(params) => {
                Self::show_scroll_fields(ui, params);
            }
            NodeType::ListWindows(params) => {
                Self::show_list_windows_fields(ui, params);
            }
            NodeType::FocusWindow(params) => {
                Self::show_focus_window_fields(ui, params);
            }
            NodeType::AppDebugKitOp(params) => {
                Self::show_debugkit_fields(ui, params);
            }
        }
    }

    fn show_ai_step_fields(ui: &mut egui::Ui, params: &mut clickweave_core::AiStepParams) {
        ui.label(RichText::new("Prompt").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut params.prompt)
                .desired_rows(5)
                .desired_width(ui.available_width()),
        );
        ui.add_space(12.0);

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
        ui.add_space(12.0);

        ui.label(
            RichText::new("Template image (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut img_path = params.template_image.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut img_path)
                    .desired_width(ui.available_width() - 70.0),
            );
            if ui.button("Browse").clicked()
                && let Some(file) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg"])
                    .pick_file()
            {
                img_path = file.to_string_lossy().to_string();
            }
        });
        if img_path != params.template_image.clone().unwrap_or_default() {
            params.template_image = if img_path.is_empty() {
                None
            } else {
                Some(img_path)
            };
        }
        ui.add_space(12.0);

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
        ui.add_space(12.0);

        // Allowed tools
        ui.label(
            RichText::new("Allowed tools (comma-separated, empty = all)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut tools_str = params
            .allowed_tools
            .as_ref()
            .map(|t| t.join(", "))
            .unwrap_or_default();
        if ui.text_edit_singleline(&mut tools_str).changed() {
            params.allowed_tools = if tools_str.trim().is_empty() {
                None
            } else {
                Some(
                    tools_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                )
            };
        }
    }

    fn show_take_screenshot_fields(
        ui: &mut egui::Ui,
        params: &mut clickweave_core::TakeScreenshotParams,
    ) {
        ui.label(RichText::new("Mode").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("screenshot_mode")
            .selected_text(match params.mode {
                ScreenshotMode::Screen => "Screen",
                ScreenshotMode::Window => "Window",
                ScreenshotMode::Region => "Region",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut params.mode, ScreenshotMode::Screen, "Screen");
                ui.selectable_value(&mut params.mode, ScreenshotMode::Window, "Window");
                ui.selectable_value(&mut params.mode, ScreenshotMode::Region, "Region");
            });
        ui.add_space(12.0);

        ui.label(
            RichText::new("Target (app name / window id)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut target = params.target.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut target).changed() {
            params.target = if target.is_empty() {
                None
            } else {
                Some(target)
            };
        }
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Include OCR")
                    .size(12.0)
                    .color(TEXT_SECONDARY),
            );
            ui.checkbox(&mut params.include_ocr, "");
        });
    }

    fn show_find_text_fields(ui: &mut egui::Ui, params: &mut clickweave_core::FindTextParams) {
        ui.label(
            RichText::new("Search text")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.text_edit_singleline(&mut params.search_text);
        ui.add_space(12.0);

        ui.label(RichText::new("Match mode").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("match_mode")
            .selected_text(match params.match_mode {
                MatchMode::Contains => "Contains",
                MatchMode::Exact => "Exact",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut params.match_mode, MatchMode::Contains, "Contains");
                ui.selectable_value(&mut params.match_mode, MatchMode::Exact, "Exact");
            });
        ui.add_space(12.0);

        ui.label(
            RichText::new("Scope (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut scope = params.scope.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut scope).changed() {
            params.scope = if scope.is_empty() { None } else { Some(scope) };
        }
        ui.add_space(12.0);

        ui.label(
            RichText::new("Select result (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut select = params.select_result.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut select).changed() {
            params.select_result = if select.is_empty() {
                None
            } else {
                Some(select)
            };
        }
    }

    fn show_find_image_fields(ui: &mut egui::Ui, params: &mut clickweave_core::FindImageParams) {
        ui.label(
            RichText::new("Template image")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut img_path = params.template_image.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut img_path)
                    .desired_width(ui.available_width() - 70.0),
            );
            if ui.button("Browse").clicked()
                && let Some(file) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg"])
                    .pick_file()
            {
                img_path = file.to_string_lossy().to_string();
            }
        });
        if img_path != params.template_image.clone().unwrap_or_default() {
            params.template_image = if img_path.is_empty() {
                None
            } else {
                Some(img_path)
            };
        }
        ui.add_space(12.0);

        ui.label(RichText::new("Threshold").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        let mut threshold = params.threshold as f32;
        if ui
            .add(egui::Slider::new(&mut threshold, 0.0..=1.0))
            .changed()
        {
            params.threshold = threshold as f64;
        }
        ui.add_space(12.0);

        ui.label(
            RichText::new("Max results")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.add(egui::DragValue::new(&mut params.max_results).range(1..=20));
    }

    fn show_click_fields(ui: &mut egui::Ui, params: &mut clickweave_core::ClickParams) {
        ui.label(
            RichText::new("Target (coordinates or element)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut target = params.target.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut target).changed() {
            params.target = if target.is_empty() {
                None
            } else {
                Some(target)
            };
        }
        ui.add_space(12.0);

        ui.label(RichText::new("Button").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("mouse_button")
            .selected_text(match params.button {
                MouseButton::Left => "Left",
                MouseButton::Right => "Right",
                MouseButton::Center => "Center",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut params.button, MouseButton::Left, "Left");
                ui.selectable_value(&mut params.button, MouseButton::Right, "Right");
                ui.selectable_value(&mut params.button, MouseButton::Center, "Center");
            });
        ui.add_space(12.0);

        ui.label(
            RichText::new("Click count")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.add(egui::DragValue::new(&mut params.click_count).range(1..=3));
    }

    fn show_type_text_fields(ui: &mut egui::Ui, params: &mut clickweave_core::TypeTextParams) {
        ui.label(RichText::new("Text").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut params.text)
                .desired_rows(3)
                .desired_width(ui.available_width()),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Press Enter after")
                    .size(12.0)
                    .color(TEXT_SECONDARY),
            );
            ui.checkbox(&mut params.press_enter, "");
        });
    }

    fn show_scroll_fields(ui: &mut egui::Ui, params: &mut clickweave_core::ScrollParams) {
        ui.label(
            RichText::new("Delta Y (negative=up, positive=down)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.add(egui::DragValue::new(&mut params.delta_y).range(-1000..=1000));
        ui.add_space(12.0);

        ui.label(
            RichText::new("X position (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut x = params.x.unwrap_or(0.0);
        if ui.add(egui::DragValue::new(&mut x).speed(1.0)).changed() {
            params.x = if x == 0.0 { None } else { Some(x) };
        }
        ui.add_space(12.0);

        ui.label(
            RichText::new("Y position (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut y = params.y.unwrap_or(0.0);
        if ui.add(egui::DragValue::new(&mut y).speed(1.0)).changed() {
            params.y = if y == 0.0 { None } else { Some(y) };
        }
    }

    fn show_list_windows_fields(
        ui: &mut egui::Ui,
        params: &mut clickweave_core::ListWindowsParams,
    ) {
        ui.label(
            RichText::new("App name filter (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut app_name = params.app_name.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut app_name).changed() {
            params.app_name = if app_name.is_empty() {
                None
            } else {
                Some(app_name)
            };
        }
        ui.add_space(12.0);

        ui.label(
            RichText::new("Title pattern (optional)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut pattern = params.title_pattern.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut pattern).changed() {
            params.title_pattern = if pattern.is_empty() {
                None
            } else {
                Some(pattern)
            };
        }
    }

    fn show_focus_window_fields(
        ui: &mut egui::Ui,
        params: &mut clickweave_core::FocusWindowParams,
    ) {
        ui.label(RichText::new("Method").size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        egui::ComboBox::from_id_salt("focus_method")
            .selected_text(match params.method {
                FocusMethod::WindowId => "Window ID",
                FocusMethod::AppName => "App Name",
                FocusMethod::TitlePattern => "Title Pattern",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut params.method, FocusMethod::WindowId, "Window ID");
                ui.selectable_value(&mut params.method, FocusMethod::AppName, "App Name");
                ui.selectable_value(
                    &mut params.method,
                    FocusMethod::TitlePattern,
                    "Title Pattern",
                );
            });
        ui.add_space(12.0);

        let value_label = match params.method {
            FocusMethod::WindowId => "Window ID",
            FocusMethod::AppName => "App name",
            FocusMethod::TitlePattern => "Title pattern",
        };
        ui.label(RichText::new(value_label).size(12.0).color(TEXT_SECONDARY));
        ui.add_space(4.0);
        let mut value = params.value.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut value).changed() {
            params.value = if value.is_empty() { None } else { Some(value) };
        }
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Bring to front")
                    .size(12.0)
                    .color(TEXT_SECONDARY),
            );
            ui.checkbox(&mut params.bring_to_front, "");
        });
    }

    fn show_debugkit_fields(ui: &mut egui::Ui, params: &mut clickweave_core::AppDebugKitParams) {
        ui.label(
            RichText::new("Operation name")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ui.text_edit_singleline(&mut params.operation_name);
        ui.add_space(12.0);

        ui.label(
            RichText::new("Parameters (JSON)")
                .size(12.0)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        let mut json_str = serde_json::to_string_pretty(&params.parameters).unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::multiline(&mut json_str)
                    .desired_rows(5)
                    .desired_width(ui.available_width())
                    .code_editor(),
            )
            .changed()
            && let Ok(val) = serde_json::from_str(&json_str)
        {
            params.parameters = val;
        }
    }

    fn ensure_runs_loaded(&mut self, node_id: uuid::Uuid) {
        if self.cached_runs_node == Some(node_id) {
            return;
        }
        self.cached_runs_node = Some(node_id);
        self.cached_runs = Vec::new();
        self.selected_run_index = None;

        if let Some(project_path) = &self.project_path {
            let storage = RunStorage::new(project_path, self.workflow.id);
            match storage.load_runs_for_node(node_id) {
                Ok(runs) => {
                    if !runs.is_empty() {
                        self.selected_run_index = Some(runs.len() - 1);
                    }
                    self.cached_runs = runs;
                }
                Err(e) => {
                    tracing::warn!("Failed to load runs for node {}: {}", node_id, e);
                }
            }
        }
    }

    fn refresh_runs(&mut self, node_id: uuid::Uuid) {
        self.cached_runs_node = None;
        self.ensure_runs_loaded(node_id);
    }

    fn show_trace_tab(&mut self, ui: &mut egui::Ui, node_id: uuid::Uuid) {
        self.ensure_runs_loaded(node_id);

        if self.cached_runs.is_empty() {
            ui.label(
                RichText::new("No runs recorded yet. Execute this node to see trace data.")
                    .color(TEXT_MUTED),
            );
            return;
        }

        // Run selector
        let run_count = self.cached_runs.len();
        let selected_idx = self
            .selected_run_index
            .unwrap_or(run_count.saturating_sub(1));

        ui.horizontal(|ui| {
            ui.label(RichText::new("Run:").size(12.0).color(TEXT_SECONDARY));
            egui::ComboBox::from_id_salt("trace_run_selector")
                .selected_text(format!("Run {} of {}", selected_idx + 1, run_count))
                .show_ui(ui, |ui| {
                    for i in (0..run_count).rev() {
                        let run = &self.cached_runs[i];
                        let status_icon = match run.status {
                            RunStatus::Ok => "✓",
                            RunStatus::Failed => "✗",
                            RunStatus::Stopped => "⏹",
                        };
                        let label = format!(
                            "{} Run {} - {}",
                            status_icon,
                            i + 1,
                            format_timestamp(run.started_at)
                        );
                        if ui.selectable_label(selected_idx == i, label).clicked() {
                            self.selected_run_index = Some(i);
                        }
                    }
                });

            if ui.button("↻ Refresh").clicked() {
                self.refresh_runs(node_id);
            }
        });

        ui.add_space(12.0);

        let Some(run) = self.cached_runs.get(selected_idx) else {
            return;
        };

        // Run summary
        let status_color = match run.status {
            RunStatus::Ok => ACCENT_GREEN,
            RunStatus::Failed => theme::NODE_END,
            RunStatus::Stopped => TEXT_MUTED,
        };
        let status_text = match run.status {
            RunStatus::Ok => "OK",
            RunStatus::Failed => "Failed",
            RunStatus::Stopped => "Stopped",
        };
        let duration = run.ended_at.map(|e| e.saturating_sub(run.started_at));

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Status: {}", status_text)).color(status_color));
            if let Some(ms) = duration {
                ui.label(
                    RichText::new(format!("  Duration: {}ms", ms))
                        .size(12.0)
                        .color(TEXT_SECONDARY),
                );
            }
            ui.label(
                RichText::new(format!("  Artifacts: {}", run.artifacts.len()))
                    .size(12.0)
                    .color(TEXT_SECONDARY),
            );
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Events timeline
        if run.events.is_empty() {
            ui.label(RichText::new("No trace events recorded.").color(TEXT_MUTED));
        } else {
            ui.label(RichText::new("Events").size(14.0).color(TEXT_PRIMARY));
            ui.add_space(4.0);

            for (i, event) in run.events.iter().enumerate() {
                let icon = match event.event_type.as_str() {
                    "node_started" => "▶",
                    "tool_call" => "🔧",
                    "tool_result" => "📋",
                    "retry" => "🔄",
                    _ => "•",
                };

                let relative_ms = event.timestamp.saturating_sub(run.started_at);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("+{}ms", relative_ms))
                            .size(11.0)
                            .color(TEXT_MUTED)
                            .monospace(),
                    );
                    ui.label(RichText::new(icon).size(12.0));
                    ui.label(
                        RichText::new(&event.event_type)
                            .size(12.0)
                            .color(TEXT_PRIMARY),
                    );
                });

                // Show payload summary (collapsed)
                let payload_str = serde_json::to_string(&event.payload).unwrap_or_default();
                if payload_str != "null" && payload_str != "{}" {
                    let collapse_id = format!("event_payload_{}_{}", node_id, i);
                    egui::CollapsingHeader::new(
                        RichText::new("payload").size(11.0).color(TEXT_MUTED),
                    )
                    .id_salt(collapse_id)
                    .default_open(false)
                    .show(ui, |ui| {
                        let pretty =
                            serde_json::to_string_pretty(&event.payload).unwrap_or(payload_str);
                        ui.label(
                            RichText::new(pretty)
                                .size(11.0)
                                .color(TEXT_SECONDARY)
                                .monospace(),
                        );
                    });
                }

                if i < run.events.len() - 1 {
                    ui.add_space(4.0);
                }
            }
        }

        // Artifacts section
        if !run.artifacts.is_empty() {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(RichText::new("Artifacts").size(14.0).color(TEXT_PRIMARY));
            ui.add_space(4.0);

            for artifact in &run.artifacts {
                let kind_icon = match artifact.kind {
                    clickweave_core::ArtifactKind::Screenshot => "📸",
                    clickweave_core::ArtifactKind::Ocr => "📝",
                    clickweave_core::ArtifactKind::TemplateMatch => "🎯",
                    clickweave_core::ArtifactKind::Log => "📜",
                    clickweave_core::ArtifactKind::Other => "📎",
                };

                let filename = std::path::Path::new(&artifact.path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| artifact.path.clone());

                ui.horizontal(|ui| {
                    ui.label(RichText::new(kind_icon).size(12.0));
                    ui.label(
                        RichText::new(&filename)
                            .size(12.0)
                            .color(theme::ACCENT_BLUE),
                    );
                });
            }
        }
    }

    fn show_checks_tab(&mut self, ui: &mut egui::Ui, node_id: uuid::Uuid) {
        // Load last run status for checks display
        self.ensure_runs_loaded(node_id);
        let last_run = self.cached_runs.last().cloned();

        let Some(node) = self.workflow.find_node(node_id) else {
            return;
        };

        let checks_empty = node.checks.is_empty();

        if checks_empty {
            ui.label(
                RichText::new("No checks configured. Add a check to verify node output.")
                    .color(TEXT_MUTED),
            );
            ui.add_space(12.0);
        } else {
            // Display existing checks
            let checks: Vec<_> = node.checks.to_vec();
            let mut delete_index = None;

            for (i, check) in checks.iter().enumerate() {
                let check_id = format!("check_{}_{}", node_id, i);

                let pass_icon = if last_run.is_some() {
                    "⏳" // we don't evaluate checks yet, just show pending
                } else {
                    "—"
                };

                let on_fail_label = match check.on_fail {
                    OnCheckFail::FailNode => "Fail",
                    OnCheckFail::WarnOnly => "Warn",
                };

                egui::CollapsingHeader::new(
                    RichText::new(format!("{} {} [{}]", pass_icon, check.name, on_fail_label))
                        .size(13.0)
                        .color(TEXT_PRIMARY),
                )
                .id_salt(&check_id)
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Type: {:?}", check.check_type))
                            .size(12.0)
                            .color(TEXT_SECONDARY),
                    );

                    let params_str =
                        serde_json::to_string_pretty(&check.params).unwrap_or_default();
                    if params_str != "null" && params_str != "{}" {
                        ui.label(
                            RichText::new(format!("Params: {}", params_str))
                                .size(11.0)
                                .color(TEXT_MUTED)
                                .monospace(),
                        );
                    }

                    ui.add_space(4.0);
                    if ui
                        .add(
                            Button::new(RichText::new("Delete").size(11.0).color(theme::NODE_END))
                                .frame(false),
                        )
                        .clicked()
                    {
                        delete_index = Some(i);
                    }
                });

                ui.add_space(4.0);
            }

            if let Some(idx) = delete_index
                && let Some(node) = self.workflow.find_node_mut(node_id)
            {
                node.checks.remove(idx);
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Add check section
        ui.label(RichText::new("Add Check").size(13.0).color(TEXT_PRIMARY));
        ui.add_space(8.0);

        let check_types = [
            (CheckType::TextPresent, "Text Present"),
            (CheckType::TextAbsent, "Text Absent"),
            (CheckType::TemplateFound, "Template Found"),
            (CheckType::WindowTitleMatches, "Window Title Matches"),
        ];

        for (ct, label) in check_types {
            if ui
                .add(
                    Button::new(RichText::new(format!("+ {}", label)).size(12.0))
                        .frame(false)
                        .min_size(Vec2::new(ui.available_width(), 28.0)),
                )
                .clicked()
            {
                let check = Check {
                    name: label.to_string(),
                    check_type: ct,
                    params: serde_json::json!({}),
                    on_fail: OnCheckFail::FailNode,
                };
                if let Some(node) = self.workflow.find_node_mut(node_id) {
                    node.checks.push(check);
                }
            }
        }
    }

    fn show_runs_tab(&mut self, ui: &mut egui::Ui, node_id: uuid::Uuid) {
        self.ensure_runs_loaded(node_id);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} runs", self.cached_runs.len()))
                    .size(13.0)
                    .color(TEXT_PRIMARY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("↻ Refresh").clicked() {
                    self.refresh_runs(node_id);
                }
            });
        });

        ui.add_space(8.0);

        if self.cached_runs.is_empty() {
            ui.label(
                RichText::new("No runs yet. Execute this node to see run history.")
                    .color(TEXT_MUTED),
            );
            return;
        }

        let selected_idx = self.selected_run_index;

        // Display runs in reverse chronological order
        for i in (0..self.cached_runs.len()).rev() {
            let run = &self.cached_runs[i];

            let status_color = match run.status {
                RunStatus::Ok => ACCENT_GREEN,
                RunStatus::Failed => theme::NODE_END,
                RunStatus::Stopped => TEXT_MUTED,
            };
            let status_text = match run.status {
                RunStatus::Ok => "✓ OK",
                RunStatus::Failed => "✗ Failed",
                RunStatus::Stopped => "⏹ Stopped",
            };

            let duration = run.ended_at.map(|e| e.saturating_sub(run.started_at));
            let duration_str = duration
                .map(|ms| format!("{}ms", ms))
                .unwrap_or_else(|| "—".to_string());

            let event_count = run.events.len();
            let artifact_count = run.artifacts.len();

            let is_selected = selected_idx == Some(i);
            let bg = if is_selected {
                theme::BG_ACTIVE
            } else {
                theme::BG_PANEL
            };

            egui::Frame::NONE
                .fill(bg)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    let resp = ui
                        .horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("#{}", i + 1))
                                    .size(12.0)
                                    .color(TEXT_MUTED)
                                    .monospace(),
                            );
                            ui.label(RichText::new(status_text).size(12.0).color(status_color));
                            ui.label(
                                RichText::new(format_timestamp(run.started_at))
                                    .size(11.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(duration_str.clone())
                                        .size(11.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} events, {} artifacts",
                                        event_count, artifact_count
                                    ))
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                                );
                            });
                        })
                        .response;

                    if resp.interact(egui::Sense::click()).clicked() {
                        self.selected_run_index = Some(i);
                        self.detail_tab = DetailTab::Trace;
                    }
                });

            ui.add_space(4.0);
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
        self.show_node_palette(ctx);
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

        // Node detail overlay (on top of everything)
        self.show_node_detail_overlay(ctx);

        // Continuous repaint while running
        if matches!(self.executor_state, ExecutorState::Running) {
            ctx.request_repaint();
        }
    }
}

fn format_timestamp(millis: u64) -> String {
    let secs = millis / 1000;
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}
