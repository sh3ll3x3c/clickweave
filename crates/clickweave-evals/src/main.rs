use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clickweave_evals::{
    EvalReport, EvalScenario, EvalSuiteReport, llm_config, load_scenarios_dir, run_eval,
};
use clickweave_llm::{LlmClient, LlmConfig};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let suite_mode = args.scenario_dir.is_some();
    let scenarios = match (&args.scenario, &args.scenario_dir) {
        (Some(path), None) => vec![EvalScenario::load(path)?],
        (None, Some(path)) => load_scenarios_dir(path)?,
        _ => bail!("provide exactly one of --scenario or --scenario-dir"),
    };
    let prompt = match args.agent_prompt {
        Some(path) => Some(fs::read_to_string(&path).context("read agent prompt file")?),
        None => None,
    };

    let agent_base_url = args
        .agent_base_url
        .or_else(|| env::var("CLICKWEAVE_EVAL_AGENT_BASE_URL").ok())
        .context("missing --agent-base-url or CLICKWEAVE_EVAL_AGENT_BASE_URL")?;
    let agent_model = args
        .agent_model
        .or_else(|| env::var("CLICKWEAVE_EVAL_AGENT_MODEL").ok())
        .context("missing --agent-model or CLICKWEAVE_EVAL_AGENT_MODEL")?;
    let agent_api_key = args
        .agent_api_key
        .or_else(|| env::var("CLICKWEAVE_EVAL_AGENT_API_KEY").ok());
    let agent_config = llm_config(agent_base_url, agent_model, agent_api_key);

    let judge_config = match (
        args.judge_base_url
            .or_else(|| env::var("CLICKWEAVE_EVAL_JUDGE_BASE_URL").ok()),
        args.judge_model
            .or_else(|| env::var("CLICKWEAVE_EVAL_JUDGE_MODEL").ok()),
    ) {
        (Some(base_url), Some(model)) => {
            let api_key = args
                .judge_api_key
                .or_else(|| env::var("CLICKWEAVE_EVAL_JUDGE_API_KEY").ok());
            Some(llm_config(base_url, model, api_key))
        }
        (None, None) => None,
        _ => bail!("judge config requires both base URL and model"),
    };

    let mut reports = run_scenarios(
        scenarios,
        prompt,
        agent_config,
        judge_config,
        args.concurrency,
    )
    .await?;
    let output = if suite_mode {
        serde_json::to_string_pretty(&suite_report(reports))?
    } else {
        serde_json::to_string_pretty(&reports.remove(0))?
    };
    if let Some(path) = args.out {
        fs::write(&path, format!("{output}\n")).context("write eval report")?;
    } else {
        println!("{output}");
    }
    Ok(())
}

async fn run_scenarios(
    scenarios: Vec<EvalScenario>,
    prompt: Option<String>,
    agent_config: LlmConfig,
    judge_config: Option<LlmConfig>,
    concurrency: usize,
) -> Result<Vec<EvalReport>> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::with_capacity(scenarios.len());

    for (index, scenario) in scenarios.into_iter().enumerate() {
        let semaphore = Arc::clone(&semaphore);
        let prompt = prompt.clone();
        let agent_config = agent_config.clone();
        let judge_config = judge_config.clone();
        handles.push(tokio::spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .context("eval concurrency semaphore closed")?;
            let agent = LlmClient::new(agent_config);
            let report = match judge_config {
                Some(config) => {
                    let judge = LlmClient::new(config);
                    run_eval(scenario, agent, prompt, Some(&judge)).await?
                }
                None => run_eval::<_, LlmClient>(scenario, agent, prompt, None).await?,
            };
            Ok::<_, anyhow::Error>((index, report))
        }));
    }

    let mut first_error = None;
    let mut indexed_reports = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(Ok(report)) => indexed_reports.push(report),
            Ok(Err(err)) => {
                first_error.get_or_insert(err);
            }
            Err(err) => {
                first_error.get_or_insert_with(|| anyhow!("eval task failed to join: {err}"));
            }
        }
    }
    if let Some(err) = first_error {
        return Err(err);
    }
    indexed_reports.sort_by_key(|(index, _)| *index);
    Ok(indexed_reports
        .into_iter()
        .map(|(_, report)| report)
        .collect())
}

fn suite_report(reports: Vec<clickweave_evals::EvalReport>) -> EvalSuiteReport {
    let scenario_count = reports.len();
    let final_score_mean = mean(reports.iter().map(|report| report.final_score));
    let deterministic_score_mean = mean(reports.iter().map(|report| report.deterministic.score));
    let prompt_sha = reports.first().map(|report| report.prompt_sha.clone());
    EvalSuiteReport {
        scenario_count,
        final_score_mean,
        deterministic_score_mean,
        prompt_sha,
        reports,
    }
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let mut count = 0usize;
    let mut total = 0.0_f32;
    for value in values {
        count += 1;
        total += value;
    }
    if count == 0 {
        0.0
    } else {
        total / count as f32
    }
}

