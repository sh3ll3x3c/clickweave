use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{error::Error, fmt};

use anyhow::{Context, Result, bail};
use clickweave_engine::Mcp;
use clickweave_engine::agent::{
    AgentChannels, AgentConfig, PermissionPolicy, RunnerOutput,
    run_agent_workflow_with_prompt_override,
};
use clickweave_llm::{ChatBackend, ChatOptions, ChatResponse, LlmConfig, Message, ToolCall};
use clickweave_mcp::{ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const CODEX_JUDGE_PROMPT: &str = include_str!("../prompts/codex_judge.md");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalScenario {
    pub id: String,
    pub description: String,
    pub goal: String,
    pub max_steps: usize,
    pub tools: Vec<ToolSpec>,
    #[serde(default)]
    pub tool_behaviors: Vec<ToolBehavior>,
    pub scoring: ScoringSpec,
}

impl EvalScenario {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).context("read scenario file")?;
        let scenario: Self = serde_json::from_str(&raw).context("parse scenario file")?;
        scenario.validate_privacy()?;
        Ok(scenario)
    }

    /// Fail closed on obvious private-data hazards. Evals should use
    /// synthetic fixtures; real user traces must be reduced/redacted before
    /// becoming fixtures.
    pub fn validate_privacy(&self) -> Result<()> {
        if !self.id.starts_with("synthetic_") {
            bail!(
                "scenario {} is not marked as synthetic; eval fixtures must use synthetic redacted data",
                self.id
            );
        }
        let raw = serde_json::to_string(self)?;
        if private_marker(&raw).is_some() {
            bail!(
                "scenario {} appears to contain private path/secret material; use a synthetic redacted fixture",
                self.id
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBehavior {
    pub tool: String,
    #[serde(default)]
    pub response: Option<Value>,
    #[serde(default)]
    pub error: bool,
    #[serde(default)]
    pub required_args: Vec<String>,
    #[serde(default)]
    pub requires_state: HashMap<String, Value>,
    #[serde(default)]
    pub sets_state: HashMap<String, Value>,
    #[serde(default)]
    pub response_sequence: Vec<ToolResponse>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolResponse {
    #[serde(default)]
    pub response: Option<Value>,
    #[serde(default)]
    pub error: bool,
    #[serde(default)]
    pub sets_state: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringSpec {
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub forbidden_tools: Vec<String>,
    #[serde(default)]
    pub allowed_error_tools: Vec<String>,
    #[serde(default)]
    pub required_agent_tools: Vec<String>,
    #[serde(default)]
    pub required_agent_tool_groups: Vec<Vec<String>>,
    #[serde(default)]
    pub required_agent_tool_counts: HashMap<String, usize>,
    #[serde(default)]
    pub forbidden_agent_tools: Vec<String>,
    #[serde(default)]
    pub stop_after_agent_tools: Vec<String>,
    #[serde(default)]
    pub max_agent_tool_calls: Option<usize>,
    #[serde(default)]
    pub max_repeated_action_warnings: Option<usize>,
    #[serde(default = "default_true")]
    pub completion_required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTrace {
    pub tool: String,
    pub arguments: Value,
    pub success: bool,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmTurnTrace {
    pub request_messages: Value,
    pub assistant: Option<AssistantTrace>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTrace {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallTrace>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallTrace {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicScore {
    pub score: f32,
    pub completed: bool,
    pub steps: usize,
    pub required_tools_missing: Vec<String>,
    pub required_agent_tools_missing: Vec<String>,
    pub required_agent_tool_groups_missing: Vec<Vec<String>>,
    pub required_agent_tool_counts_missing: Vec<String>,
    pub forbidden_tool_calls: usize,
    pub forbidden_agent_tool_calls: usize,
    pub invalid_tool_errors: usize,
    pub repeated_action_warnings: usize,
    pub agent_tool_calls: usize,
    pub max_agent_tool_calls_excess: usize,
    pub max_repeated_action_warnings_excess: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticJudgeReport {
    pub score: f32,
    pub verdict: String,
    pub failure_class: String,
    pub root_cause: String,
    #[serde(default)]
    pub prompt_feedback: Vec<String>,
    #[serde(default)]
    pub recommended_prompt_patch: String,
    pub overfit_risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub scenario_id: String,
    pub prompt_sha: String,
    pub deterministic: DeterministicScore,
    pub semantic_judge: Option<SemanticJudgeReport>,
    pub final_score: f32,
    pub tool_trace: Vec<ToolTrace>,
    pub llm_trace: Vec<LlmTurnTrace>,
    pub events: Vec<Value>,
    pub eval_halt: Option<EvalHalt>,
    pub privacy: PrivacyReport,
    pub run_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSuiteReport {
    pub scenario_count: usize,
    pub final_score_mean: f32,
    pub deterministic_score_mean: f32,
    pub prompt_sha: Option<String>,
    pub reports: Vec<EvalReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalHalt {
    pub reason: String,
    pub agent_tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyReport {
    pub synthetic_fixture_only: bool,
    pub screenshots_omitted: bool,
    pub secrets_redacted: bool,
    pub local_paths_redacted: bool,
}

pub struct ScenarioMcp {
    tools: Vec<Value>,
    behaviors: HashMap<String, ToolBehavior>,
    state: Mutex<HashMap<String, Value>>,
    call_counts: Mutex<HashMap<String, usize>>,
    calls: Mutex<Vec<ToolTrace>>,
}

impl ScenarioMcp {
    pub fn new(scenario: &EvalScenario) -> Self {
        let tools = scenario
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters.clone().unwrap_or_else(|| {
                            json!({"type": "object", "properties": {}})
                        })
                    }
                })
            })
            .collect();
        let behaviors = scenario
            .tool_behaviors
            .iter()
            .map(|b| (b.tool.clone(), b.clone()))
            .collect();
        Self {
            tools,
            behaviors,
            state: Mutex::new(HashMap::new()),
            call_counts: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn traces(&self) -> Vec<ToolTrace> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, tool: &str, arguments: Option<Value>, success: bool, result: String) {
        self.calls.lock().unwrap().push(ToolTrace {
            tool: tool.to_string(),
            arguments: redact_value(arguments.unwrap_or(Value::Null)),
            success,
            result: redact_text(&result),
        });
    }

    fn next_response(&self, tool: &str, behavior: &ToolBehavior) -> ToolResponse {
        let mut counts = self.call_counts.lock().unwrap();
        let idx = counts.entry(tool.to_string()).or_insert(0);
        let call_idx = *idx;
        *idx += 1;

        if behavior.response_sequence.is_empty() {
            return ToolResponse {
                response: behavior.response.clone(),
                error: behavior.error,
                sets_state: behavior.sets_state.clone(),
            };
        }
        behavior.response_sequence[call_idx.min(behavior.response_sequence.len() - 1)].clone()
    }
}

impl Mcp for ScenarioMcp {
    async fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<ToolCallResult> {
        let args = arguments.clone().unwrap_or(Value::Null);
        let Some(behavior) = self.behaviors.get(name) else {
            let result = "ok".to_string();
            self.record(name, arguments, true, result.clone());
            return Ok(text_result(result, false));
        };

        for required in &behavior.required_args {
            if args.get(required).is_none_or(Value::is_null) {
                let result = format!("missing required argument: {required}");
                self.record(name, arguments, false, result.clone());
                return Ok(text_result(result, true));
            }
        }

        {
            let state = self.state.lock().unwrap();
            for (key, expected) in &behavior.requires_state {
                if state.get(key) != Some(expected) {
                    let result = format!("state requirement not met: {key}");
                    self.record(name, arguments, false, result.clone());
                    return Ok(text_result(result, true));
                }
            }
        }

        let outcome = self.next_response(name, behavior);

        if outcome.error {
            let result = outcome
                .response
                .as_ref()
                .map(response_text)
                .unwrap_or_else(|| "synthetic error".to_string());
            self.record(name, arguments, false, result.clone());
            return Ok(text_result(result, true));
        }

        if !outcome.sets_state.is_empty() {
            let mut state = self.state.lock().unwrap();
            for (key, value) in &outcome.sets_state {
                state.insert(key.clone(), value.clone());
            }
        }

        let result = outcome
            .response
            .as_ref()
            .map(response_text)
            .unwrap_or_else(|| "ok".to_string());
        self.record(name, arguments, true, result.clone());
        Ok(text_result(result, false))
    }

    fn has_tool(&self, name: &str) -> bool {
        if !self
            .tools
            .iter()
            .any(|tool| tool.pointer("/function/name").and_then(Value::as_str) == Some(name))
        {
            return false;
        }
        let Some(behavior) = self.behaviors.get(name) else {
            return true;
        };
        if behavior.requires_state.is_empty() {
            return true;
        }
        let state = self.state.lock().unwrap();
        behavior
            .requires_state
            .iter()
            .all(|(key, expected)| state.get(key) == Some(expected))
    }

    fn tools_as_openai(&self) -> Vec<Value> {
        self.tools.clone()
    }

    async fn refresh_server_tool_list(&self) -> Result<()> {
        Ok(())
    }
}

pub fn load_scenarios_dir(path: &Path) -> Result<Vec<EvalScenario>> {
    let mut files: Vec<PathBuf> = fs::read_dir(path)
        .with_context(|| format!("read scenario dir {}", path.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("json")).then_some(path)
        })
        .collect();
    files.sort();

    let mut scenarios = Vec::with_capacity(files.len());
    for file in files {
        scenarios.push(EvalScenario::load(&file)?);
    }
    Ok(scenarios)
}

fn text_result(text: String, is_error: bool) -> ToolCallResult {
    ToolCallResult {
        content: vec![ToolContent::Text { text }],
        is_error: is_error.then_some(true),
    }
}

fn response_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "ok".to_string()),
    }
}

pub struct RecordingBackend<B> {
    inner: B,
    turns: Mutex<Vec<LlmTurnTrace>>,
    stop_after_agent_tools: HashSet<String>,
    eval_halt: Mutex<Option<EvalHalt>>,
}

impl<B> RecordingBackend<B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            turns: Mutex::new(Vec::new()),
            stop_after_agent_tools: HashSet::new(),
            eval_halt: Mutex::new(None),
        }
    }

    pub fn with_stop_after_agent_tools(inner: B, stop_after_agent_tools: &[String]) -> Self {
        Self {
            inner,
            turns: Mutex::new(Vec::new()),
            stop_after_agent_tools: stop_after_agent_tools.iter().cloned().collect(),
            eval_halt: Mutex::new(None),
        }
    }

    pub fn traces(&self) -> Vec<LlmTurnTrace> {
        self.turns.lock().unwrap().clone()
    }

    pub fn eval_halt(&self) -> Option<EvalHalt> {
        self.eval_halt.lock().unwrap().clone()
    }

    fn maybe_record_eval_halt(&self, assistant: &Option<AssistantTrace>) -> bool {
        if self.stop_after_agent_tools.is_empty() {
            return false;
        }
        let Some(tool) = assistant
            .as_ref()
            .and_then(|assistant| {
                assistant
                    .tool_calls
                    .iter()
                    .find(|call| self.stop_after_agent_tools.contains(&call.name))
            })
            .map(|call| call.name.clone())
        else {
            return false;
        };
        let mut halt = self.eval_halt.lock().unwrap();
        if halt.is_none() {
            *halt = Some(EvalHalt {
                reason: "stop_after_agent_tools".to_string(),
                agent_tool: tool,
            });
            return true;
        }
        false
    }
}

#[derive(Debug)]
struct EvalHaltTriggered;

impl fmt::Display for EvalHaltTriggered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("eval halted after configured agent tool")
    }
}

impl Error for EvalHaltTriggered {}

impl<B: ChatBackend> ChatBackend for RecordingBackend<B> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn chat_with_options(
        &self,
        messages: &[Message],
        tools: Option<&[Value]>,
        options: &ChatOptions,
    ) -> Result<ChatResponse> {
        let request_messages = redact_messages(messages)?;
        match self.inner.chat_with_options(messages, tools, options).await {
            Ok(response) => {
                let assistant = response.choices.first().map(|choice| AssistantTrace {
                    content: choice.message.content_text().map(redact_text),
                    tool_calls: choice
                        .message
                        .tool_calls
                        .as_ref()
                        .map(|calls| calls.iter().map(redact_tool_call).collect())
                        .unwrap_or_default(),
                    finish_reason: choice.finish_reason.clone(),
                });
                self.turns.lock().unwrap().push(LlmTurnTrace {
                    request_messages,
                    assistant: assistant.clone(),
                    error: None,
                });
                if self.maybe_record_eval_halt(&assistant) {
                    return Err(EvalHaltTriggered.into());
                }
                Ok(response)
            }
            Err(err) => {
                self.turns.lock().unwrap().push(LlmTurnTrace {
                    request_messages,
                    assistant: None,
                    error: Some(redact_text(&err.to_string())),
                });
                Err(err)
            }
        }
    }

    async fn fetch_model_info(&self) -> Result<Option<clickweave_llm::ModelInfo>> {
        self.inner.fetch_model_info().await
    }
}

