mod ai_step;
mod app_resolve;
mod check_eval;
mod deterministic;
mod element_resolve;
mod run_loop;
mod trace;

#[cfg(test)]
mod tests;

use clickweave_core::runtime::RuntimeContext;
use clickweave_core::storage::RunStorage;
use clickweave_core::{Check, NodeRun, NodeVerdict, Workflow};
use clickweave_llm::{ChatBackend, LlmClient, LlmConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutorState {
    Idle,
    Running,
}

pub enum ExecutorCommand {
    Stop,
}

/// Events sent from the executor back to the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutorEvent {
    Log(String),
    StateChanged(ExecutorState),
    NodeStarted(Uuid),
    NodeCompleted(Uuid),
    NodeFailed(Uuid, String),
    RunCreated(Uuid, NodeRun),
    WorkflowCompleted,
    ChecksCompleted(Vec<NodeVerdict>),
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedApp {
    pub name: String,
    pub pid: i32,
}

pub struct WorkflowExecutor<C: ChatBackend = LlmClient> {
    workflow: Workflow,
    agent: C,
    vlm: Option<C>,
    mcp_command: String,
    project_path: Option<PathBuf>,
    event_tx: Sender<ExecutorEvent>,
    storage: RunStorage,
    app_cache: RwLock<HashMap<String, ResolvedApp>>,
    focused_app: RwLock<Option<String>>,
    element_cache: RwLock<HashMap<(String, Option<String>), String>>,
    context: RuntimeContext,
    /// Nodes that completed successfully and have checks, in execution order.
    completed_checks: Vec<(Uuid, Vec<Check>, Option<String>)>,
}

impl WorkflowExecutor {
    pub fn new(
        workflow: Workflow,
        agent_config: LlmConfig,
        vlm_config: Option<LlmConfig>,
        mcp_command: String,
        project_path: Option<PathBuf>,
        event_tx: Sender<ExecutorEvent>,
        storage: RunStorage,
    ) -> Self {
        Self {
            workflow,
            agent: LlmClient::new(agent_config),
            vlm: vlm_config.map(LlmClient::new),
            mcp_command,
            project_path,
            event_tx,
            storage,
            app_cache: RwLock::new(HashMap::new()),
            focused_app: RwLock::new(None),
            element_cache: RwLock::new(HashMap::new()),
            context: RuntimeContext::new(),
            completed_checks: Vec::new(),
        }
    }
}