#[derive(Debug)]
struct Args {
    scenario: Option<PathBuf>,
    scenario_dir: Option<PathBuf>,
    agent_prompt: Option<PathBuf>,
    agent_base_url: Option<String>,
    agent_model: Option<String>,
    agent_api_key: Option<String>,
    judge_base_url: Option<String>,
    judge_model: Option<String>,
    judge_api_key: Option<String>,
    concurrency: usize,
    out: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            scenario: None,
            scenario_dir: None,
            agent_prompt: None,
            agent_base_url: None,
            agent_model: None,
            agent_api_key: None,
            judge_base_url: None,
            judge_model: None,
            judge_api_key: None,
            concurrency: 1,
            out: None,
        }
    }
}

impl Args {
    fn parse() -> Result<Self> {
        Self::parse_from(env::args().skip(1))
    }

    fn parse_from<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let mut out = Args::default();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--scenario" => out.scenario = Some(PathBuf::from(next_value(&mut args, &flag)?)),
                "--scenario-dir" => {
                    out.scenario_dir = Some(PathBuf::from(next_value(&mut args, &flag)?))
                }
                "--agent-prompt" => {
                    out.agent_prompt = Some(PathBuf::from(next_value(&mut args, &flag)?))
                }
                "--agent-base-url" => out.agent_base_url = Some(next_value(&mut args, &flag)?),
                "--agent-model" => out.agent_model = Some(next_value(&mut args, &flag)?),
                "--agent-api-key" => out.agent_api_key = Some(next_value(&mut args, &flag)?),
                "--judge-base-url" => out.judge_base_url = Some(next_value(&mut args, &flag)?),
                "--judge-model" => out.judge_model = Some(next_value(&mut args, &flag)?),
                "--judge-api-key" => out.judge_api_key = Some(next_value(&mut args, &flag)?),
                "--concurrency" => {
                    let raw = next_value(&mut args, &flag)?;
                    out.concurrency = raw.parse().with_context(|| {
                        format!("--concurrency expects a positive integer, got {raw}")
                    })?;
                }
                "--out" => out.out = Some(PathBuf::from(next_value(&mut args, &flag)?)),
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => bail!("unknown argument: {other}"),
            }
        }
        if out.scenario.is_none() == out.scenario_dir.is_none() {
            bail!("provide exactly one of --scenario or --scenario-dir");
        }
        if out.concurrency == 0 {
            bail!("--concurrency must be greater than 0");
        }
        Ok(out)
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .with_context(|| format!("{flag} requires a value"))
}

fn print_help() {
    println!(
        "Usage: clickweave-evals (--scenario <file> | --scenario-dir <dir>) [--agent-prompt <md>] \\
         --agent-base-url <url> --agent-model <model> \\
         [--judge-base-url <url> --judge-model <model>] [--concurrency <n>] [--out <json>]\n\n\
         Environment fallbacks: CLICKWEAVE_EVAL_AGENT_BASE_URL, \\
         CLICKWEAVE_EVAL_AGENT_MODEL, CLICKWEAVE_EVAL_AGENT_API_KEY, \\
         CLICKWEAVE_EVAL_JUDGE_BASE_URL, CLICKWEAVE_EVAL_JUDGE_MODEL, \\
         CLICKWEAVE_EVAL_JUDGE_API_KEY"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_concurrency_defaults_to_one() {
        let args = Args::parse_from([
            "--scenario",
            "scenario.json",
            "--agent-base-url",
            "http://localhost:1234/v1",
            "--agent-model",
            "local-model",
        ])
        .unwrap();

        assert_eq!(args.concurrency, 1);
    }

    #[test]
    fn parse_concurrency_override() {
        let args = Args::parse_from([
            "--scenario-dir",
            "scenarios",
            "--agent-base-url",
            "http://localhost:1234/v1",
            "--agent-model",
            "local-model",
            "--concurrency",
            "2",
        ])
        .unwrap();

        assert_eq!(args.concurrency, 2);
    }

    #[test]
    fn parse_rejects_zero_concurrency() {
        let err = Args::parse_from([
            "--scenario-dir",
            "scenarios",
            "--agent-base-url",
            "http://localhost:1234/v1",
            "--agent-model",
            "local-model",
            "--concurrency",
            "0",
        ])
        .unwrap_err();

        assert!(err.to_string().contains("greater than 0"));
    }
}