pub async fn run_eval<B, J>(
    scenario: EvalScenario,
    agent: B,
    agent_system_prompt: Option<String>,
    judge: Option<&J>,
) -> Result<EvalReport>
where
    B: ChatBackend,
    J: ChatBackend,
{
    scenario.validate_privacy()?;
    if let Some(prompt) = agent_system_prompt.as_deref()
        && personal_marker(prompt).is_some()
    {
        bail!("agent prompt candidate appears to contain private material");
    }
    let default_prompt = include_str!("../../clickweave-engine/prompts/agent_system.md");
    let prompt_sha = prompt_sha(agent_system_prompt.as_deref().unwrap_or(default_prompt));
    let mcp = ScenarioMcp::new(&scenario);
    let recording_agent = if scenario.scoring.stop_after_agent_tools.is_empty() {
        RecordingBackend::new(agent)
    } else {
        RecordingBackend::with_stop_after_agent_tools(
            agent,
            &scenario.scoring.stop_after_agent_tools,
        )
    };
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<RunnerOutput>(256);
    let (approval_tx, _approval_rx) = tokio::sync::mpsc::channel(1);

    let mut config = AgentConfig {
        max_steps: scenario.max_steps,
        ..AgentConfig::default()
    };
    config.allow_focus_window = false;

    let run = run_agent_workflow_with_prompt_override(
        &recording_agent,
        config,
        scenario.goal.clone(),
        &mcp,
        Some(AgentChannels {
            event_tx,
            approval_tx,
        }),
        None,
        Some(PermissionPolicy {
            allow_all: true,
            ..PermissionPolicy::default()
        }),
        uuid::Uuid::new_v4(),
        None,
        None,
        None,
        None,
        None,
        agent_system_prompt,
    )
    .await;
    let eval_halt = recording_agent.eval_halt();
    let (completed, state_steps, run_error) = match run {
        Ok((state, _writer)) => (state.completed, state.steps.len(), None),
        Err(err)
            if eval_halt.is_some() && err.chain().any(|cause| cause.is::<EvalHaltTriggered>()) =>
        {
            (false, 0, None)
        }
        Err(err) => (false, 0, Some(redact_text(&err.to_string()))),
    };

    let mut events = Vec::new();
    while let Ok(output) = event_rx.try_recv() {
        if let Some(event) = output.into_event() {
            events.push(redact_value(serde_json::to_value(event)?));
        }
    }

    let llm_trace = recording_agent.traces();
    let tool_trace = mcp.traces();
    let steps = if eval_halt.is_some() {
        count_step_events(&events)
    } else {
        state_steps
    };
    let deterministic = score_deterministic(
        &scenario,
        completed,
        steps,
        &tool_trace,
        &llm_trace,
        &events,
    );
    let semantic_judge = if let Some(judge_backend) = judge {
        Some(
            run_semantic_judge(
                judge_backend,
                &scenario,
                &prompt_sha,
                &deterministic,
                &tool_trace,
                &llm_trace,
                run_error.as_deref(),
            )
            .await?,
        )
    } else {
        None
    };
    let judge_score = semantic_judge.as_ref().map(|j| j.score).unwrap_or(0.0);
    let final_score = if semantic_judge.is_some() {
        deterministic.score * 0.8 + judge_score * 0.2
    } else {
        deterministic.score
    };

    Ok(EvalReport {
        scenario_id: scenario.id,
        prompt_sha,
        deterministic,
        semantic_judge,
        final_score,
        tool_trace,
        llm_trace,
        events,
        eval_halt,
        privacy: PrivacyReport {
            synthetic_fixture_only: true,
            screenshots_omitted: true,
            secrets_redacted: true,
            local_paths_redacted: true,
        },
        run_error,
    })
}

