//! Resolution probe — interactive CLI for testing CDP target resolution.
//!
//! Runs the same resolution chain the executor uses when `cdp_click` resolves
//! a target against a DOM snapshot, but offline (no running app needed).
//!
//! # Resolution chain
//!
//! 1. **Direct match** — `find_elements_in_snapshot(snapshot, target)` with
//!    optional role/parent narrowing.
//! 2. **Inventory fallback** (0 matches) — builds a compact element inventory,
//!    asks the LLM to pick the best label, then searches the snapshot for that
//!    label.
//! 3. **Disambiguation** (2+ matches) — formats candidates with ARIA roles and
//!    ancestor chains, asks the LLM to pick the correct uid.
//!
//! # What it does NOT cover
//!
//! - Workflow planning (see `planner_eval`)
//! - MCP tool calls / real app interaction
//! - Supervision / VLM verification
//! - Execution or retries
//!
//! # Usage
//!
//! ```bash
//! cargo run -p clickweave-llm --features eval --bin resolution_probe -- \
//!   --snapshot eval/snapshots/chat-app.txt \
//!   --target "Type a message"
//! ```
//!
//! Pass `--role textbox` or `--parent-role region` to narrow matches, matching
//! the `CdpExpected` constraints the executor applies.
//!
//! Use `--runs N` to repeat the LLM call N times and see consistency.
//!
//! # Configuration
//!
//! Reads the LLM endpoint from `eval/eval.toml` (same config as `planner_eval`).
//! Override with `--endpoint` and `--model` flags.

use anyhow::{Context, Result};
use clap::Parser;
use clickweave_core::cdp::{
    build_disambiguation_prompt, build_inventory_prompt, find_interactive_in_snapshot,
    narrow_by_parent, narrow_matches, resolve_disambiguation_response, resolve_inventory_response,
};
use clickweave_llm::{ChatBackend, LlmClient, LlmConfig};
use serde::Deserialize;
use std::path::PathBuf;

// ── CLI ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "resolution-probe",
    about = "Test CDP target resolution against a snapshot file"
)]
struct Cli {
    /// Path to the snapshot file (accessibility tree text)
    #[arg(long)]
    snapshot: PathBuf,

    /// Target string to resolve (e.g. "Type a message")
    #[arg(long)]
    target: String,

    /// Filter matches by ARIA role (e.g. "textbox", "button")
    #[arg(long)]
    role: Option<String>,

    /// Filter matches by parent role
    #[arg(long)]
    parent_role: Option<String>,

    /// Filter matches by parent name
    #[arg(long)]
    parent_name: Option<String>,

    /// Number of LLM resolution runs (for consistency testing)
    #[arg(long, default_value = "1")]
    runs: u32,

    /// Path to eval config file (for LLM endpoint)
    #[arg(long, default_value = "eval/eval.toml")]
    config: PathBuf,

    /// Override LLM endpoint URL
    #[arg(long)]
    endpoint: Option<String>,

    /// Override model name
    #[arg(long)]
    model: Option<String>,
}

// ── Config ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct EvalConfig {
    llm: LlmSection,
}

#[derive(Deserialize)]
struct LlmSection {
    #[serde(default)]
    endpoint: Option<String>,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
}

// ── Resolution result ───────────────────────────────────────────

#[derive(Debug)]
enum ResolutionPath {
    /// Target matched exactly one element.
    DirectSingle,
    /// Target matched multiple elements; LLM picked one.
    Disambiguation,
    /// No direct match; inventory fallback resolved to a label.
    Inventory { resolved_label: String },
    /// Inventory fallback found multiple matches; LLM disambiguated.
    InventoryThenDisambiguation { resolved_label: String },
}

#[derive(Debug)]
struct ResolutionResult {
    path: ResolutionPath,
    uid: String,
    label: String,
    role: String,
    ancestors: Vec<(String, String)>,
}

