//! Runtime context for workflow execution.
//!
//! Holds variables produced by node outputs and loop iteration counters.
//! Variables are global to the execution — a variable set inside a loop
//! is visible after the loop ends (no nested scoping).

use crate::{Condition, LiteralValue, Operator, ValueRef};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Runtime state maintained during workflow execution.
#[derive(Debug, Default)]
pub struct RuntimeContext {
    /// Variables produced by node outputs.
    /// Key format: "<sanitized_node_name>.<field>" (e.g., "find_text_1.found").
    pub variables: HashMap<String, Value>,

    /// Loop iteration counters. Key: Loop node UUID, Value: current iteration (0-indexed).
    pub loop_counters: HashMap<Uuid, u32>,
}

impl RuntimeContext {
    /// Create a new, empty runtime context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a variable.
    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) {
        self.variables.insert(name.into(), value);
    }

    /// Look up a variable by name.
    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// Resolve a [`ValueRef`] to a concrete [`Value`].
    ///
    /// - `Variable { name }` → stored value, or `Value::Null` if missing.
    /// - `Literal { value }` → converted to the corresponding JSON value.
    pub fn resolve_value_ref(&self, value_ref: &ValueRef) -> Value {
        match value_ref {
            ValueRef::Variable { name } => self.variables.get(name).cloned().unwrap_or(Value::Null),
            ValueRef::Literal { value } => match value {
                LiteralValue::String { value } => Value::String(value.clone()),
                LiteralValue::Number { value } => serde_json::Number::from_f64(*value)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                LiteralValue::Bool { value } => Value::Bool(*value),
            },
        }
    }

    /// Evaluate a [`Condition`] against the current runtime state.
    pub fn evaluate_condition(&self, condition: &Condition) -> bool {
        let left = self.resolve_value_ref(&condition.left);
        let right = self.resolve_value_ref(&condition.right);
        evaluate_operator(&condition.operator, &left, &right)
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Apply an operator to two resolved JSON values.
fn evaluate_operator(op: &Operator, left: &Value, right: &Value) -> bool {
    match op {
        Operator::Equals => values_equal(left, right),
        Operator::NotEquals => !values_equal(left, right),
        Operator::GreaterThan => compare_numbers(left, right, |l, r| l > r),
        Operator::LessThan => compare_numbers(left, right, |l, r| l < r),
        Operator::GreaterThanOrEqual => compare_numbers(left, right, |l, r| l >= r),
        Operator::LessThanOrEqual => compare_numbers(left, right, |l, r| l <= r),
        Operator::Contains => string_contains(left, right),
        Operator::NotContains => !string_contains(left, right),
        Operator::IsEmpty => is_empty(left),
        Operator::IsNotEmpty => !is_empty(left),
    }
}

/// A value is considered "empty" when it carries no meaningful content.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        // Booleans and numbers are never empty — they always carry a value.
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Equality with light type coercion.
///
/// Coercion rules:
/// - `bool` == `"true"` / `"false"` (case-sensitive)
/// - `number` == `number` using f64 epsilon comparison
/// - `string` == `string` exact match
/// - `null` == `null`
/// - Mismatched types that don't match a coercion rule → `false`
fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        // null == null
        (Value::Null, Value::Null) => true,

        // string == string
        (Value::String(l), Value::String(r)) => l == r,

        // number == number (epsilon)
        (Value::Number(_), Value::Number(_)) => match (value_as_f64(left), value_as_f64(right)) {
            (Some(l), Some(r)) => (l - r).abs() < f64::EPSILON,
            _ => false,
        },

        // bool == bool
        (Value::Bool(l), Value::Bool(r)) => l == r,

        // bool <-> string coercion
        (Value::Bool(b), Value::String(s)) | (Value::String(s), Value::Bool(b)) => {
            if *b {
                s == "true"
            } else {
                s == "false"
            }
        }

        // Everything else (arrays, objects, mismatched primitives) → not equal.
        _ => false,
    }
}

/// Numeric comparison with extraction from Number or parseable String.
fn compare_numbers(left: &Value, right: &Value, cmp: impl Fn(f64, f64) -> bool) -> bool {
    match (value_as_f64(left), value_as_f64(right)) {
        (Some(l), Some(r)) => cmp(l, r),
        _ => false,
    }
}