fn count_step_events(events: &[Value]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event.get("type").and_then(Value::as_str),
                Some("step_completed" | "step_failed")
            )
        })
        .count()
}

fn score_deterministic(
    scenario: &EvalScenario,
    completed: bool,
    steps: usize,
    tool_trace: &[ToolTrace],
    llm_trace: &[LlmTurnTrace],
    events: &[Value],
) -> DeterministicScore {
    let seen: HashSet<&str> = tool_trace.iter().map(|call| call.tool.as_str()).collect();
    let agent_tool_names: Vec<&str> = llm_trace
        .iter()
        .filter_map(|turn| turn.assistant.as_ref())
        .flat_map(|assistant| assistant.tool_calls.iter().map(|call| call.name.as_str()))
        .collect();
    let agent_seen: HashSet<&str> = agent_tool_names.iter().copied().collect();
    let required_tools_missing: Vec<String> = scenario
        .scoring
        .required_tools
        .iter()
        .filter(|tool| !seen.contains(tool.as_str()))
        .cloned()
        .collect();
    let required_agent_tools_missing: Vec<String> = scenario
        .scoring
        .required_agent_tools
        .iter()
        .filter(|tool| !agent_seen.contains(tool.as_str()))
        .cloned()
        .collect();
    let required_agent_tool_groups_missing: Vec<Vec<String>> = scenario
        .scoring
        .required_agent_tool_groups
        .iter()
        .filter(|group| !group.iter().any(|tool| agent_seen.contains(tool.as_str())))
        .cloned()
        .collect();
    let mut agent_counts: HashMap<&str, usize> = HashMap::new();
    for name in &agent_tool_names {
        *agent_counts.entry(name).or_insert(0) += 1;
    }
    let required_agent_tool_counts_missing: Vec<String> = scenario
        .scoring
        .required_agent_tool_counts
        .iter()
        .filter_map(|(tool, required)| {
            let actual = agent_counts.get(tool.as_str()).copied().unwrap_or(0);
            (actual < *required).then(|| format!("{tool}: required {required}, saw {actual}"))
        })
        .collect();
    let forbidden: HashSet<&str> = scenario
        .scoring
        .forbidden_tools
        .iter()
        .map(String::as_str)
        .collect();
    let forbidden_tool_calls = tool_trace
        .iter()
        .filter(|call| forbidden.contains(call.tool.as_str()))
        .count();
    let forbidden_agent: HashSet<&str> = scenario
        .scoring
        .forbidden_agent_tools
        .iter()
        .map(String::as_str)
        .collect();
    let forbidden_agent_tool_calls = agent_tool_names
        .iter()
        .filter(|tool| forbidden_agent.contains(**tool))
        .count();
    let allowed_error_tools: HashSet<&str> = scenario
        .scoring
        .allowed_error_tools
        .iter()
        .map(String::as_str)
        .collect();
    let invalid_tool_errors = tool_trace
        .iter()
        .filter(|call| !call.success && !allowed_error_tools.contains(call.tool.as_str()))
        .count();
    let repeated_action_warnings = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(Value::as_str) == Some("warning")
                && event
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.contains("repeated"))
        })
        .count();
    let agent_tool_calls = agent_tool_names.len();
    let max_agent_tool_calls_excess = scenario
        .scoring
        .max_agent_tool_calls
        .map(|max| agent_tool_calls.saturating_sub(max))
        .unwrap_or_default();
    let max_repeated_action_warnings_excess = scenario
        .scoring
        .max_repeated_action_warnings
        .map(|max| repeated_action_warnings.saturating_sub(max))
        .unwrap_or_default();

    let mut score = 1.0_f32;
    if scenario.scoring.completion_required && !completed {
        score -= 0.35;
    }
    score -= required_tools_missing.len() as f32 * 0.12;
    score -= required_agent_tools_missing.len() as f32 * 0.12;
    score -= required_agent_tool_groups_missing.len() as f32 * 0.12;
    score -= required_agent_tool_counts_missing.len() as f32 * 0.12;
    score -= forbidden_tool_calls as f32 * 0.15;
    score -= forbidden_agent_tool_calls as f32 * 0.20;
    score -= invalid_tool_errors as f32 * 0.12;
    score -= repeated_action_warnings as f32 * 0.05;
    score -= max_agent_tool_calls_excess as f32 * 0.04;
    score -= max_repeated_action_warnings_excess as f32;
    if scenario.max_steps > 0 {
        score -= (steps as f32 / scenario.max_steps as f32).min(1.0) * 0.08;
    }

    DeterministicScore {
        score: score.clamp(0.0, 1.0),
        completed,
        steps,
        required_tools_missing,
        required_agent_tools_missing,
        required_agent_tool_groups_missing,
        required_agent_tool_counts_missing,
        forbidden_tool_calls,
        forbidden_agent_tool_calls,
        invalid_tool_errors,
        repeated_action_warnings,
        agent_tool_calls,
        max_agent_tool_calls_excess,
        max_repeated_action_warnings_excess,
    }
}

