//! `SKILL.md` emitter.
//!
//! Inverse of [`super::parser::parse_skill_md`]. Renders a [`Skill`]
//! into the canonical format: minimal YAML frontmatter, markdown body
//! with section + step markers in document order, and a single fenced
//! ` ```json action_sketch ` block carrying the executable plan as
//! pretty JSON.

#![allow(dead_code)]

use std::collections::HashMap;

use super::SKILL_SCHEMA_VERSION;
use super::types::{ClickweaveSkillMeta, Skill, SkillFrontmatter};

const FRONTMATTER_DELIMITER: &str = "---";

/// Render a [`Skill`] to its canonical `SKILL.md` form. Always succeeds —
/// the input is fully owned in-memory state, and the JSON for the
/// fenced block round-trips through `serde_json::to_string_pretty` so
/// no fallible serializer is on the path.
pub fn emit_skill_md(skill: &Skill) -> String {
    let mut out = String::new();
    out.push_str(FRONTMATTER_DELIMITER);
    out.push('\n');
    let frontmatter = SkillFrontmatter {
        name: skill.name.clone(),
        description: skill.description.clone(),
        id: skill.id.clone(),
        version: skill.version,
        schema_version: SKILL_SCHEMA_VERSION,
        variables: skill.variables.clone(),
        clickweave: Some(ClickweaveSkillMeta {
            state: skill.state,
            scope: skill.scope,
            tags: skill.tags.clone(),
            subgoal_text: skill.subgoal_text.clone(),
            subgoal_signature: skill.subgoal_signature.clone(),
            applicability: skill.applicability.clone(),
            parameter_schema: skill.parameter_schema.clone(),
            outputs: skill.outputs.clone(),
            outcome_predicate: skill.outcome_predicate.clone(),
            provenance: skill.provenance.clone(),
            stats: skill.stats.clone(),
            edited_by_user: skill.edited_by_user,
            created_at: skill.created_at,
            updated_at: skill.updated_at,
            produced_node_ids: skill.produced_node_ids.clone(),
        }),
    };
    let yaml = serde_yaml::to_string(&frontmatter)
        .unwrap_or_else(|err| format!("# emitter: yaml encode failed: {err}\n"));
    out.push_str(&yaml);
    out.push_str(FRONTMATTER_DELIMITER);
    out.push_str("\n\n");

    if skill.sections.is_empty() {
        // No parsed sections — fall back to the raw body so callers
        // that hand-built a skill in memory can still emit a valid
        // markdown file.
        out.push_str(skill.body.trim_end());
        out.push('\n');
    } else {
        // Build a section-id → prose-lines map by scanning the raw body.
        // This avoids relying on `body_range` byte offsets (which are now
        // UTF-16 positions for frontend use) for Rust-side string slicing.
        let section_prose = collect_section_prose(&skill.body);

        for section in &skill.sections {
            let prefix = "#".repeat(section.level as usize);
            out.push_str(&prefix);
            out.push(' ');
            out.push_str(&section.heading);
            out.push('\n');
            out.push_str(&format!("<!-- section: {} -->\n", section.id));
            for step_id in &section.step_ids {
                out.push_str(&format!("<!-- step: {step_id} -->\n"));
            }
            // Re-emit prose lines that belong to this section, skipping
            // HTML comment markers already written above. This preserves
            // human-authored instructions under each section heading.
            match section_prose.get(section.id.as_str()) {
                Some(prose) if !prose.is_empty() => {
                    for line in prose {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                _ => {
                    out.push('\n');
                }
            }
        }
    }

    let pretty =
        serde_json::to_string_pretty(&skill.action_sketch).unwrap_or_else(|_| "[]".to_string());
    out.push_str("```json action_sketch\n");
    out.push_str(&pretty);
    if !pretty.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

/// Scan the raw body text and collect prose lines for each section,
/// keyed by section ID. Lines that are heading markers (`##`/`###`),
/// HTML comment markers (`<!-- ... -->`), or the fenced action-sketch
/// block are excluded. Trailing blank lines are stripped from each
/// section's prose so the emitter can append a single blank separator
/// line itself.
///
/// Returns a map from section ID → non-empty prose lines.
fn collect_section_prose(body: &str) -> HashMap<&str, Vec<&str>> {
    let mut result: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut current_id: Option<&str> = None;
    let mut in_fence = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Skip the action_sketch fenced block.
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        // Section heading — look for the following `<!-- section: id -->`
        // marker to set `current_id`. Reset on each heading.
        if trimmed.starts_with("##") {
            current_id = None;
            continue;
        }

        // Section-ID marker: `<!-- section: <id> -->`
        if let Some(rest) = trimmed.strip_prefix("<!-- section:") {
            if let Some(id_part) = rest.strip_suffix("-->") {
                let id = id_part.trim();
                if !id.is_empty() {
                    current_id = Some(id);
                    result.entry(id).or_default();
                }
            }
            continue;
        }

        // Step marker — skip.
        if trimmed.starts_with("<!-- step:") {
            continue;
        }

        // All other HTML comments — skip.
        if trimmed.starts_with("<!--") {
            continue;
        }

        // Prose line belonging to the current section.
        if let Some(id) = current_id {
            result.entry(id).or_default().push(line);
        }
    }

    // Strip trailing blank lines from every section's prose.
    for prose in result.values_mut() {
        while prose
            .last()
            .map(|l: &&str| l.trim().is_empty())
            .unwrap_or(false)
        {
            prose.pop();
        }
    }

    result
}
