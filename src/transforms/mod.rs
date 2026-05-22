use crate::{
    config::DatabaseKind,
    models::{DataValue, RuleTransform, Rules},
};

pub fn apply_transform(
    value: &mut DataValue,
    transform: &RuleTransform,
    source_kind: DatabaseKind,
) -> Result<(), String> {
    match transform.name.as_str() {
        "copy" | "identity" => Ok(()),
        "unique" => Ok(()),
        "trim" => {
            expect_no_arguments(transform)?;
            transform_string_value(value, source_kind, |text| text.trim().to_string())
        }
        "lowercase" => {
            expect_no_arguments(transform)?;
            transform_string_value(value, source_kind, |text| text.to_lowercase())
        }
        "uppercase" => {
            expect_no_arguments(transform)?;
            transform_string_value(value, source_kind, |text| text.to_uppercase())
        }
        "sum" | "add" => {
            let operand = parse_numeric_argument(transform)?;
            transform_numeric_value(value, source_kind, operand, NumericOperation::Add)
        }
        "multiply" | "mul" => {
            let operand = parse_numeric_argument(transform)?;
            transform_numeric_value(value, source_kind, operand, NumericOperation::Multiply)
        }
        unknown => Err(format!("unsupported transformation function `{unknown}`")),
    }
}

fn expect_no_arguments(transform: &RuleTransform) -> Result<(), String> {
    if transform.arguments.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "transformation `{}` does not accept arguments",
            transform.expression()
        ))
    }
}

fn parse_numeric_argument(transform: &RuleTransform) -> Result<f64, String> {
    if transform.arguments.len() != 1 {
        return Err(format!(
            "transformation `{}` expects exactly one numeric argument",
            transform.expression()
        ));
    }

    transform.arguments[0].parse::<f64>().map_err(|error| {
        format!(
            "transformation `{}` has invalid numeric argument `{}`: {error}",
            transform.name, transform.arguments[0]
        )
    })
}

enum NumericOperation {
    Add,
    Multiply,
}

fn transform_numeric_value(
    value: &mut DataValue,
    source_kind: DatabaseKind,
    operand: f64,
    operation: NumericOperation,
) -> Result<(), String> {
    let _ = source_kind;

    match value {
        DataValue::I64(current) => {
            let result = apply_numeric_operation(*current as f64, operand, &operation);
            *value = cast_numeric_result(*current, result)?;
        }
        DataValue::U64(current) => {
            let result = apply_numeric_operation(*current as f64, operand, &operation);
            *value = cast_numeric_result(*current, result)?;
        }
        DataValue::F64(current) => {
            *current = apply_numeric_operation(*current, operand, &operation);
        }
        DataValue::String(text) => {
            let parsed = parse_numeric_text(text)?;
            let result = apply_numeric_operation(parsed, operand, &operation);
            *value = cast_text_numeric_result(text, result)?;
        }
        DataValue::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|error| format!("numeric transformation requires UTF-8 data: {error}"))?;
            let parsed = parse_numeric_text(text)?;
            let result = apply_numeric_operation(parsed, operand, &operation);
            *value = cast_text_numeric_result(text, result)?;
        }
        DataValue::Null => {}
        DataValue::Bool(_) | DataValue::Date(_) | DataValue::Time(_) | DataValue::DateTime(_) => {
            return Err(String::from(
                "numeric transformations only support integer and floating-point values",
            ))
        }
    }

    Ok(())
}

pub fn is_row_transform(transform: &RuleTransform) -> bool {
    transform.name == "unique"
}

pub fn unique_destination_field_indexes(rule: &Rules) -> Result<Option<Vec<usize>>, String> {
    let mut unique_transforms = rule
        .function_chain
        .iter()
        .filter(|transform| transform.name == "unique");

    let Some(transform) = unique_transforms.next() else {
        return Ok(None);
    };

    if unique_transforms.next().is_some() {
        return Err(format!(
            "rule for destination table `{}` defines more than one `unique(...)` transform",
            rule.destination_table
        ));
    }

    if transform.arguments.is_empty() {
        return Err(format!(
            "`unique(...)` for destination table `{}` must list at least one destination field",
            rule.destination_table
        ));
    }

    let mut indexes = Vec::new();

    for field in &transform.arguments {
        let Some(index) = rule
            .destination_fields
            .iter()
            .position(|destination_field| destination_field == field)
        else {
            return Err(format!(
                "`unique({})` references unknown destination field `{}` for table `{}`",
                transform.arguments.join(","),
                field,
                rule.destination_table
            ));
        };

        if indexes.contains(&index) {
            return Err(format!(
                "`unique({})` repeats destination field `{}` for table `{}`",
                transform.arguments.join(","),
                field,
                rule.destination_table
            ));
        }

        indexes.push(index);
    }

    Ok(Some(indexes))
}