async fn run_semantic_judge<J: ChatBackend>(
    judge: &J,
    scenario: &EvalScenario,
    prompt_sha: &str,
    deterministic: &DeterministicScore,
    tool_trace: &[ToolTrace],
    llm_trace: &[LlmTurnTrace],
    run_error: Option<&str>,
) -> Result<SemanticJudgeReport> {
    let input = json!({
        "scenario": {
            "id": scenario.id,
            "description": scenario.description,
            "goal": redact_text(&scenario.goal),
            "scoring": scenario.scoring,
        },
        "prompt_sha": prompt_sha,
        "deterministic": deterministic,
        "tool_trace": tool_trace,
        "llm_trace": llm_trace,
        "run_error": run_error,
        "privacy": {
            "synthetic_fixture_only": true,
            "screenshots_omitted": true,
            "paths_and_secrets_redacted": true
        }
    });
    let messages = vec![
        Message::system(CODEX_JUDGE_PROMPT),
        Message::user(serde_json::to_string_pretty(&input)?),
    ];
    let response = judge
        .chat_with_options(
            &messages,
            None,
            &ChatOptions {
                temperature: Some(0.0),
                max_tokens: Some(2048),
            },
        )
        .await?;
    let text = response
        .choices
        .first()
        .and_then(|choice| choice.message.content_text())
        .context("judge returned no text")?;
    parse_judge_report(text)
}

