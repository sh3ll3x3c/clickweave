use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clickweave_evals::{EvalScenario, EvalSuiteReport, llm_config, load_scenarios_dir, run_eval};
use clickweave_llm::LlmClient;

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

    let mut reports = Vec::with_capacity(scenarios.len());
    match judge_config {
        Some(config) => {
            let judge = LlmClient::new(config);
            for scenario in scenarios {
                let agent = LlmClient::new(agent_config.clone());
                reports.push(run_eval(scenario, agent, prompt.clone(), Some(&judge)).await?);
            }
        }
        None => {
            for scenario in scenarios {
                let agent = LlmClient::new(agent_config.clone());
                reports
                    .push(run_eval::<_, LlmClient>(scenario, agent, prompt.clone(), None).await?);
            }
        }
    };
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

#[derive(Debug, Default)]
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
    out: Option<PathBuf>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = env::args().skip(1);
        let mut out = Args::default();
        while let Some(flag) = args.next() {
            let value = |args: &mut std::iter::Skip<env::Args>, flag: &str| -> Result<String> {
                args.next()
                    .with_context(|| format!("{flag} requires a value"))
            };
            match flag.as_str() {
                "--scenario" => out.scenario = Some(PathBuf::from(value(&mut args, &flag)?)),
                "--scenario-dir" => {
                    out.scenario_dir = Some(PathBuf::from(value(&mut args, &flag)?))
                }
                "--agent-prompt" => {
                    out.agent_prompt = Some(PathBuf::from(value(&mut args, &flag)?))
                }
                "--agent-base-url" => out.agent_base_url = Some(value(&mut args, &flag)?),
                "--agent-model" => out.agent_model = Some(value(&mut args, &flag)?),
                "--agent-api-key" => out.agent_api_key = Some(value(&mut args, &flag)?),
                "--judge-base-url" => out.judge_base_url = Some(value(&mut args, &flag)?),
                "--judge-model" => out.judge_model = Some(value(&mut args, &flag)?),
                "--judge-api-key" => out.judge_api_key = Some(value(&mut args, &flag)?),
                "--out" => out.out = Some(PathBuf::from(value(&mut args, &flag)?)),
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
        Ok(out)
    }
}

fn print_help() {
    println!(
        "Usage: clickweave-evals (--scenario <file> | --scenario-dir <dir>) [--agent-prompt <md>] \\
         --agent-base-url <url> --agent-model <model> \\
         [--judge-base-url <url> --judge-model <model>] [--out <json>]\n\n\
         Environment fallbacks: CLICKWEAVE_EVAL_AGENT_BASE_URL, \\
         CLICKWEAVE_EVAL_AGENT_MODEL, CLICKWEAVE_EVAL_AGENT_API_KEY, \\
         CLICKWEAVE_EVAL_JUDGE_BASE_URL, CLICKWEAVE_EVAL_JUDGE_MODEL, \\
         CLICKWEAVE_EVAL_JUDGE_API_KEY"
    );
}