// ── Main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load snapshot
    let snapshot_text = std::fs::read_to_string(&cli.snapshot)
        .with_context(|| format!("Failed to read snapshot: {}", cli.snapshot.display()))?;

    // Load LLM config (optional — only needed when resolution requires an LLM call)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join(&cli.config);
    let config: Option<EvalConfig> = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|text| toml_edit::de::from_str(&text).ok());

    let endpoint = cli
        .endpoint
        .clone()
        .or_else(|| config.as_ref().and_then(|c| c.llm.endpoint.clone()));
    let model = cli
        .model
        .clone()
        .or_else(|| config.as_ref().map(|c| c.llm.model.clone()));
    let api_key = config.as_ref().and_then(|c| c.llm.api_key.clone());

    let client: Option<LlmClient> = match (&endpoint, &model) {
        (Some(ep), Some(m)) => Some(LlmClient::new(LlmConfig {
            base_url: ep
                .trim_end_matches('/')
                .trim_end_matches("/chat/completions")
                .to_string(),
            api_key,
            model: m.clone(),
            max_tokens: Some(256),
            ..LlmConfig::default()
        })),
        _ => None,
    };

    // Print header
    println!("═══ Resolution Probe ═══");
    println!("Snapshot: {}", cli.snapshot.display());
    println!(
        "Snapshot size: {} chars, {} lines",
        snapshot_text.len(),
        snapshot_text.lines().count()
    );
    println!("Target: \"{}\"", cli.target);
    if let (Some(m), Some(ep)) = (&model, &endpoint) {
        println!("LLM: {} @ {}", m, ep);
    } else {
        println!("LLM: not configured (direct matches only)");
    }
    if let Some(ref role) = cli.role {
        println!("Role filter: {}", role);
    }
    if let Some(ref pr) = cli.parent_role {
        println!("Parent role filter: {}", pr);
    }
    println!("Runs: {}", cli.runs);
    println!();

    // Step 1: Direct match (prefer interactive roles)
    let mut matches = find_interactive_in_snapshot(&snapshot_text, &cli.target);
    println!(
        "Step 1 — find_interactive_in_snapshot(\"{}\") → {} matches",
        cli.target,
        matches.len()
    );
    for m in &matches {
        println!(
            "  uid={} role={} \"{}\" (parent: {:?})",
            m.uid, m.role, m.label, m.parent_role
        );
    }

    // Step 1b: Narrow
    narrow_matches(
        &mut matches,
        cli.role.as_deref(),
        None, // href not exposed via CLI
    );
    narrow_by_parent(
        &mut matches,
        cli.parent_role.as_deref(),
        cli.parent_name.as_deref(),
    );

    if matches.len() != find_interactive_in_snapshot(&snapshot_text, &cli.target).len() {
        println!("  After narrowing (role/parent): {} matches", matches.len());
    }
    println!();

    // Run resolution
    for run in 0..cli.runs {
        if cli.runs > 1 {
            println!("── Run {}/{} ──", run + 1, cli.runs);
        }

        let result = resolve(&cli.target, &matches, &snapshot_text, client.as_ref()).await?;

        match &result.path {
            ResolutionPath::DirectSingle => {
                println!("Path: direct single match");
            }
            ResolutionPath::Disambiguation => {
                println!(
                    "Path: disambiguation (LLM picked from {} candidates)",
                    matches.len()
                );
            }
            ResolutionPath::Inventory { resolved_label } => {
                println!("Path: inventory fallback → \"{}\"", resolved_label);
            }
            ResolutionPath::InventoryThenDisambiguation { resolved_label } => {
                println!(
                    "Path: inventory fallback → \"{}\" → disambiguation",
                    resolved_label
                );
            }
        }

        println!(
            "Result: uid={} role={} \"{}\"",
            result.uid, result.role, result.label
        );

        if !result.ancestors.is_empty() {
            let chain: Vec<String> = result
                .ancestors
                .iter()
                .map(|(role, name)| {
                    if name.is_empty() {
                        role.clone()
                    } else {
                        format!("{} \"{}\"", role, name)
                    }
                })
                .collect();
            println!("Ancestors: {}", chain.join(" > "));
        }
        println!();

        // Re-fetch matches for next run (disambiguation may have different LLM results)
    }

    Ok(())
}