pub fn parse_judge_report(text: &str) -> Result<SemanticJudgeReport> {
    let json_text = extract_json_object(text).context("judge response did not contain JSON")?;
    let mut report: SemanticJudgeReport =
        serde_json::from_str(json_text).context("parse judge JSON")?;
    report.score = report.score.clamp(0.0, 1.0);
    report.root_cause = redact_text(&report.root_cause);
    report.recommended_prompt_patch = redact_text(&report.recommended_prompt_patch);
    report.prompt_feedback = report
        .prompt_feedback
        .into_iter()
        .map(|s| redact_text(&s))
        .collect();
    Ok(report)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (start <= end).then_some(&text[start..=end])
}

fn redact_messages(messages: &[Message]) -> Result<Value> {
    let mut value = serde_json::to_value(messages)?;
    if let Value::Array(items) = &mut value {
        for item in items {
            if item.get("role").and_then(Value::as_str) == Some("system") {
                let content_sha = item.get("content").and_then(Value::as_str).map(prompt_sha);
                if let Value::Object(obj) = item {
                    obj.insert(
                        "content".to_string(),
                        Value::String("[SYSTEM_PROMPT_OMITTED]".to_string()),
                    );
                    if let Some(sha) = content_sha {
                        obj.insert("content_sha".to_string(), Value::String(sha));
                    }
                }
            }
        }
    }
    Ok(redact_value(value))
}

