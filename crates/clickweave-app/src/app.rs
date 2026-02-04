use crate::editor::WorkflowEditor;
use crate::executor::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use crate::theme::{
    self, ACCENT_CORAL, ACCENT_GREEN, BG_DARK, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use clickweave_core::{Workflow, validate_workflow};
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

    // Image preview cache
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
                        // event_rx already taken out, don't put it back
                        return;
                    }
                }
                ExecutorEvent::NodeStarted(id) => {
                    self.active_node = Some(id);
                }
                ExecutorEvent::NodeCompleted(_) | ExecutorEvent::WorkflowCompleted => {
                    self.active_node = None;
                }
            }
        }

        // Put the receiver back if we didn't go idle
        self.event_rx = Some(rx);
    }

    fn add_step_node(&mut self) {
        let offset = self.workflow.nodes.len() as f32 * 50.0;
        let id = self.workflow.add_node(
            clickweave_core::NodeKind::Step,
            clickweave_core::Position {
                x: 300.0 + offset,
                y: 200.0 + offset,
            },
            "New Step",
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
                    ("🏠", "Home", true),
                    ("📋", "Templates", false),
                    ("📊", "Variables", false),
                    ("📜", "Executions", false),
                    ("❓", "Help", false),
                ];

                for (icon, label, _active) in nav_items {
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
                        // TODO: Handle navigation
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Node palette section
                if !self.sidebar_collapsed {
                    ui.label(RichText::new("NODES").size(11.0).color(TEXT_MUTED));
                    ui.add_space(8.0);
                }

                if ui
                    .add_sized(
                        [
                            if self.sidebar_collapsed {
                                40.0
                            } else {
                                sidebar_width - 16.0
                            },
                            32.0,
                        ],
                        Button::new(if self.sidebar_collapsed {
                            RichText::new("⚡").size(16.0)
                        } else {
                            RichText::new("⚡  Add Step").size(13.0)
                        }),
                    )
                    .on_hover_text("Add a new step node")
                    .clicked()
                {
                    self.add_step_node();
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
                    let _ = ui.selectable_label(true, "Editor");
                    let _ = ui.selectable_label(false, "Executions");

                    // Right side controls
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);

                        // More menu
                        ui.menu_button("⋯", |ui| {
                            if ui.button("Settings").clicked() {
                                self.show_settings = !self.show_settings;
                                ui.close();
                            }
                            if ui.button("New").clicked() {
                                self.workflow = Workflow::default();
                                self.editor.sync_from_workflow(&self.workflow);
                                self.project_path = None;
                                ui.close();
                            }
                            if ui.button("Open...").clicked() {
                                self.open_workflow();
                                ui.close();
                            }
                        });

                        // Save button (coral accent)
                        let save_btn = Button::new(RichText::new("Save").color(Color32::WHITE))
                            .fill(ACCENT_CORAL)
                            .corner_radius(6.0);
                        if ui.add(save_btn).clicked() {
                            self.save_workflow();
                        }

                        // Share button
                        if ui.button("Share").clicked() {
                            // TODO: Share functionality
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
                        // Header with node type
                        let node_kind = node.kind;
                        ui.horizontal(|ui| {
                            let (icon, color) = match node_kind {
                                clickweave_core::NodeKind::Start => ("▶", theme::NODE_START),
                                clickweave_core::NodeKind::Step => ("⚡", theme::NODE_STEP),
                                clickweave_core::NodeKind::End => ("⏹", theme::NODE_END),
                            };
                            ui.colored_label(color, RichText::new(icon).size(20.0));
                            ui.add_space(8.0);
                            ui.heading(&node.name);
                        });

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(node_kind.display_name())
                                .size(12.0)
                                .color(TEXT_MUTED),
                        );

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(12.0);

                        // Node name
                        ui.label(RichText::new("Name").size(12.0).color(TEXT_SECONDARY));
                        ui.add_space(4.0);
                        ui.text_edit_singleline(&mut node.name);

                        if node_kind == clickweave_core::NodeKind::Step {
                            ui.add_space(16.0);

                            // Prompt
                            ui.label(RichText::new("Prompt").size(12.0).color(TEXT_SECONDARY));
                            ui.add_space(4.0);
                            ui.add(
                                egui::TextEdit::multiline(&mut node.params.prompt)
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
                            let mut btn_text = node.params.button_text.clone().unwrap_or_default();
                            if ui.text_edit_singleline(&mut btn_text).changed() {
                                node.params.button_text = if btn_text.is_empty() {
                                    None
                                } else {
                                    Some(btn_text)
                                };
                            }

                            ui.add_space(16.0);

                            // Image path
                            ui.label(
                                RichText::new("Image path (optional)")
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(4.0);
                            let mut img_path = node.params.image_path.clone().unwrap_or_default();
                            let orig_img_path = img_path.clone();
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
                                    if let Some(proj) = &self.project_path {
                                        let assets = proj.join("assets");
                                        let _ = std::fs::create_dir_all(&assets);
                                        let filename = file.file_name().unwrap();
                                        let dest = assets.join(filename);
                                        if std::fs::copy(&file, &dest).is_ok() {
                                            img_path =
                                                format!("assets/{}", filename.to_string_lossy());
                                        }
                                    } else {
                                        img_path = file.to_string_lossy().to_string();
                                    }
                                }
                            });
                            if img_path != orig_img_path {
                                node.params.image_path = if img_path.is_empty() {
                                    None
                                } else {
                                    Some(img_path.clone())
                                };
                                // Clear cached texture when path changes
                                self.texture_cache.remove(&node_id.to_string());
                            }

                            // Image preview
                            if !img_path.is_empty() {
                                ui.add_space(8.0);
                                let cache_key = node_id.to_string();
                                let abs_path = if img_path.starts_with('/') {
                                    PathBuf::from(&img_path)
                                } else if let Some(proj) = &self.project_path {
                                    proj.join(&img_path)
                                } else {
                                    PathBuf::from(&img_path)
                                };

                                if !self.texture_cache.contains_key(&cache_key)
                                    && let Ok(img) = image::open(&abs_path)
                                {
                                    let rgba = img.to_rgba8();
                                    let size = [rgba.width() as usize, rgba.height() as usize];
                                    let pixels = rgba.into_raw();
                                    let color_image =
                                        egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                                    let texture = ui.ctx().load_texture(
                                        &cache_key,
                                        color_image,
                                        egui::TextureOptions::LINEAR,
                                    );
                                    self.texture_cache.insert(cache_key.clone(), texture);
                                }

                                if let Some(texture) = self.texture_cache.get(&cache_key) {
                                    let max_width = ui.available_width();
                                    let aspect =
                                        texture.size()[1] as f32 / texture.size()[0] as f32;
                                    let width = max_width.min(260.0);
                                    let height = width * aspect;
                                    ui.image(egui::load::SizedTexture::new(
                                        texture.id(),
                                        [width, height],
                                    ));
                                }
                            }

                            ui.add_space(16.0);

                            // Max tool calls
                            ui.label(
                                RichText::new("Max tool calls")
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(4.0);
                            let mut max_calls = node.params.max_tool_calls.unwrap_or(10);
                            if ui
                                .add(egui::DragValue::new(&mut max_calls).range(1..=100))
                                .changed()
                            {
                                node.params.max_tool_calls = Some(max_calls);
                            }

                            ui.add_space(16.0);

                            // Timeout
                            ui.label(
                                RichText::new("Timeout (ms, 0 = none)")
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            );
                            ui.add_space(4.0);
                            let mut timeout = node.params.timeout_ms.unwrap_or(0);
                            if ui
                                .add(
                                    egui::DragValue::new(&mut timeout)
                                        .range(0..=300000)
                                        .speed(100),
                                )
                                .changed()
                            {
                                node.params.timeout_ms =
                                    if timeout == 0 { None } else { Some(timeout) };
                            }

                            // Delete button
                            ui.add_space(24.0);
                            let delete_btn =
                                Button::new(RichText::new("🗑 Delete Node").color(theme::NODE_END))
                                    .frame(false);
                            if ui.add(delete_btn).clicked() {
                                should_delete_node = Some(node_id);
                            }
                        }
                    } else {
                        self.selected_node = None;
                    }
                } else {
                    // Node selector when nothing selected
                    ui.heading("Add Node");
                    ui.add_space(8.0);

                    // Search box
                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.node_search)
                                .hint_text("Search nodes..."),
                        );
                    });

                    ui.add_space(16.0);

                    // Node categories
                    ui.collapsing(RichText::new("⚡ Actions").color(TEXT_PRIMARY), |ui| {
                        ui.add_space(4.0);
                        if ui
                            .add(
                                Button::new("Step - AI-powered action")
                                    .frame(false)
                                    .min_size(Vec2::new(ui.available_width(), 28.0)),
                            )
                            .clicked()
                        {
                            self.add_step_node();
                        }
                    });

                    ui.collapsing(RichText::new("🔀 Flow").color(TEXT_PRIMARY), |ui| {
                        ui.label(
                            RichText::new("Coming soon: conditionals, loops")
                                .size(12.0)
                                .color(TEXT_MUTED),
                        );
                    });
                }
            });

        // Handle deferred deletion
        if let Some(node_id) = should_delete_node {
            self.workflow.remove_node(node_id);
            self.editor.sync_from_workflow(&self.workflow);
            self.selected_node = None;
        }
    }

    fn show_floating_toolbar(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("floating_toolbar"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
            .show(ctx, |ui| {
                theme::floating_toolbar_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Zoom controls
                        if ui
                            .add(Button::new("⊞").frame(false))
                            .on_hover_text("Fit to screen")
                            .clicked()
                        {
                            // TODO: Fit view
                        }
                        if ui
                            .add(Button::new("−").frame(false))
                            .on_hover_text("Zoom out")
                            .clicked()
                        {
                            // TODO: Zoom out
                        }
                        if ui
                            .add(Button::new("+").frame(false))
                            .on_hover_text("Zoom in")
                            .clicked()
                        {
                            // TODO: Zoom in
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Logs toggle
                        if ui
                            .add(Button::new("📜").frame(false))
                            .on_hover_text("Toggle logs")
                            .clicked()
                        {
                            self.logs_drawer_open = !self.logs_drawer_open;
                        }

                        ui.add_space(24.0);

                        // Test workflow button
                        let is_running = matches!(self.executor_state, ExecutorState::Running);
                        if is_running {
                            let stop_btn =
                                Button::new(RichText::new("⏹ Stop").color(Color32::WHITE))
                                    .fill(theme::NODE_END)
                                    .corner_radius(6.0)
                                    .min_size(Vec2::new(120.0, 32.0));
                            if ui.add(stop_btn).clicked() {
                                self.stop_workflow();
                            }
                        } else {
                            let test_btn =
                                Button::new(RichText::new("▶ Test workflow").color(Color32::WHITE))
                                    .fill(ACCENT_CORAL)
                                    .corner_radius(6.0)
                                    .min_size(Vec2::new(120.0, 32.0));
                            if ui.add(test_btn).clicked() {
                                self.run_workflow();
                            }
                        }
                    });
                });
            });
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
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: BG_DARK,
                ..Default::default()
            })
            .show(ctx, |ui| {
                let response = self.editor.show(ui, &mut self.workflow, active_node);
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
