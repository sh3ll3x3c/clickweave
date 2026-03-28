use std::collections::HashMap;

use crate::output_schema::{self, ConditionValue, OutputFieldType, OutputRef};
use crate::{Node, NodeType, Workflow};

use super::ValidationError;

/// Non-fatal warning about a variable/field reference issue.
#[derive(Debug, Clone)]
pub struct VariableWarning {
    pub node_name: String,
    pub message: String,
}

/// Build a lookup from `auto_id` -> `&Node` for nodes that produce runtime
/// variables. Control-flow nodes are excluded because they don't produce
/// variables accessible via OutputRef.
fn build_producer_map(workflow: &Workflow) -> HashMap<&str, &Node> {
    workflow
        .nodes
        .iter()
        .filter(|n| {
            !matches!(
                n.node_type,
                NodeType::If(_) | NodeType::Switch(_) | NodeType::Loop(_) | NodeType::EndLoop(_)
            )
        })
        .map(|n| (n.auto_id.as_str(), n))
        .collect()
}

/// Validate that variable references in conditions and ref params point to
/// actual nodes, reference valid output fields, and have compatible types.
///
/// Node existence checks are hard errors (the workflow is broken).
/// Field existence and type compatibility are warnings (the ref will resolve
/// to null at runtime but the workflow is structurally valid).
pub(crate) fn validate_condition_variables(
    workflow: &Workflow,
) -> Result<Vec<VariableWarning>, ValidationError> {
    let producers = build_producer_map(workflow);
    let mut warnings = Vec::new();

    for node in &workflow.nodes {
        // --- Validate condition refs on control-flow nodes ---
        let conditions: Vec<&crate::Condition> = match &node.node_type {
            NodeType::Loop(p) => vec![&p.exit_condition],
            NodeType::If(p) => vec![&p.condition],
            NodeType::Switch(p) => p.cases.iter().map(|c| &c.condition).collect(),
            _ => vec![],
        };

        for condition in &conditions {
            validate_output_ref(&condition.left, &node.name, &producers)?;
            check_ref(
                &condition.left,
                &node.name,
                None,
                None,
                &producers,
                &mut warnings,
            );

            if let ConditionValue::Ref(ref output_ref) = condition.right {
                validate_output_ref(output_ref, &node.name, &producers)?;
                check_ref(
                    output_ref,
                    &node.name,
                    None,
                    None,
                    &producers,
                    &mut warnings,
                );
            }
        }

        // --- Validate ref params on action/query nodes ---
        let ref_params = node.node_type.ref_params();
        for (input_name, output_ref) in &ref_params {
            validate_output_ref(output_ref, &node.name, &producers)?;
            check_ref(
                output_ref,
                &node.name,
                Some(input_name),
                Some(&node.node_type),
                &producers,
                &mut warnings,
            );
        }
    }

    Ok(warnings)
}

/// Check that the referenced node auto_id exists among variable producers.
fn validate_output_ref(
    output_ref: &OutputRef,
    node_name: &str,
    producers: &HashMap<&str, &Node>,
) -> Result<(), ValidationError> {
    if output_ref.node.is_empty() {
        return Err(ValidationError::EmptyVariableReference(
            node_name.to_string(),
        ));
    }
    if !producers.contains_key(output_ref.node.as_str()) {
        let variable = format!("{}.{}", output_ref.node, output_ref.field);
        return Err(ValidationError::InvalidVariableReference {
            node_name: node_name.to_string(),
            variable,
            prefix: output_ref.node.clone(),
        });
    }
    Ok(())
}