fn redact_tool_call(call: &ToolCall) -> ToolCallTrace {
    ToolCallTrace {
        name: call.function.name.clone(),
        arguments: redact_value(call.function.arguments.clone()),
    }
}

pub fn redact_value(value: Value) -> Value {
    match value {
        Value::String(s) => Value::String(redact_text(&s)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        Value::Object(obj) => {
            let mut redacted = Map::new();
            for (key, value) in obj {
                let lowered = key.to_lowercase();
                if lowered.contains("api_key")
                    || lowered.contains("authorization")
                    || lowered.contains("token")
                    || lowered.contains("secret")
                    || lowered.contains("password")
                {
                    redacted.insert(key, Value::String("[REDACTED_SECRET]".to_string()));
                } else if lowered == "image_url" || lowered.contains("base64") {
                    redacted.insert(key, Value::String("[IMAGE_OMITTED]".to_string()));
                } else {
                    redacted.insert(key, redact_value(value));
                }
            }
            Value::Object(redacted)
        }
        other => other,
    }
}

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        out = out.replace(&home, "[REDACTED_HOME]");
    }
    if contains_local_path(&out) {
        return "[REDACTED_PATH_CONTEXT]".to_string();
    }
    if looks_like_email(&out) || looks_like_phone(&out) {
        return "[REDACTED_PERSONAL_CONTEXT]".to_string();
    }
    if contains_http_url(&out) {
        return "[REDACTED_URL_CONTEXT]".to_string();
    }
    for marker in [
        "Bearer ",
        "api_key",
        "authorization",
        "private_key",
        "password",
    ] {
        if out.to_lowercase().contains(&marker.to_lowercase()) {
            out = "[REDACTED_SECRET_CONTEXT]".to_string();
            break;
        }
    }
    if out.starts_with("data:image/") {
        return "[IMAGE_OMITTED]".to_string();
    }
    const MAX_TEXT: usize = 6000;
    if out.len() > MAX_TEXT {
        out.truncate(MAX_TEXT);
        out.push_str("...[TRUNCATED]");
    }
    out
}