fn apply_numeric_operation(value: f64, operand: f64, operation: &NumericOperation) -> f64 {
    match operation {
        NumericOperation::Add => value + operand,
        NumericOperation::Multiply => value * operand,
    }
}

fn cast_numeric_result<T>(original: T, result: f64) -> Result<DataValue, String>
where
    T: IntoNumericValue,
{
    original.into_data_value(result)
}

fn cast_text_numeric_result(original: &str, result: f64) -> Result<DataValue, String> {
    if original.contains('.') {
        Ok(DataValue::F64(result))
    } else if is_integral(result) && result >= i64::MIN as f64 && result <= i64::MAX as f64 {
        Ok(DataValue::I64(result as i64))
    } else {
        Ok(DataValue::F64(result))
    }
}

trait IntoNumericValue {
    fn into_data_value(self, result: f64) -> Result<DataValue, String>;
}

impl IntoNumericValue for i64 {
    fn into_data_value(self, result: f64) -> Result<DataValue, String> {
        if is_integral(result) && result >= i64::MIN as f64 && result <= i64::MAX as f64 {
            Ok(DataValue::I64(result as i64))
        } else {
            Ok(DataValue::F64(result))
        }
    }
}

impl IntoNumericValue for u64 {
    fn into_data_value(self, result: f64) -> Result<DataValue, String> {
        if is_integral(result) && result >= 0.0 && result <= u64::MAX as f64 {
            Ok(DataValue::U64(result as u64))
        } else {
            Ok(DataValue::F64(result))
        }
    }
}

fn is_integral(value: f64) -> bool {
    (value.fract()).abs() < f64::EPSILON
}

fn parse_numeric_text(value: &str) -> Result<f64, String> {
    value.trim().parse::<f64>().map_err(|error| {
        format!(
            "numeric transformation expected a number but received `{}`: {error}",
            value
        )
    })
}

fn transform_string_value<F>(
    value: &mut DataValue,
    source_kind: DatabaseKind,
    transformer: F,
) -> Result<(), String>
where
    F: FnOnce(&str) -> String,
{
    match value {
        DataValue::String(text) => {
            *text = transformer(text);
        }
        DataValue::Bytes(bytes) => {
            let _ = source_kind;
            let text = std::str::from_utf8(bytes)
                .map_err(|error| format!("string transformation requires UTF-8 data: {error}"))?;
            *value = DataValue::String(transformer(text));
        }
        DataValue::Null
        | DataValue::I64(_)
        | DataValue::U64(_)
        | DataValue::F64(_)
        | DataValue::Bool(_)
        | DataValue::Date(_)
        | DataValue::Time(_)
        | DataValue::DateTime(_) => {
            let _ = source_kind;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_transform_updates_integer_values() {
        let mut value = DataValue::I64(4);
        let transform = RuleTransform {
            name: String::from("sum"),
            arguments: vec![String::from("3")],
        };

        apply_transform(&mut value, &transform, DatabaseKind::Mysql).unwrap();

        assert_eq!(value, DataValue::I64(7));
    }

    #[test]
    fn multiply_transform_updates_float_values() {
        let mut value = DataValue::F64(2.5);
        let transform = RuleTransform {
            name: String::from("multiply"),
            arguments: vec![String::from("1.2")],
        };

        apply_transform(&mut value, &transform, DatabaseKind::Mysql).unwrap();

        assert_eq!(value, DataValue::F64(3.0));
    }

    #[test]
    fn numeric_transform_rejects_text_values() {
        let mut value = DataValue::String(String::from("abc"));
        let transform = RuleTransform {
            name: String::from("sum"),
            arguments: vec![String::from("2")],
        };

        assert!(apply_transform(&mut value, &transform, DatabaseKind::Mysql).is_err());
    }

    #[test]
    fn numeric_transform_accepts_numeric_strings() {
        let mut value = DataValue::String(String::from("4"));
        let transform = RuleTransform {
            name: String::from("multiply"),
            arguments: vec![String::from("5")],
        };

        apply_transform(&mut value, &transform, DatabaseKind::Mysql).unwrap();

        assert_eq!(value, DataValue::I64(20));
    }
}
