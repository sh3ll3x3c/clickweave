use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The type of data an output field produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum OutputFieldType {
    Bool,
    Number,
    String,
    Array,
    Object,
    Any,
}

/// A declared output field on a node type (compile-time schema metadata).
#[derive(Debug, Clone)]
pub struct OutputField {
    pub name: &'static str,
    pub field_type: OutputFieldType,
    pub description: &'static str,
}

/// Owned version of OutputField for TypeScript bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OutputFieldInfo {
    pub name: String,
    pub field_type: OutputFieldType,
    pub description: String,
}

impl From<&OutputField> for OutputFieldInfo {
    fn from(f: &OutputField) -> Self {
        Self {
            name: f.name.to_string(),
            field_type: f.field_type,
            description: f.description.to_string(),
        }
    }
}

/// Method used to verify an action node's effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum VerificationMethod {
    Vlm,
    Dom,
    AccessibilityTree,
}

/// Verification configuration carried by action-node params structs.
///
/// Flattens onto its owner via `#[serde(flatten)]` as the pair of sibling
/// fields `verification_method` / `verification_assertion`, which is the
/// same on-disk layout the core types used before the substruct was
/// extracted. Both fields are optional in the stored representation — a
/// verification is only considered "configured" when both halves are
/// present. That check is centralized in [`VerificationConfig::resolved`]
/// and exposed through the [`HasVerification`] trait.
///
/// Using `Option` on the inner fields (rather than wrapping the whole
/// substruct in `Option<_>`) keeps `specta::Flatten` satisfied — specta
/// does not implement `Flatten` for `Option<T>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VerificationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_method: Option<VerificationMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_assertion: Option<String>,
}

impl VerificationConfig {
    /// Create a fully-configured verification from a method + assertion pair.
    pub fn new(method: VerificationMethod, assertion: impl Into<String>) -> Self {
        Self {
            verification_method: Some(method),
            verification_assertion: Some(assertion.into()),
        }
    }

    /// True when neither half is set.
    pub fn is_empty(&self) -> bool {
        self.verification_method.is_none() && self.verification_assertion.is_none()
    }

    /// Resolve to a concrete `(method, assertion)` pair when both halves are
    /// present. Partial configs (one half missing) resolve to `None`.
    pub fn resolved(&self) -> Option<ResolvedVerification<'_>> {
        match (self.verification_method, &self.verification_assertion) {
            (Some(method), Some(assertion)) => Some(ResolvedVerification { method, assertion }),
            _ => None,
        }
    }
}

/// Borrowed view of a fully-configured verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedVerification<'a> {
    pub method: VerificationMethod,
    pub assertion: &'a str,
}

/// Uniform accessor for the [`VerificationConfig`] carried by every
/// action-node params struct. Returns `None` for node types that do not
/// support verification.
pub trait HasVerification {
    fn verification(&self) -> Option<&VerificationConfig>;

    /// Convenience: the resolved verification (both halves present) if any.
    fn resolved_verification(&self) -> Option<ResolvedVerification<'_>> {
        self.verification().and_then(|v| v.resolved())
    }
}

/// What kind of data a node produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum OutputRole {
    Query,
    Action,
    Ai,
    Generic,
}

/// The execution context a node operates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum NodeContext {
    Native,
    Cdp,
    Independent,
}

// --- Output schema registry ---

use crate::NodeType;

// Short aliases for OutputFieldType variants used in schema constants.
use OutputFieldType as T;

const FIND_TEXT_OUTPUTS: &[OutputField] = &[
    OutputField {
        name: "found",
        field_type: T::Bool,
        description: "Whether any matches were found",
    },
    OutputField {
        name: "count",
        field_type: T::Number,
        description: "Number of matches found",
    },
    OutputField {
        name: "text",
        field_type: T::String,
        description: "Text of the first match",
    },
    OutputField {
        name: "coordinates",
        field_type: T::Object,
        description: "Coordinates of the first match",
    },
];

const FIND_IMAGE_OUTPUTS: &[OutputField] = &[
    OutputField {
        name: "found",
        field_type: T::Bool,
        description: "Whether any matches were found",
    },
    OutputField {
        name: "count",
        field_type: T::Number,
        description: "Number of matches found",
    },
    OutputField {
        name: "coordinates",
        field_type: T::Object,
        description: "Coordinates of the first match",
    },
    OutputField {
        name: "confidence",
        field_type: T::Number,
        description: "Confidence score of the first match",
    },
];