fn private_marker(input: &str) -> Option<&'static str> {
    let lowered = input.to_lowercase();
    for marker in [
        "/users/",
        "\\users\\",
        "/home/",
        "~/",
        "%appdata%",
        "application support",
        "api_key",
        "authorization",
        "private_key",
        "begin rsa",
        "begin openssh",
        "bearer ",
        "password",
    ] {
        if lowered.contains(marker) {
            return Some(marker);
        }
    }
    if lowered.contains("\"secret\"")
        || lowered.contains("secret_")
        || lowered.contains("\"token\"")
        || lowered.contains("token_")
    {
        return Some("secret");
    }
    personal_marker(input)
}

fn personal_marker(input: &str) -> Option<&'static str> {
    if contains_local_path(input) {
        return Some("path");
    }
    if looks_like_email(input) {
        return Some("email");
    }
    if looks_like_phone(input) {
        return Some("phone");
    }
    if contains_http_url(input) {
        return Some("url");
    }
    None
}

fn contains_local_path(input: &str) -> bool {
    let lowered = input.to_lowercase();
    lowered.contains("/users/")
        || lowered.contains("\\users\\")
        || lowered.contains("/home/")
        || lowered.contains("~/")
        || lowered.contains("%appdata%")
        || lowered.contains("application support")
}

fn contains_http_url(input: &str) -> bool {
    let lowered = input.to_lowercase();
    lowered.contains("http://") || lowered.contains("https://")
}

fn looks_like_email(input: &str) -> bool {
    input
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | '<' | '>' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}'
                )
        })
        .any(|token| {
            let Some((local, domain)) = token.split_once('@') else {
                return false;
            };
            !local.is_empty()
                && domain.contains('.')
                && domain
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        })
}

fn looks_like_phone(input: &str) -> bool {
    let mut digits = 0usize;
    let mut span = 0usize;
    for c in input.chars() {
        if c.is_ascii_digit() {
            digits += 1;
            span += 1;
        } else if matches!(c, '+' | '-' | '(' | ')' | '.' | ' ') && span > 0 {
            span += 1;
        } else {
            digits = 0;
            span = 0;
        }
        if digits >= 10 && span <= 30 {
            return true;
        }
        if span > 30 {
            digits = 0;
            span = 0;
        }
    }
    false
}

fn prompt_sha(prompt: &str) -> String {
    blake3::hash(prompt.as_bytes()).to_hex()[..16].to_string()
}

