use crate::output_schema::{ConditionValue, OutputRef};
use crate::{Condition, LiteralValue, Operator, Position};

pub fn pos(x: f32, y: f32) -> Position {
    Position { x, y }
}

/// Dummy condition referencing `click_1.result`.
///
/// The `click_1` auto_id exists in most test workflows (it's the first Click
/// node added). The `result` field does not exist on Click's output schema,
/// so field-level validation will emit a warning, but it won't cause a hard
/// error. Tests that need a fully valid condition should construct their own.
pub fn dummy_condition() -> Condition {
    Condition {
        left: OutputRef {
            node: "click_1".to_string(),
            field: "result".to_string(),
        },
        operator: Operator::Equals,
        right: ConditionValue::Literal {
            value: LiteralValue::Bool { value: true },
        },
    }
}
