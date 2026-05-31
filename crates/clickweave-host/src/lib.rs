pub mod approval;
pub mod config;
pub mod context;
pub mod lifecycle;
pub mod mcp;
pub mod run;
pub mod runs;
pub mod skills;
pub mod storage;

// Re-export engine types so downstream crates (clickweave-cli) can use them
// without declaring separate dependencies on clickweave-engine / clickweave-llm.
pub use clickweave_core::SkillRun;
pub use clickweave_engine::agent::episodic::EpisodicContext;
pub use clickweave_engine::agent::episodic::types::WriteRequest as EpisodicWriterTx;
pub use clickweave_engine::agent::skills::{Skill, SkillContext};
pub use clickweave_engine::agent::{
    AgentChannels, AgentConfig, AgentState, ApprovalRequest, PermissionPolicy, RunnerOutput,
    TerminalReason,
};
pub use clickweave_engine::executor::Mcp;
pub use clickweave_engine::executor::error::ExecutorResult;
pub use clickweave_engine::executor::skill_runner::run_skill_steps;
pub use clickweave_llm::{ChatBackend, LlmClient, LlmConfig};
pub use uuid::Uuid;