/// Check that a referenced field exists and (if `input_name` + `consumer_type`
/// are given) that the field's type is accepted by the consuming input.
/// Computes the source output schema once for both checks.
fn check_ref(
    output_ref: &OutputRef,
    node_name: &str,
    input_name: Option<&str>,
    consumer_type: Option<&NodeType>,
    producers: &HashMap<&str, &Node>,
    warnings: &mut Vec<VariableWarning>,
) {
    let Some(source_node) = producers.get(output_ref.node.as_str()) else {
        return;
    };

    let has_verification = source_node.node_type.has_verification();
    let schema = output_schema::full_output_schema(&source_node.node_type, has_verification);

    let Some(output_field) = schema.iter().find(|f| f.name == output_ref.field) else {
        let available: Vec<&str> = schema.iter().map(|f| f.name).collect();
        let available_str = if available.is_empty() {
            "none".to_string()
        } else {
            available.join(", ")
        };
        warnings.push(VariableWarning {
            node_name: node_name.to_string(),
            message: format!(
                "references field '{}' on '{}', but that node's outputs are: {}",
                output_ref.field, output_ref.node, available_str,
            ),
        });
        return;
    };

    // Type compatibility check (only when input context is provided)
    let (Some(input_name), Some(consumer_type)) = (input_name, consumer_type) else {
        return;
    };

    if output_field.field_type == OutputFieldType::Any {
        return;
    }

    let input_schema = consumer_type.input_schema();
    let Some(input_field) = input_schema.iter().find(|f| f.name == input_name) else {
        return;
    };

    if input_field
        .accepted_types
        .contains(&output_field.field_type)
    {
        return;
    }

    let accepted: Vec<String> = input_field
        .accepted_types
        .iter()
        .map(|t| format!("{:?}", t))
        .collect();
    warnings.push(VariableWarning {
        node_name: node_name.to_string(),
        message: format!(
            "input '{}' expects {} but '{}.{}' produces {:?}",
            input_name,
            accepted.join(", "),
            output_ref.node,
            output_ref.field,
            output_field.field_type,
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::pos;
    use crate::output_schema::{ConditionValue, OutputRef};
    use crate::{
        ClickParams, Condition, EdgeOutput, EndLoopParams, FindTextParams, IfParams, LiteralValue,
        LoopParams, NodeType, Operator, SwitchCase, SwitchParams, Workflow,
    };

    use super::super::ValidationError;
    use super::super::ValidationWarning;
    use super::super::validate_workflow;

    #[test]
    fn test_validate_loop_valid_variable_reference() {
        // Loop exit condition references "find_text_1.found" -- a FindText node exists with that auto_id
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: Condition {
                    left: OutputRef {
                        node: "find_text_1".to_string(),
                        field: "found".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
                max_iterations: 10,
            }),
            pos(100.0, 0.0),
        );
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: loop_node }),
            pos(200.0, 0.0),
        );
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge(find, loop_node);
        wf.add_edge_with_output(loop_node, end_loop, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);
        wf.add_edge(end_loop, loop_node);

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_loop_invalid_variable_reference() {
        // Loop exit condition references "find_textt.found" -- typo, no matching node
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: Condition {
                    left: OutputRef {
                        node: "find_textt".to_string(),
                        field: "found".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
                max_iterations: 10,
            }),
            pos(100.0, 0.0),
        );
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: loop_node }),
            pos(200.0, 0.0),
        );
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge(find, loop_node);
        wf.add_edge_with_output(loop_node, end_loop, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);
        wf.add_edge(end_loop, loop_node);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_validate_if_invalid_variable_reference() {
        // If condition references a variable with no matching node
        let mut wf = Workflow::default();
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "nonexistent_node".to_string(),
                        field: "result".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_validate_literal_right_condition_passes() {
        // Conditions with a valid left ref and a literal right should pass
        let mut wf = Workflow::default();
        let click = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 100.0));
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "click_1".to_string(),
                        field: "result".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Number { value: 1.0 },
                    },
                },
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge(click, if_node);
        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_node_ref_without_match_is_invalid() {
        // An OutputRef node that doesn't match any node should fail
        let mut wf = Workflow::default();
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "no_such_node".to_string(),
                        field: "result".to_string(),
                    },
                    operator: Operator::IsNotEmpty,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_validate_switch_invalid_variable_reference() {
        // Switch case condition references a variable with no matching node
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let switch_node = wf.add_node(
            NodeType::Switch(SwitchParams {
                cases: vec![SwitchCase {
                    name: "found".to_string(),
                    condition: Condition {
                        left: OutputRef {
                            node: "typo_node".to_string(),
                            field: "found".to_string(),
                        },
                        operator: Operator::Equals,
                        right: ConditionValue::Literal {
                            value: LiteralValue::Bool { value: true },
                        },
                    },
                }],
            }),
            pos(100.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));

        wf.add_edge(find, switch_node);
        wf.add_edge_with_output(
            switch_node,
            a,
            EdgeOutput::SwitchCase {
                name: "found".to_string(),
            },
        );

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_validate_control_flow_node_name_not_valid_prefix() {
        // A Loop condition that references its own Loop auto_id should fail --
        // control-flow nodes don't produce runtime variables.
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: Condition {
                    left: OutputRef {
                        node: "loop_1".to_string(),
                        field: "success".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
                max_iterations: 10,
            }),
            pos(100.0, 0.0),
        );
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: loop_node }),
            pos(200.0, 0.0),
        );
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge(find, loop_node);
        wf.add_edge_with_output(loop_node, end_loop, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);
        wf.add_edge(end_loop, loop_node);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_validate_empty_variable_reference_rejected() {
        // Empty variable name should be rejected, not silently skipped
        let mut wf = Workflow::default();
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: String::new(),
                        field: String::new(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::EmptyVariableReference(_)));
    }

    #[test]
    fn test_valid_field_reference_no_warning() {
        // find_text_1.found is a valid field on FindText -- no warning
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "find_text_1".to_string(),
                        field: "found".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(100.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 100.0));

        wf.add_edge(find, if_node);
        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert!(
            result.warnings.is_empty(),
            "expected no warnings but got: {:?}",
            result
                .warnings
                .iter()
                .map(|w| w.message())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_invalid_field_reference_produces_warning() {
        // find_text_1.nonexistent_field is not a valid field -- produces warning
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "find_text_1".to_string(),
                        field: "nonexistent_field".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(100.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 100.0));

        wf.add_edge(find, if_node);
        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let result = validate_workflow(&wf).expect("should pass (warnings, not errors)");
        let var_warnings: Vec<_> = result
            .warnings
            .iter()
            .filter(|w| matches!(w, ValidationWarning::Variable(_)))
            .collect();
        assert_eq!(var_warnings.len(), 1);
        assert!(var_warnings[0].message().contains("nonexistent_field"));
        assert!(var_warnings[0].message().contains("find_text_1"));
    }

    #[test]
    fn test_ref_param_valid_field_and_type() {
        // Click with target_ref pointing to find_text_1.coordinates (Object) -- valid
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let click = wf.add_node(
            NodeType::Click(ClickParams {
                target_ref: Some(OutputRef {
                    node: "find_text_1".to_string(),
                    field: "coordinates".to_string(),
                }),
                ..Default::default()
            }),
            pos(100.0, 0.0),
        );
        wf.add_edge(find, click);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_ref_param_type_mismatch_produces_warning() {
        // Click with target_ref pointing to find_text_1.found (Bool) -- target_ref expects Object
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let click = wf.add_node(
            NodeType::Click(ClickParams {
                target_ref: Some(OutputRef {
                    node: "find_text_1".to_string(),
                    field: "found".to_string(),
                }),
                ..Default::default()
            }),
            pos(100.0, 0.0),
        );
        wf.add_edge(find, click);

        let result = validate_workflow(&wf).expect("should pass (warnings, not errors)");
        let var_warnings: Vec<_> = result
            .warnings
            .iter()
            .filter(|w| matches!(w, ValidationWarning::Variable(_)))
            .collect();
        assert_eq!(var_warnings.len(), 1);
        assert!(var_warnings[0].message().contains("target_ref"));
        assert!(var_warnings[0].message().contains("Bool"));
    }

    #[test]
    fn test_ref_param_invalid_field_produces_warning() {
        // Click with target_ref pointing to find_text_1.bogus -- field doesn't exist
        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        let click = wf.add_node(
            NodeType::Click(ClickParams {
                target_ref: Some(OutputRef {
                    node: "find_text_1".to_string(),
                    field: "bogus".to_string(),
                }),
                ..Default::default()
            }),
            pos(100.0, 0.0),
        );
        wf.add_edge(find, click);

        let result = validate_workflow(&wf).expect("should pass (warnings, not errors)");
        let var_warnings: Vec<_> = result
            .warnings
            .iter()
            .filter(|w| matches!(w, ValidationWarning::Variable(_)))
            .collect();
        assert!(
            !var_warnings.is_empty(),
            "expected a warning about invalid field"
        );
        assert!(var_warnings[0].message().contains("bogus"));
    }

    #[test]
    fn test_ref_param_dangling_node_is_error() {
        // Click with target_ref pointing to nonexistent_node -- hard error
        let mut wf = Workflow::default();
        wf.add_node(
            NodeType::Click(ClickParams {
                target_ref: Some(OutputRef {
                    node: "nonexistent_node".to_string(),
                    field: "coordinates".to_string(),
                }),
                ..Default::default()
            }),
            pos(0.0, 0.0),
        );

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidVariableReference { .. }
        ));
    }

    #[test]
    fn test_verification_fields_accessible_when_enabled() {
        // Click with verification enabled: find_text_1.verified should be valid
        use crate::output_schema::VerificationMethod;

        let mut wf = Workflow::default();
        let find = wf.add_node(NodeType::FindText(FindTextParams::default()), pos(0.0, 0.0));
        // Give find_text_1 verification -- but FindText is Query role, so
        // verification outputs aren't added. Use a Click node with verification instead.
        let click = wf.add_node(
            NodeType::Click(ClickParams {
                verification_method: Some(VerificationMethod::Vlm),
                verification_assertion: Some("button clicked".to_string()),
                ..Default::default()
            }),
            pos(100.0, 0.0),
        );
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: Condition {
                    left: OutputRef {
                        node: "click_1".to_string(),
                        field: "verified".to_string(),
                    },
                    operator: Operator::Equals,
                    right: ConditionValue::Literal {
                        value: LiteralValue::Bool { value: true },
                    },
                },
            }),
            pos(200.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(300.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(300.0, 100.0));

        wf.add_edge(find, click);
        wf.add_edge(click, if_node);
        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert!(
            result.warnings.is_empty(),
            "expected no warnings but got: {:?}",
            result
                .warnings
                .iter()
                .map(|w| w.message())
                .collect::<Vec<_>>(),
        );
    }
}