const FIND_APP_OUTPUTS: &[OutputField] = &[
    OutputField {
        name: "found",
        field_type: T::Bool,
        description: "Whether the app is running",
    },
    OutputField {
        name: "name",
        field_type: T::String,
        description: "App name",
    },
    OutputField {
        name: "pid",
        field_type: T::Number,
        description: "Process ID",
    },
];

const TAKE_SCREENSHOT_OUTPUTS: &[OutputField] = &[OutputField {
    name: "result",
    field_type: T::String,
    description: "Screenshot data",
}];

const CDP_WAIT_OUTPUTS: &[OutputField] = &[OutputField {
    name: "found",
    field_type: T::Bool,
    description: "Whether the text appeared before timeout",
}];

const AI_STEP_OUTPUTS: &[OutputField] = &[OutputField {
    name: "result",
    field_type: T::String,
    description: "LLM response text",
}];

const GENERIC_OUTPUTS: &[OutputField] = &[OutputField {
    name: "result",
    field_type: T::Any,
    description: "Raw tool result",
}];

const EMPTY_OUTPUTS: &[OutputField] = &[];

const VERIFICATION_OUTPUTS: &[OutputField] = &[
    OutputField {
        name: "verified",
        field_type: T::Bool,
        description: "Whether the action had the intended effect",
    },
    OutputField {
        name: "verification_reasoning",
        field_type: T::String,
        description: "Explanation of the verification result",
    },
];

impl NodeType {
    /// Returns the static output schema (without verification fields).
    pub fn output_schema(&self) -> &'static [OutputField] {
        match self {
            Self::FindText(_) => FIND_TEXT_OUTPUTS,
            Self::FindImage(_) => FIND_IMAGE_OUTPUTS,
            Self::FindApp(_) => FIND_APP_OUTPUTS,
            Self::TakeScreenshot(_) => TAKE_SCREENSHOT_OUTPUTS,
            Self::CdpWait(_) => CDP_WAIT_OUTPUTS,
            Self::AiStep(_) => AI_STEP_OUTPUTS,
            Self::McpToolCall(_) | Self::AppDebugKitOp(_) => GENERIC_OUTPUTS,
            _ => EMPTY_OUTPUTS,
        }
    }
}

/// Full output schema including verification fields when enabled.
pub fn full_output_schema(
    node_type: &NodeType,
    has_verification: bool,
) -> Cow<'static, [OutputField]> {
    let base = node_type.output_schema();
    if has_verification && node_type.output_role() == OutputRole::Action {
        let mut fields: Vec<OutputField> = base.to_vec();
        fields.extend_from_slice(VERIFICATION_OUTPUTS);
        Cow::Owned(fields)
    } else {
        Cow::Borrowed(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn output_field_type_serde_roundtrip() {
        for t in [
            OutputFieldType::Bool,
            OutputFieldType::Number,
            OutputFieldType::String,
            OutputFieldType::Array,
            OutputFieldType::Object,
            OutputFieldType::Any,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            let back: OutputFieldType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, back);
        }
    }

    #[test]
    fn query_nodes_have_outputs() {
        assert!(
            !NodeType::FindText(FindTextParams::default())
                .output_schema()
                .is_empty()
        );
        assert!(
            !NodeType::FindImage(FindImageParams::default())
                .output_schema()
                .is_empty()
        );
        assert!(
            !NodeType::FindApp(FindAppParams::default())
                .output_schema()
                .is_empty()
        );
        assert!(
            !NodeType::CdpWait(CdpWaitParams::default())
                .output_schema()
                .is_empty()
        );
    }

    #[test]
    fn action_nodes_have_empty_base_outputs() {
        assert!(
            NodeType::Click(ClickParams::default())
                .output_schema()
                .is_empty()
        );
        assert!(
            NodeType::CdpClick(CdpClickParams::default())
                .output_schema()
                .is_empty()
        );
    }

    #[test]
    fn full_output_schema_adds_verification() {
        let click = NodeType::Click(ClickParams::default());
        let without = full_output_schema(&click, false);
        let with = full_output_schema(&click, true);
        assert!(without.is_empty());
        assert_eq!(with.len(), 2);
        assert_eq!(with[0].name, "verified");
    }

    #[test]
    fn find_text_has_four_outputs() {
        let ft = NodeType::FindText(FindTextParams::default());
        assert_eq!(ft.output_schema().len(), 4);
        assert_eq!(ft.output_schema()[0].name, "found");
        assert_eq!(ft.output_schema()[3].name, "coordinates");
    }
}
