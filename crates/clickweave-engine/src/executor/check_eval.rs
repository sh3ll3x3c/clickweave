use clickweave_core::{Check, CheckResult, CheckType, CheckVerdict, NodeVerdict, OnCheckFail};
use clickweave_llm::{ChatBackend, Message};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Deserialize)]
struct LlmCheckResult {
    check_name: String,
    verdict: String,
    reasoning: String,
}

fn system_prompt() -> String {
    "You are evaluating whether a UI automation step produced the expected results. \
     You will receive check criteria, trace events from the step's execution, and optionally \
     a screenshot taken after the step completed.\n\n\
     For each check, respond with ONLY a JSON array (no markdown fences):\n\
     [{\"check_name\": \"...\", \"verdict\": \"pass\" or \"fail\", \"reasoning\": \"...\"}]\n\n\
     Be precise: only mark 'pass' if the evidence clearly supports it."
        .to_string()
}

fn format_check_type(ct: &CheckType) -> &'static str {
    match ct {
        CheckType::TextPresent => "TextPresent",
        CheckType::TextAbsent => "TextAbsent",
        CheckType::TemplateFound => "TemplateFound",
        CheckType::WindowTitleMatches => "WindowTitleMatches",
    }
}

fn format_checks(checks: &[Check], expected_outcome: &Option<String>) -> String {
    let mut lines = Vec::new();
    for (i, check) in checks.iter().enumerate() {
        let params_str = serde_json::to_string(&check.params).unwrap_or_default();
        lines.push(format!(
            "{}. {} ({}): {}",
            i + 1,
            check.name,
            format_check_type(&check.check_type),
            params_str
        ));
    }
    if let Some(outcome) = expected_outcome {
        lines.push(format!(
            "{}. Expected outcome: {}",
            lines.len() + 1,
            outcome
        ));
    }
    lines.join("\n")
}

fn build_user_message(
    node_name: &str,
    checks: &[Check],
    expected_outcome: &Option<String>,
    trace_summary: &str,
    screenshot_base64: Option<&str>,
) -> Message {
    let text = format!(
        "## Node: \"{}\"\n\n## Checks:\n{}\n\n## Trace events:\n{}",
        node_name,
        format_checks(checks, expected_outcome),
        if trace_summary.is_empty() {
            "(no trace events recorded)"
        } else {
            trace_summary
        }
    );

    match screenshot_base64 {
        Some(img_data) => {
            Message::user_with_images(text, vec![(img_data.to_string(), "image/png".to_string())])
        }
        None => Message::user(format!("{}\n\n(Screenshot unavailable)", text)),
    }
}

/// Parse LLM response into CheckResults, matching against the original checks.
fn parse_verdicts(
    response_text: &str,
    checks: &[Check],
    expected_outcome: &Option<String>,
) -> (Vec<CheckResult>, Option<CheckResult>) {
    let cleaned = response_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let llm_results: Vec<LlmCheckResult> = serde_json::from_str(cleaned).unwrap_or_default();

    let mut check_results = Vec::new();
    for check in checks {
        let llm_match = llm_results.iter().find(|r| r.check_name == check.name);

        let (verdict, reasoning) = match llm_match {
            Some(r) => {
                let v = match r.verdict.to_lowercase().as_str() {
                    "pass" => CheckVerdict::Pass,
                    "fail" => match check.on_fail {
                        OnCheckFail::FailNode => CheckVerdict::Fail,
                        OnCheckFail::WarnOnly => CheckVerdict::Warn,
                    },
                    _ => CheckVerdict::Fail,
                };
                (v, r.reasoning.clone())
            }
            None => (
                CheckVerdict::Fail,
                "LLM did not return a verdict for this check".to_string(),
            ),
        };

        check_results.push(CheckResult {
            check_name: check.name.clone(),
            check_type: check.check_type,
            verdict,
            reasoning,
        });
    }

    let expected_verdict = expected_outcome.as_ref().map(|_| {
        let llm_match = llm_results
            .iter()
            .find(|r| r.check_name == "Expected outcome");

        let (verdict, reasoning) = match llm_match {
            Some(r) => {
                let v = match r.verdict.to_lowercase().as_str() {
                    "pass" => CheckVerdict::Pass,
                    _ => CheckVerdict::Fail,
                };
                (v, r.reasoning.clone())
            }
            None => (
                CheckVerdict::Fail,
                "LLM did not return a verdict for expected outcome".to_string(),
            ),
        };

        CheckResult {
            check_name: "Expected outcome".to_string(),
            check_type: CheckType::TextPresent,
            verdict,
            reasoning,
        }
    });

    (check_results, expected_verdict)
}