/// Run the full resolution chain: direct match → inventory fallback → disambiguation.
async fn resolve(
    target: &str,
    matches: &[clickweave_core::cdp::SnapshotMatch],
    snapshot_text: &str,
    client: Option<&LlmClient>,
) -> Result<ResolutionResult> {
    if matches.len() == 1 {
        let m = &matches[0];
        return Ok(ResolutionResult {
            path: ResolutionPath::DirectSingle,
            uid: m.uid.clone(),
            label: m.label.clone(),
            role: m.role.clone(),
            ancestors: m.ancestors.clone(),
        });
    }

    let client = client.context(
        "LLM required for resolution but not configured. Pass --endpoint and --model, \
         or create eval/eval.toml",
    )?;

    if matches.len() > 1 {
        // Disambiguation
        let prompt = build_disambiguation_prompt(target, matches, None, &[]);
        println!("  LLM prompt ({} chars):", prompt.len());
        for line in prompt.lines().take(15) {
            println!("    {}", line);
        }
        if prompt.lines().count() > 15 {
            println!("    ... ({} more lines)", prompt.lines().count() - 15);
        }

        let response = client
            .chat(vec![clickweave_llm::Message::user(prompt)], None)
            .await
            .context("LLM disambiguation call failed")?;

        let raw = response
            .choices
            .first()
            .and_then(|c| c.message.content_text())
            .unwrap_or_default();

        println!("  LLM response: \"{}\"", raw.trim());

        let uid = resolve_disambiguation_response(&raw, matches);
        let chosen = matches.iter().find(|m| m.uid == uid).unwrap_or(&matches[0]);

        return Ok(ResolutionResult {
            path: ResolutionPath::Disambiguation,
            uid: chosen.uid.clone(),
            label: chosen.label.clone(),
            role: chosen.role.clone(),
            ancestors: chosen.ancestors.clone(),
        });
    }

    // 0 matches — inventory fallback
    let prompt = build_inventory_prompt(target, snapshot_text)
        .context("No interactive elements in snapshot")?;

    println!("  Inventory prompt ({} chars):", prompt.len());
    for line in prompt.lines() {
        println!("    {}", line);
    }

    let response = client
        .chat(vec![clickweave_llm::Message::user(prompt)], None)
        .await
        .context("LLM inventory call failed")?;

    let raw = response
        .choices
        .first()
        .and_then(|c| c.message.content_text())
        .unwrap_or_default();

    let resolved_label = raw.trim().trim_matches('"').to_string();
    println!("  LLM response: \"{}\"", resolved_label);

    let resolved_matches =
        resolve_inventory_response(target, &raw, snapshot_text).map_err(|e| anyhow::anyhow!(e))?;

    println!(
        "  Searching for \"{}\" → {} matches",
        resolved_label,
        resolved_matches.len()
    );
    for m in &resolved_matches {
        println!("    uid={} role={} \"{}\"", m.uid, m.role, m.label);
    }

    if resolved_matches.len() == 1 {
        let m = &resolved_matches[0];
        return Ok(ResolutionResult {
            path: ResolutionPath::Inventory { resolved_label },
            uid: m.uid.clone(),
            label: m.label.clone(),
            role: m.role.clone(),
            ancestors: m.ancestors.clone(),
        });
    }

    // Multiple matches after inventory — disambiguate
    let dis_prompt = build_disambiguation_prompt(target, &resolved_matches, None, &[]);
    println!("  Disambiguation prompt ({} chars):", dis_prompt.len());
    for line in dis_prompt.lines().take(15) {
        println!("    {}", line);
    }

    let dis_response = client
        .chat(vec![clickweave_llm::Message::user(dis_prompt)], None)
        .await
        .context("LLM disambiguation call failed")?;

    let dis_raw = dis_response
        .choices
        .first()
        .and_then(|c| c.message.content_text())
        .unwrap_or_default();

    println!("  LLM response: \"{}\"", dis_raw.trim());

    let uid = resolve_disambiguation_response(&dis_raw, &resolved_matches);
    let chosen = resolved_matches
        .iter()
        .find(|m| m.uid == uid)
        .unwrap_or(&resolved_matches[0]);

    Ok(ResolutionResult {
        path: ResolutionPath::InventoryThenDisambiguation { resolved_label },
        uid: chosen.uid.clone(),
        label: chosen.label.clone(),
        role: chosen.role.clone(),
        ancestors: chosen.ancestors.clone(),
    })
}