pub fn llm_config(base_url: String, model: String, api_key: Option<String>) -> LlmConfig {
    LlmConfig {
        base_url,
        model,
        api_key: api_key.filter(|key| !key.is_empty()),
        temperature: Some(0.0),
        max_tokens: Some(2048),
        ..LlmConfig::default()
    }
    .with_thinking(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_engine::agent::test_stubs::{ScriptedLlm, llm_reply_tool};

    fn scenario() -> EvalScenario {
        serde_json::from_str(include_str!("../scenarios/synthetic_electron_pre_cdp.json")).unwrap()
    }

    #[tokio::test]
    async fn deterministic_score_penalizes_forbidden_visual_fallback() {
        let llm = ScriptedLlm::new(vec![
            llm_reply_tool("find_text", json!({"text": "Message"})),
            llm_reply_tool("agent_done", json!({"summary": "done"})),
        ]);
        let report = run_eval::<_, ScriptedLlm>(scenario(), llm, None, None)
            .await
            .unwrap();
        assert!(report.deterministic.forbidden_tool_calls >= 1);
        assert!(report.final_score < 1.0);
        assert!(
            serde_json::to_string(&report)
                .unwrap()
                .contains("[SYSTEM_PROMPT_OMITTED]")
        );
    }

    #[tokio::test]
    async fn stop_after_agent_tool_halts_after_recording_target_action() {
        let mut scenario = scenario();
        scenario.scoring.required_tools.clear();
        scenario.scoring.required_agent_tools = vec!["agent_replan".to_string()];
        scenario.scoring.required_agent_tool_groups.clear();
        scenario.scoring.required_agent_tool_counts.clear();
        scenario.scoring.forbidden_tools.clear();
        scenario.scoring.forbidden_agent_tools.clear();
        scenario.scoring.stop_after_agent_tools = vec!["agent_replan".to_string()];
        scenario.scoring.max_agent_tool_calls = Some(1);
        scenario.scoring.max_repeated_action_warnings = Some(0);
        scenario.scoring.completion_required = false;

        let llm = ScriptedLlm::new(vec![
            llm_reply_tool("agent_replan", json!({"reason": "target absent"})),
            llm_reply_tool("agent_done", json!({"summary": "should not run"})),
        ]);

        let report = run_eval::<_, ScriptedLlm>(scenario, llm, None, None)
            .await
            .unwrap();

        let halt = report.eval_halt.as_ref().expect("eval should halt");
        assert_eq!(halt.reason, "stop_after_agent_tools");
        assert_eq!(halt.agent_tool, "agent_replan");
        assert!(report.run_error.is_none());
        assert_eq!(report.llm_trace.len(), 1);
        assert_eq!(report.deterministic.agent_tool_calls, 1);
        assert_eq!(report.deterministic.max_agent_tool_calls_excess, 0);
        assert!(report.deterministic.required_agent_tools_missing.is_empty());
        assert!(!report.deterministic.completed);
        assert!(report.final_score > 0.99);
    }

    #[test]
    fn deterministic_score_can_hard_fail_repeated_action_warnings() {
        let mut scenario = scenario();
        scenario.scoring.required_tools.clear();
        scenario.scoring.required_agent_tools.clear();
        scenario.scoring.required_agent_tool_groups.clear();
        scenario.scoring.required_agent_tool_counts.clear();
        scenario.scoring.forbidden_tools.clear();
        scenario.scoring.forbidden_agent_tools.clear();
        scenario.scoring.max_agent_tool_calls = None;
        scenario.scoring.max_repeated_action_warnings = Some(0);
        scenario.scoring.completion_required = false;

        let score = score_deterministic(
            &scenario,
            false,
            0,
            &[],
            &[],
            &[json!({
                "type": "warning",
                "message": "no-progress: repeated action cycle `cdp_fill` -> `cdp_click`"
            })],
        );

        assert_eq!(score.repeated_action_warnings, 1);
        assert_eq!(score.max_repeated_action_warnings_excess, 1);
        assert_eq!(score.score, 0.0);
    }

    #[test]
    fn redaction_removes_home_and_image_payloads() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "synthetic-home".to_string());
        let raw = json!({
            "path": format!("{home}/private/project"),
            "image_url": "data:image/png;base64,abc",
            "api_key": "secret"
        });
        let out = redact_value(raw);
        let s = serde_json::to_string(&out).unwrap();
        assert!(!s.contains(&home));
        assert!(!s.contains("abc"));
        assert!(!s.contains("secret"));
    }

    #[test]
    fn scenario_privacy_gate_rejects_personal_markers() {
        let mut scenario = scenario();
        scenario.goal = "Send a note to someone@example.com".to_string();
        assert!(scenario.validate_privacy().is_err());
    }

    #[test]
    fn scenario_privacy_gate_requires_synthetic_prefix() {
        let mut scenario = scenario();
        scenario.id = "electron_pre_cdp".to_string();
        assert!(scenario.validate_privacy().is_err());
    }

    #[test]
    fn bundled_scenarios_are_valid_synthetic_fixtures() {
        let scenarios =
            load_scenarios_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")).unwrap();
        assert!(scenarios.len() >= 6);
        assert!(
            scenarios
                .iter()
                .all(|scenario| scenario.id.starts_with("synthetic_"))
        );
    }

    #[tokio::test]
    async fn cdp_tools_are_hidden_until_synthetic_connect_state() {
        let scenario = scenario();
        let mcp = ScenarioMcp::new(&scenario);
        assert!(!mcp.has_tool("cdp_find_elements"));

        mcp.call_tool("cdp_connect", Some(json!({"port": 12345})))
            .await
            .unwrap();

        assert!(mcp.has_tool("cdp_find_elements"));
    }

    #[test]
    fn parses_judge_json_with_surrounding_text() {
        let parsed = parse_judge_report(
            r#"Here:
            {"score":0.7,"verdict":"partial","failure_class":"prompt_misroutes","root_cause":"x","prompt_feedback":["y"],"recommended_prompt_patch":"","overfit_risk":"low"}
            "#,
        )
        .unwrap();
        assert_eq!(parsed.verdict, "partial");
        assert_eq!(parsed.score, 0.7);
    }
}