/// Try to extract an f64 from a JSON value.
///
/// Works for `Value::Number` and `Value::String` that can be parsed as f64.
fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Check whether `haystack` (as a string) contains `needle` (as a string).
///
/// Non-string values are converted via `Value::to_string()`.
fn string_contains(haystack: &Value, needle: &Value) -> bool {
    let h = match haystack {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let n = match needle {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    h.contains(&n)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Variable value-ref.
    fn var(name: &str) -> ValueRef {
        ValueRef::Variable {
            name: name.to_string(),
        }
    }

    /// Helper: build a Literal bool value-ref.
    fn lit_bool(v: bool) -> ValueRef {
        ValueRef::Literal {
            value: LiteralValue::Bool { value: v },
        }
    }

    /// Helper: build a Literal string value-ref.
    fn lit_str(s: &str) -> ValueRef {
        ValueRef::Literal {
            value: LiteralValue::String {
                value: s.to_string(),
            },
        }
    }

    /// Helper: build a Literal number value-ref.
    fn lit_num(n: f64) -> ValueRef {
        ValueRef::Literal {
            value: LiteralValue::Number { value: n },
        }
    }

    #[test]
    fn equals_bool_true() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("found", Value::Bool(true));

        let cond = Condition {
            left: var("found"),
            operator: Operator::Equals,
            right: lit_bool(true),
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn equals_bool_false() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("found", Value::Bool(false));

        let cond = Condition {
            left: var("found"),
            operator: Operator::Equals,
            right: lit_bool(true),
        };

        assert!(!ctx.evaluate_condition(&cond));
    }

    #[test]
    fn not_equals() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("status", Value::String("error".into()));

        let cond = Condition {
            left: var("status"),
            operator: Operator::NotEquals,
            right: lit_str("ok"),
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn greater_than() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable(
            "score",
            Value::Number(serde_json::Number::from_f64(0.95).unwrap()),
        );

        let cond = Condition {
            left: var("score"),
            operator: Operator::GreaterThan,
            right: lit_num(0.8),
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn contains_string() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("text", Value::String("Login successful".into()));

        let cond = Condition {
            left: var("text"),
            operator: Operator::Contains,
            right: lit_str("successful"),
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn is_empty_null() {
        let ctx = RuntimeContext::new();

        // "var" is not set, so it resolves to Null.
        let cond = Condition {
            left: var("var"),
            operator: Operator::IsEmpty,
            right: lit_bool(true), // right side is ignored for IsEmpty
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn is_not_empty_with_value() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("result", Value::String("data".into()));

        let cond = Condition {
            left: var("result"),
            operator: Operator::IsNotEmpty,
            right: lit_bool(true), // right side is ignored for IsNotEmpty
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn missing_variable_equals_null() {
        let ctx = RuntimeContext::new();

        // Missing variable resolves to Null; Null != "" (different types, no coercion rule).
        let cond = Condition {
            left: var("var"),
            operator: Operator::Equals,
            right: lit_str(""),
        };

        assert!(!ctx.evaluate_condition(&cond));
    }

    #[test]
    fn bool_string_coercion() {
        let mut ctx = RuntimeContext::new();
        ctx.set_variable("flag", Value::Bool(true));

        let cond = Condition {
            left: var("flag"),
            operator: Operator::Equals,
            right: lit_str("true"),
        };

        assert!(ctx.evaluate_condition(&cond));
    }

    #[test]
    fn loop_counter_tracking() {
        let mut ctx = RuntimeContext::new();
        let loop_id = Uuid::new_v4();

        // Initially no counter.
        assert_eq!(ctx.loop_counters.get(&loop_id), None);

        // Set to 0.
        ctx.loop_counters.insert(loop_id, 0);
        assert_eq!(ctx.loop_counters[&loop_id], 0);

        // Increment.
        *ctx.loop_counters.get_mut(&loop_id).unwrap() += 1;
        assert_eq!(ctx.loop_counters[&loop_id], 1);

        // Increment again.
        *ctx.loop_counters.get_mut(&loop_id).unwrap() += 1;
        assert_eq!(ctx.loop_counters[&loop_id], 2);
    }
}
