use crate::editor::WorkflowEditor;
use crate::executor::{ExecutorCommand, ExecutorState, WorkflowExecutor};
use clickweave_core::{Workflow, validate_workflow};
use clickweave_llm::LlmConfig;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;

pub struct ClickweaveApp {
    workflow: Workflow,
    editor: WorkflowEditor,
    project_path: Option<PathBuf>,

    // Executor
    executor_tx: mpsc::Sender<ExecutorCommand>,
    executor_state: ExecutorState,
    executor: Option<WorkflowExecutor>,

    // Settings
    llm_config: LlmConfig,
    mcp_command: String,

    // UI state
    show_settings: bool,
    logs: Vec<String>,
    selected_node: Option<uuid::Uuid>,
}

impl ClickweaveApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (executor_tx, _executor_rx) = mpsc::channel();

        Self {
            workflow: Workflow::default(),
            editor: WorkflowEditor::new(),
            project_path: None,
            executor_tx,
            executor_state: ExecutorState::Idle,
            executor: None,
            llm_config: LlmConfig::default(),
            mcp_command: "npx".to_string(),
            show_settings: false,
            logs: vec!["Clickweave started".to_string()],
            selected_node: None,
        }
    }

    fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::info!("{}", msg);
        self.logs.push(msg);
        if self.logs.len() > 1000 {
            self.logs.remove(0);
        }
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
            // Create assets directory
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
        // Sync editor state to workflow
        self.editor.sync_to_workflow(&mut self.workflow);

        // Validate
        if let Err(e) = validate_workflow(&self.workflow) {
            self.log(format!("Validation failed: {}", e));
            return;
        }

        self.log("Starting workflow execution...");
        self.executor_state = ExecutorState::Running;

        // Create executor
        let workflow = self.workflow.clone();
        let llm_config = self.llm_config.clone();
        let mcp_command = self.mcp_command.clone();
        let project_path = self.project_path.clone();

        let (tx, rx) = mpsc::channel();
        self.executor_tx = tx;

        // Spawn executor in background thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut executor =
                    WorkflowExecutor::new(workflow, llm_config, mcp_command, project_path);
                executor.run(rx).await;
            });
        });
    }

    fn stop_workflow(&mut self) {
        let _ = self.executor_tx.send(ExecutorCommand::Stop);
        self.executor_state = ExecutorState::Idle;
        self.log("Workflow stopped");
    }
}

