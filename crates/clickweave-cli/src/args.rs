use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Clickweave headless CLI.
#[derive(Debug, Parser)]
#[command(name = "clickweave", about = "Clickweave headless automation CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run an agent against a goal.
    Run(RunArgs),
    /// Execute a saved skill.
    #[command(name = "run-skill")]
    RunSkill(RunSkillArgs),
    /// Manage skills.
    Skills(SkillsArgs),
    /// Manage run history.
    Runs(RunsArgs),
}

/// Arguments for the `run` subcommand.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Goal description for the agent.
    pub goal: String,

    // ── Project location (mutually exclusive group) ──────────────────────
    /// Path to a saved project directory or project.json file.
    #[arg(long, group = "project_source")]
    pub project: Option<PathBuf>,

    /// Name of an unsaved/transient project (must be combined with --project-id).
    #[arg(long, requires = "project_id", group = "project_source")]
    pub project_name: Option<String>,

    /// ID of an unsaved/transient project (must be combined with --project-name).
    #[arg(long, requires = "project_name")]
    pub project_id: Option<String>,

    // ── LLM endpoint ─────────────────────────────────────────────────────
    /// LLM base URL (overrides CLICKWEAVE_BASE_URL).
    #[arg(long)]
    pub base_url: Option<String>,

    /// LLM model name (overrides CLICKWEAVE_MODEL).
    #[arg(long)]
    pub model: Option<String>,

    /// LLM API key (overrides CLICKWEAVE_API_KEY).
    #[arg(long)]
    pub api_key: Option<String>,

    // ── MCP ──────────────────────────────────────────────────────────────
    /// Path to the native-devtools-mcp binary.
    #[arg(long)]
    pub mcp_binary: Option<String>,

    // ── Run parameters ───────────────────────────────────────────────────
    /// Maximum number of agent steps.
    #[arg(long)]
    pub max_steps: Option<usize>,

    // ── Approval ─────────────────────────────────────────────────────────
    /// Automatically approve all tool calls (alias for --allow-all).
    #[arg(long, group = "approval_mode")]
    pub yes: bool,

    /// Automatically approve all tool calls.
    #[arg(long, group = "approval_mode")]
    pub allow_all: bool,

    /// Path to a PermissionPolicy JSON file (engine-side policy, not a responder).
    #[arg(long, group = "approval_mode")]
    pub policy: Option<PathBuf>,

    // ── Output ───────────────────────────────────────────────────────────
    /// Emit NDJSON to stdout instead of human-readable output.
    #[arg(long)]
    pub json: bool,

    /// Disable trace persistence (privacy kill switch).
    #[arg(long)]
    pub no_store_traces: bool,

    /// Include base64 screenshot blobs in JSON output (only relevant with --json).
    #[arg(long)]
    pub include_screenshots: bool,
}

/// Arguments for the `run-skill` subcommand.
/// Note: no approval flags — the deterministic skill runner does not gate per-step.
#[derive(Debug, Args)]
pub struct RunSkillArgs {
    /// Skill ID to execute.
    pub skill_id: String,

    // ── Project location ─────────────────────────────────────────────────
    /// Path to a saved project directory or project.json file.
    #[arg(long, group = "project_source")]
    pub project: Option<PathBuf>,

    /// Name of an unsaved/transient project (must be combined with --project-id).
    #[arg(long, requires = "project_id", group = "project_source")]
    pub project_name: Option<String>,

    /// ID of an unsaved/transient project (must be combined with --project-name).
    #[arg(long, requires = "project_name")]
    pub project_id: Option<String>,

    // ── MCP ──────────────────────────────────────────────────────────────
    /// Path to the native-devtools-mcp binary.
    #[arg(long)]
    pub mcp_binary: Option<String>,

    // ── Variables ────────────────────────────────────────────────────────
    /// Set a skill variable as key=value (repeatable).
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Path to a JSON file of skill variables.
    #[arg(long = "vars")]
    pub vars_file: Option<PathBuf>,

    // ── Output ───────────────────────────────────────────────────────────
    /// Emit NDJSON to stdout instead of human-readable output.
    #[arg(long)]
    pub json: bool,

    /// Disable trace persistence (privacy kill switch).
    #[arg(long)]
    pub no_store_traces: bool,
}

