//! Markdown + YAML frontmatter format for on-disk skill files.
//!
//! Each skill file has the shape:
//!
//! ```text
//! ---
//! id: open-vesna-chat
//! version: 1
//! …
//! ---
//!
//! # Free-form markdown body
//! ```
//!
//! `parse_skill_md` lifts the YAML frontmatter into a [`Skill`] struct
//! and stuffs the markdown body into `Skill::body`. `emit_skill_md`
//! reverses the split. Round-trip is lossless.

#![allow(dead_code)]

use super::types::{Skill, SkillError};

const FRONTMATTER_DELIMITER: &str = "---";

pub fn parse_skill_md(contents: &str) -> Result<Skill, SkillError> {
    let trimmed = contents.trim_start_matches(['\u{feff}', '\n', '\r']);
    if !trimmed.starts_with(FRONTMATTER_DELIMITER) {
        return Err(SkillError::InvalidFrontmatter(
            "expected leading `---` frontmatter delimiter".into(),
        ));
    }
    let after_open = trimmed[FRONTMATTER_DELIMITER.len()..].trim_start_matches(['\r', '\n']);
    let close_marker = format!("\n{FRONTMATTER_DELIMITER}");
    let close_idx = after_open
        .find(&close_marker)
        .ok_or_else(|| SkillError::InvalidFrontmatter("missing trailing `---` delimiter".into()))?;
    let yaml_text = &after_open[..close_idx];
    let after_close = &after_open[close_idx + close_marker.len()..];
    let body = after_close.trim_start_matches(['\r', '\n']).to_string();

    let mut skill: Skill = serde_yaml::from_str(yaml_text)?;
    skill.body = body;
    Ok(skill)
}

pub fn emit_skill_md(skill: &Skill) -> Result<String, SkillError> {
    // Split the body off so the YAML frontmatter never carries it as a
    // duplicate; emitting then re-parsing lands the body verbatim
    // through the markdown channel.
    let mut frontmatter_skill = skill.clone();
    let body = std::mem::take(&mut frontmatter_skill.body);
    let yaml = serde_yaml::to_string(&frontmatter_skill)?;
    let mut out = String::new();
    out.push_str(FRONTMATTER_DELIMITER);
    out.push('\n');
    out.push_str(&yaml);
    out.push_str(FRONTMATTER_DELIMITER);
    out.push('\n');
    out.push('\n');
    out.push_str(&body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::types::*;
    use super::*;
    use chrono::TimeZone;

    fn sample_skill() -> Skill {
        Skill {
            id: "open-vesna-chat".into(),
            version: 1,
            state: SkillState::Draft,
            scope: SkillScope::ProjectLocal,
            name: "Open Vesna's chat".into(),
            description: "Selects a contact in Telegram's sidebar.".into(),
            tags: vec!["telegram".into()],
            subgoal_text: "open chat with Vesna on Telegram".into(),
            subgoal_signature: SubgoalSignature("7f3a92c1ef5a8d04".into()),
            applicability: ApplicabilityHints {
                apps: vec!["Telegram".into()],
                hosts: vec![],
                signature: ApplicabilitySignature("a04b3f1e9d72c1c8".into()),
            },
            parameter_schema: vec![],
            action_sketch: vec![],
            outputs: vec![],
            outcome_predicate: OutcomePredicate::SubgoalCompleted {
                post_state_world_model_signature: None,
            },
            provenance: vec![],
            stats: SkillStats::default(),
            edited_by_user: false,
            created_at: chrono::Utc.timestamp_opt(0, 0).unwrap(),
            updated_at: chrono::Utc.timestamp_opt(0, 0).unwrap(),
            produced_node_ids: vec![],
            body: "# Open Vesna's chat\n\nSelects the named contact.\n".into(),
        }
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let original = sample_skill();
        let md = emit_skill_md(&original).unwrap();
        let parsed = parse_skill_md(&md).unwrap();
        assert_eq!(original.id, parsed.id);
        assert_eq!(original.version, parsed.version);
        assert_eq!(original.state, parsed.state);
        assert_eq!(original.scope, parsed.scope);
        assert_eq!(original.name, parsed.name);
        assert_eq!(original.description, parsed.description);
        assert_eq!(original.tags, parsed.tags);
        assert_eq!(original.subgoal_text, parsed.subgoal_text);
        assert_eq!(original.subgoal_signature, parsed.subgoal_signature);
        assert_eq!(original.applicability.apps, parsed.applicability.apps);
        assert_eq!(original.body.trim(), parsed.body.trim());
    }

    #[test]
    fn parse_rejects_missing_leading_delimiter() {
        let bad = "name: foo\n";
        assert!(matches!(
            parse_skill_md(bad),
            Err(SkillError::InvalidFrontmatter(_))
        ));
    }

    #[test]
    fn parse_rejects_missing_trailing_delimiter() {
        let bad = "---\nname: foo\n";
        assert!(matches!(
            parse_skill_md(bad),
            Err(SkillError::InvalidFrontmatter(_))
        ));
    }

    #[test]
    fn parse_unknown_state_variant_errors() {
        let bad = "---\nstate: invalid\n---\n\nbody";
        assert!(parse_skill_md(bad).is_err());
    }

    #[test]
    fn body_with_leading_blank_line_is_preserved() {
        let original = sample_skill();
        let md = emit_skill_md(&original).unwrap();
        let parsed = parse_skill_md(&md).unwrap();
        assert!(parsed.body.contains("# Open Vesna's chat"));
    }
}