impl eframe::App for ClickweaveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
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
                    if ui.button("Save").clicked() {
                        self.save_workflow();
                        ui.close();
                    }
                    if ui.button("Save As...").clicked() {
                        self.save_workflow_as();
                        ui.close();
                    }
                });

                ui.separator();

                let is_running = matches!(self.executor_state, ExecutorState::Running);

                if ui
                    .add_enabled(!is_running, egui::Button::new("▶ Run"))
                    .clicked()
                {
                    self.run_workflow();
                }

                if ui
                    .add_enabled(is_running, egui::Button::new("⏹ Stop"))
                    .clicked()
                {
                    self.stop_workflow();
                }

                ui.separator();

                if ui.button("⚙ Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }

                ui.separator();

                // Status
                let status = match self.executor_state {
                    ExecutorState::Idle => "Idle",
                    ExecutorState::Running => "Running...",
                    ExecutorState::Paused => "Paused",
                    ExecutorState::Error => "Error",
                };
                ui.label(format!("Status: {}", status));
            });
        });

        // Settings window
        if self.show_settings {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("LLM Configuration");
                    ui.horizontal(|ui| {
                        ui.label("Base URL:");
                        ui.text_edit_singleline(&mut self.llm_config.base_url);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Model:");
                        ui.text_edit_singleline(&mut self.llm_config.model);
                    });
                    ui.horizontal(|ui| {
                        ui.label("API Key:");
                        let mut key = self.llm_config.api_key.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut key).changed() {
                            self.llm_config.api_key = if key.is_empty() { None } else { Some(key) };
                        }
                    });

                    ui.separator();
                    ui.heading("MCP Configuration");
                    ui.horizontal(|ui| {
                        ui.label("Command:");
                        ui.text_edit_singleline(&mut self.mcp_command);
                    });
                    ui.label("(Use 'npx' for npx -y native-devtools-mcp)");

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_settings = false;
                    }
                });
        }

        // Left panel: Node palette
        egui::SidePanel::left("palette")
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.heading("Nodes");
                ui.separator();

                if ui.button("+ Step").clicked() {
                    let id = self.workflow.add_node(
                        clickweave_core::NodeKind::Step,
                        clickweave_core::Position { x: 300.0, y: 200.0 },
                        "New Step",
                    );
                    self.editor.sync_from_workflow(&self.workflow);
                    self.selected_node = Some(id);
                }

                ui.separator();
                ui.heading("Workflow");
                ui.label(format!("Nodes: {}", self.workflow.nodes.len()));
                ui.label(format!("Edges: {}", self.workflow.edges.len()));
            });

        // Right panel: Node inspector
        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();

                if let Some(node_id) = self.selected_node {
                    if let Some(node) = self.workflow.find_node_mut(node_id) {
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut node.name);
                        });

                        ui.label(format!("Type: {}", node.kind.display_name()));

                        if node.kind == clickweave_core::NodeKind::Step {
                            ui.separator();
                            ui.label("Prompt:");
                            ui.add(
                                egui::TextEdit::multiline(&mut node.params.prompt)
                                    .desired_rows(5)
                                    .desired_width(f32::INFINITY),
                            );

                            ui.separator();
                            ui.label("Button text (optional):");
                            let mut btn_text = node.params.button_text.clone().unwrap_or_default();
                            if ui.text_edit_singleline(&mut btn_text).changed() {
                                node.params.button_text = if btn_text.is_empty() {
                                    None
                                } else {
                                    Some(btn_text)
                                };
                            }

                            ui.separator();
                            ui.label("Image path (optional):");
                            let mut img_path = node.params.image_path.clone().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut img_path);
                                if ui.button("Browse...").clicked() {
                                    if let Some(file) = rfd::FileDialog::new()
                                        .add_filter("Images", &["png", "jpg", "jpeg"])
                                        .pick_file()
                                    {
                                        // Copy to assets if we have a project path
                                        if let Some(proj) = &self.project_path {
                                            let assets = proj.join("assets");
                                            let _ = std::fs::create_dir_all(&assets);
                                            let filename = file.file_name().unwrap();
                                            let dest = assets.join(filename);
                                            if std::fs::copy(&file, &dest).is_ok() {
                                                img_path = format!(
                                                    "assets/{}",
                                                    filename.to_string_lossy()
                                                );
                                            }
                                        } else {
                                            img_path = file.to_string_lossy().to_string();
                                        }
                                    }
                                }
                            });
                            if img_path != node.params.image_path.clone().unwrap_or_default() {
                                node.params.image_path = if img_path.is_empty() {
                                    None
                                } else {
                                    Some(img_path)
                                };
                            }

                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label("Max tool calls:");
                                let mut max_calls = node.params.max_tool_calls.unwrap_or(10);
                                if ui
                                    .add(egui::DragValue::new(&mut max_calls).range(1..=100))
                                    .changed()
                                {
                                    node.params.max_tool_calls = Some(max_calls);
                                }
                            });
                        }

                        ui.separator();
                        if node.kind == clickweave_core::NodeKind::Step {
                            if ui.button("🗑 Delete Node").clicked() {
                                self.workflow.remove_node(node_id);
                                self.editor.sync_from_workflow(&self.workflow);
                                self.selected_node = None;
                            }
                        }
                    } else {
                        self.selected_node = None;
                    }
                } else {
                    ui.label("Select a node to edit");
                }
            });

        // Bottom panel: Log console
        egui::TopBottomPanel::bottom("logs")
            .default_height(150.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Logs");
                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for log in &self.logs {
                            ui.label(log);
                        }
                    });
            });

        // Center: Graph editor
        egui::CentralPanel::default().show(ctx, |ui| {
            let response = self.editor.show(ui, &mut self.workflow);
            if let Some(selected) = response.selected_node {
                self.selected_node = Some(selected);
            }
        });

        // Request continuous repaint while running
        if matches!(self.executor_state, ExecutorState::Running) {
            ctx.request_repaint();
        }
    }
}