/// Arguments for the `skills` subcommand.
#[derive(Debug, Args)]
pub struct SkillsArgs {
    #[command(subcommand)]
    pub subcommand: SkillsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillsSubcommand {
    /// List all skills in the project.
    List {
        /// Path to the project skills directory or project path.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Show details for a single skill.
    Show {
        /// Skill ID to display.
        skill_id: String,
        /// Path to the project skills directory or project path.
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

/// Arguments for the `runs` subcommand.
#[derive(Debug, Args)]
pub struct RunsArgs {
    #[command(subcommand)]
    pub subcommand: RunsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum RunsSubcommand {
    /// List runs for a skill.
    List {
        /// Skill ID to list runs for.
        skill_id: String,
        /// Path to the project directory.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Show trace events for a skill run.
    Events {
        /// Skill ID.
        skill_id: String,
        /// Run ID (UUID).
        run_id: String,
        /// Path to the project directory.
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// `run` parses goal + project flag.
    #[test]
    fn run_parses_goal_and_project() {
        let cli = Cli::try_parse_from([
            "clickweave",
            "run",
            "open the calculator",
            "--project",
            "/tmp/myproject",
        ])
        .unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected Run");
        };
        assert_eq!(args.goal, "open the calculator");
        assert_eq!(args.project, Some(PathBuf::from("/tmp/myproject")));
    }

    /// `run` rejects both --project and --project-name at the same time.
    #[test]
    fn run_rejects_conflicting_project_flags() {
        let result = Cli::try_parse_from([
            "clickweave",
            "run",
            "goal",
            "--project",
            "/tmp/x",
            "--project-name",
            "foo",
            "--project-id",
            "00000000-0000-0000-0000-000000000001",
        ]);
        assert!(
            result.is_err(),
            "conflicting project flags should be rejected"
        );
    }

    /// `run` rejects --project-name without --project-id.
    #[test]
    fn run_rejects_project_name_without_project_id() {
        let result = Cli::try_parse_from(["clickweave", "run", "goal", "--project-name", "foo"]);
        assert!(result.is_err());
    }

    /// `run` rejects --yes and --allow-all together.
    #[test]
    fn run_rejects_yes_and_allow_all_together() {
        let result = Cli::try_parse_from(["clickweave", "run", "goal", "--yes", "--allow-all"]);
        assert!(result.is_err());
    }

    /// `run` rejects --yes and --policy together.
    #[test]
    fn run_rejects_yes_and_policy_together() {
        let result = Cli::try_parse_from([
            "clickweave",
            "run",
            "goal",
            "--yes",
            "--policy",
            "/tmp/policy.json",
        ]);
        assert!(result.is_err());
    }

    /// `run-skill` parses skill ID.
    #[test]
    fn run_skill_parses_skill_id() {
        let cli = Cli::try_parse_from(["clickweave", "run-skill", "skl_abc123"]).unwrap();
        let Command::RunSkill(args) = cli.command else {
            panic!("expected RunSkill");
        };
        assert_eq!(args.skill_id, "skl_abc123");
    }

    /// `run-skill` has NO approval flags.
    #[test]
    fn run_skill_has_no_approval_flags() {
        let help = Cli::command()
            .find_subcommand("run-skill")
            .unwrap()
            .clone()
            .render_long_help()
            .to_string();
        assert!(!help.contains("--yes"), "run-skill must not expose --yes");
        assert!(
            !help.contains("--policy"),
            "run-skill must not expose --policy"
        );
        assert!(
            !help.contains("--allow-all"),
            "run-skill must not expose --allow-all"
        );
    }

    /// `run-skill` accepts --var repeatable flags.
    #[test]
    fn run_skill_parses_vars() {
        let cli = Cli::try_parse_from([
            "clickweave",
            "run-skill",
            "skl_abc123",
            "--var",
            "name=Alice",
            "--var",
            "count=5",
        ])
        .unwrap();
        let Command::RunSkill(args) = cli.command else {
            panic!("expected RunSkill");
        };
        assert_eq!(args.vars, vec!["name=Alice", "count=5"]);
    }

    /// `skills list` parses correctly.
    #[test]
    fn skills_list_parses() {
        let cli = Cli::try_parse_from(["clickweave", "skills", "list"]).unwrap();
        let Command::Skills(args) = cli.command else {
            panic!("expected Skills");
        };
        assert!(matches!(args.subcommand, SkillsSubcommand::List { .. }));
    }

    /// `runs list` requires skill ID.
    #[test]
    fn runs_list_parses_with_skill_id() {
        let cli = Cli::try_parse_from(["clickweave", "runs", "list", "skl_abc123"]).unwrap();
        let Command::Runs(args) = cli.command else {
            panic!("expected Runs");
        };
        let RunsSubcommand::List { skill_id, .. } = args.subcommand else {
            panic!("expected List");
        };
        assert_eq!(skill_id, "skl_abc123");
    }

    /// `runs events` requires both skill ID and run ID.
    #[test]
    fn runs_events_parses_with_ids() {
        let run_id = "00000000-0000-0000-0000-000000000001";
        let cli =
            Cli::try_parse_from(["clickweave", "runs", "events", "skl_abc123", run_id]).unwrap();
        let Command::Runs(args) = cli.command else {
            panic!("expected Runs");
        };
        let RunsSubcommand::Events {
            skill_id,
            run_id: rid,
            ..
        } = args.subcommand
        else {
            panic!("expected Events");
        };
        assert_eq!(skill_id, "skl_abc123");
        assert_eq!(rid, run_id);
    }
}