fn has_hard_failure(verdict: &NodeVerdict) -> bool {
    verdict
        .check_results
        .iter()
        .any(|r| r.verdict == CheckVerdict::Fail)
        || verdict
            .expected_outcome_verdict
            .as_ref()
            .is_some_and(|r| r.verdict == CheckVerdict::Fail)
}

/// Run the check evaluation pass for all completed checked nodes.
/// Short-circuits on first FailNode failure.
pub(crate) async fn run_check_pass<C: ChatBackend>(
    backend: &C,
    completed_checks: &[(Uuid, Vec<Check>, Option<String>)],
    node_names: &HashMap<Uuid, String>,
    trace_summaries: &HashMap<Uuid, String>,
    screenshots: &HashMap<Uuid, String>,
    log: impl Fn(String),
) -> Vec<NodeVerdict> {
    let mut verdicts = Vec::new();

    if completed_checks.is_empty() {
        return verdicts;
    }

    log("Starting check evaluation pass".to_string());

    for (node_id, checks, expected_outcome) in completed_checks {
        let node_name = node_names
            .get(node_id)
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        log(format!("Evaluating checks for node: {}", node_name));

        let trace_summary = trace_summaries
            .get(node_id)
            .map(|s| s.as_str())
            .unwrap_or("");

        let screenshot_b64 = screenshots.get(node_id).map(|s| s.as_str());

        let messages = vec![
            Message::system(system_prompt()),
            build_user_message(
                node_name,
                checks,
                expected_outcome,
                trace_summary,
                screenshot_b64,
            ),
        ];

        let (check_results, expected_verdict) = match backend.chat(messages, None).await {
            Ok(response) => {
                let text = response
                    .choices
                    .first()
                    .and_then(|c| c.message.text_content())
                    .unwrap_or("");
                parse_verdicts(text, checks, expected_outcome)
            }
            Err(e) => {
                log(format!(
                    "LLM check evaluation failed for {}: {}",
                    node_name, e
                ));
                let fail_results: Vec<CheckResult> = checks
                    .iter()
                    .map(|c| CheckResult {
                        check_name: c.name.clone(),
                        check_type: c.check_type,
                        verdict: CheckVerdict::Fail,
                        reasoning: format!("Check evaluation failed: {}", e),
                    })
                    .collect();
                let fail_expected = expected_outcome.as_ref().map(|_| CheckResult {
                    check_name: "Expected outcome".to_string(),
                    check_type: CheckType::TextPresent,
                    verdict: CheckVerdict::Fail,
                    reasoning: format!("Check evaluation failed: {}", e),
                });
                (fail_results, fail_expected)
            }
        };

        let node_verdict = NodeVerdict {
            node_id: *node_id,
            node_name: node_name.to_string(),
            check_results,
            expected_outcome_verdict: expected_verdict,
        };

        let failed = has_hard_failure(&node_verdict);
        verdicts.push(node_verdict);

        if failed {
            log(format!(
                "Check failed for node '{}' — stopping check evaluation",
                node_name
            ));
            break;
        }
    }

    log(format!(
        "Check evaluation complete: {} node(s) evaluated",
        verdicts.len()
    ));

    verdicts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_check(name: &str, check_type: CheckType, on_fail: OnCheckFail) -> Check {
        Check {
            name: name.to_string(),
            check_type,
            params: serde_json::json!({}),
            on_fail,
        }
    }

    #[test]
    fn parse_all_pass() {
        let response = r#"[
            {"check_name": "Check 1", "verdict": "pass", "reasoning": "Text visible"},
            {"check_name": "Check 2", "verdict": "pass", "reasoning": "Title matches"}
        ]"#;
        let checks = vec![
            make_check("Check 1", CheckType::TextPresent, OnCheckFail::FailNode),
            make_check(
                "Check 2",
                CheckType::WindowTitleMatches,
                OnCheckFail::FailNode,
            ),
        ];
        let (results, expected) = parse_verdicts(response, &checks, &None);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.verdict == CheckVerdict::Pass));
        assert!(expected.is_none());
    }

    #[test]
    fn parse_fail_with_warn_only() {
        let response =
            r#"[{"check_name": "Soft Check", "verdict": "fail", "reasoning": "Not found"}]"#;
        let checks = vec![make_check(
            "Soft Check",
            CheckType::TextPresent,
            OnCheckFail::WarnOnly,
        )];
        let (results, _) = parse_verdicts(response, &checks, &None);
        assert_eq!(results[0].verdict, CheckVerdict::Warn);
    }

    #[test]
    fn parse_expected_outcome() {
        let response = r#"[
            {"check_name": "Check 1", "verdict": "pass", "reasoning": "ok"},
            {"check_name": "Expected outcome", "verdict": "fail", "reasoning": "Dashboard not visible"}
        ]"#;
        let checks = vec![make_check(
            "Check 1",
            CheckType::TextPresent,
            OnCheckFail::FailNode,
        )];
        let expected_outcome = Some("Dashboard should be visible".to_string());
        let (results, expected) = parse_verdicts(response, &checks, &expected_outcome);
        assert_eq!(results[0].verdict, CheckVerdict::Pass);
        assert_eq!(expected.unwrap().verdict, CheckVerdict::Fail);
    }

    #[test]
    fn parse_markdown_fenced_json() {
        let response = "```json\n[{\"check_name\": \"Check 1\", \"verdict\": \"pass\", \"reasoning\": \"ok\"}]\n```";
        let checks = vec![make_check(
            "Check 1",
            CheckType::TextPresent,
            OnCheckFail::FailNode,
        )];
        let (results, _) = parse_verdicts(response, &checks, &None);
        assert_eq!(results[0].verdict, CheckVerdict::Pass);
    }

    #[test]
    fn parse_missing_check_in_response() {
        let response = r#"[]"#;
        let checks = vec![make_check(
            "Missing",
            CheckType::TextPresent,
            OnCheckFail::FailNode,
        )];
        let (results, _) = parse_verdicts(response, &checks, &None);
        assert_eq!(results[0].verdict, CheckVerdict::Fail);
        assert!(results[0].reasoning.contains("did not return"));
    }

    #[test]
    fn parse_malformed_response() {
        let response = "this is not json at all";
        let checks = vec![make_check(
            "Check 1",
            CheckType::TextPresent,
            OnCheckFail::FailNode,
        )];
        let (results, _) = parse_verdicts(response, &checks, &None);
        assert_eq!(results[0].verdict, CheckVerdict::Fail);
    }

    #[test]
    fn hard_failure_detects_fail() {
        let verdict = NodeVerdict {
            node_id: Uuid::new_v4(),
            node_name: "test".to_string(),
            check_results: vec![CheckResult {
                check_name: "c".to_string(),
                check_type: CheckType::TextPresent,
                verdict: CheckVerdict::Fail,
                reasoning: "nope".to_string(),
            }],
            expected_outcome_verdict: None,
        };
        assert!(has_hard_failure(&verdict));
    }

    #[test]
    fn hard_failure_ignores_warn() {
        let verdict = NodeVerdict {
            node_id: Uuid::new_v4(),
            node_name: "test".to_string(),
            check_results: vec![CheckResult {
                check_name: "c".to_string(),
                check_type: CheckType::TextPresent,
                verdict: CheckVerdict::Warn,
                reasoning: "soft".to_string(),
            }],
            expected_outcome_verdict: None,
        };
        assert!(!has_hard_failure(&verdict));
    }
}
