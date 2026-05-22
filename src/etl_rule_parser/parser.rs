use crate::models::{RuleTransform, Rules, SourceJoin};

pub fn split_csv_values(values: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;

    for character in values.chars() {
        match character {
            ',' if paren_depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
                current.clear();
            }
            '(' => {
                paren_depth += 1;
                current.push(character);
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(format!("unexpected `)` in `{values}`"));
                }
                paren_depth -= 1;
                current.push(character);
            }
            _ => current.push(character),
        }
    }

    if paren_depth != 0 {
        return Err(format!("missing closing `)` in `{values}`"));
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }

    Ok(parts)
}

fn take_enclosed<'a>(
    input: &'a str,
    open: char,
    close: char,
) -> Result<(&'a str, &'a str), String> {
    let input = input.trim_start();
    if !input.starts_with(open) {
        return Err(format!("expected `{open}` at `{input}`"));
    }

    let end = input[1..]
        .find(close)
        .ok_or_else(|| format!("missing closing `{close}` in `{input}`"))?;

    Ok((&input[1..1 + end], &input[2 + end..]))
}

fn parse_table_field_reference(value: &str) -> Result<(String, String), String> {
    let (table, field) = value
        .trim()
        .split_once('.')
        .ok_or_else(|| format!("expected `table.field` reference in `{value}`"))?;

    if table.trim().is_empty() || field.trim().is_empty() {
        return Err(format!("invalid `table.field` reference `{value}`"));
    }

    Ok((table.trim().to_string(), field.trim().to_string()))
}

fn parse_join_condition(value: &str) -> Result<SourceJoin, String> {
    let (left, right) = value
        .split_once('=')
        .ok_or_else(|| format!("join condition `{value}` must contain `=`"))?;

    let (left_table, left_field) = parse_table_field_reference(left)?;
    let (right_table, right_field) = parse_table_field_reference(right)?;

    Ok(SourceJoin {
        left_table,
        left_field,
        right_table,
        right_field,
    })
}

fn parse_transform(value: &str) -> Result<RuleTransform, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(String::from("transform names cannot be empty"));
    }

    if let Some(open_index) = value.find('(') {
        if !value.ends_with(')') {
            return Err(format!("transform `{value}` is missing a closing `)`"));
        }

        let name = value[..open_index].trim();
        if name.is_empty() {
            return Err(format!("transform `{value}` is missing a name"));
        }

        let arguments = split_csv_values(&value[open_index + 1..value.len() - 1])?;
        Ok(RuleTransform {
            name: name.to_ascii_lowercase(),
            arguments,
        })
    } else {
        Ok(RuleTransform {
            name: value.to_ascii_lowercase(),
            arguments: Vec::new(),
        })
    }
}

fn validate_source_field_references(rule: &Rules, input: &str) -> Result<(), String> {
    for field in &rule.source_fields {
        if let Some((table, column)) = field.split_once('.') {
            if table.trim().is_empty() || column.trim().is_empty() {
                return Err(format!("rule `{input}` has invalid source field `{field}`"));
            }
            if !rule
                .source_tables
                .iter()
                .any(|source_table| source_table == table.trim())
            {
                return Err(format!(
                    "rule `{input}` references unknown source table `{}` in field `{field}`",
                    table.trim()
                ));
            }
        } else if rule.source_tables.len() > 1 {
            return Err(format!(
                "rule `{input}` must qualify source field `{field}` with `table.field` when multiple source tables are used"
            ));
        }
    }

    Ok(())
}

fn validate_transforms(rule: &Rules, input: &str) -> Result<(), String> {
    let mut unique_count = 0usize;

    for transform in &rule.function_chain {
        if transform.name == "unique" {
            unique_count += 1;

            if transform.arguments.is_empty() {
                return Err(format!(
                    "rule `{input}` must configure `unique(...)` with at least one destination field"
                ));
            }

            for field in &transform.arguments {
                if !rule
                    .destination_fields
                    .iter()
                    .any(|destination_field| destination_field == field)
                {
                    return Err(format!(
                        "rule `{input}` references unknown destination field `{field}` in `unique(...)`"
                    ));
                }
            }
        }
    }

    if unique_count > 1 {
        return Err(format!(
            "rule `{input}` defines more than one `unique(...)` transform"
        ));
    }

    Ok(())
}

pub fn parse_rule(input: &str) -> Result<Rules, String> {
    let input = input.trim();

    let (source_spec, rest) = take_enclosed(input, '(', ')')?;
    let (source_db, source_tables_raw) = source_spec
        .split_once(':')
        .ok_or_else(|| format!("source spec `{source_spec}` must be `db:table`"))?;

    let (joins_raw, rest) = if rest.trim_start().starts_with('{') {
        let (joins, rest) = take_enclosed(rest, '{', '}')?;
        (Some(joins), rest)
    } else {
        (None, rest)
    };

    let (source_fields_raw, rest) = take_enclosed(rest, '[', ']')?;
    let (function_chain_raw, rest) = take_enclosed(rest, '<', '>')?;
    let (destination_spec, rest) = take_enclosed(rest, '(', ')')?;
    let (destination_db, destination_table) = destination_spec
        .split_once(':')
        .ok_or_else(|| format!("destination spec `{destination_spec}` must be `db:table`"))?;
    let (destination_fields_raw, rest) = take_enclosed(rest, '[', ']')?;

    if !rest.trim().is_empty() {
        return Err(format!(
            "rule `{input}` has unexpected trailing content `{}`",
            rest.trim()
        ));
    }

    let source_tables = split_csv_values(source_tables_raw)?;
    let join_conditions = joins_raw
        .map(split_csv_values)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|join| parse_join_condition(&join))
        .collect::<Result<Vec<_>, _>>()?;

    let function_chain = split_csv_values(function_chain_raw)?
        .into_iter()
        .map(|transform| parse_transform(&transform))
        .collect::<Result<Vec<_>, _>>()?;

    let rule = Rules {
        source_db: source_db.trim().to_string(),
        source_tables,
        join_conditions,
        source_fields: split_csv_values(source_fields_raw)?,
        function_chain,
        destination_db: destination_db.trim().to_string(),
        destination_table: destination_table.trim().to_string(),
        destination_fields: split_csv_values(destination_fields_raw)?,
    };

    if rule.source_tables.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one source table"
        ));
    }

    if rule.source_fields.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one source field"
        ));
    }

    if rule.destination_fields.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one destination field"
        ));
    }

    if rule.function_chain.is_empty() {
        return Err(format!(
            "rule `{input}` must contain at least one transformation"
        ));
    }

    if !rule.join_conditions.is_empty() && rule.source_tables.len() == 1 {
        return Err(format!(
            "rule `{input}` defines joins but only contains one source table"
        ));
    }

    if rule.source_tables.len() > 1 && rule.join_conditions.is_empty() {
        return Err(format!(
            "rule `{input}` must define join conditions for multiple source tables"
        ));
    }

    if rule.source_tables.len() > 1 && rule.join_conditions.len() + 1 < rule.source_tables.len() {
        return Err(format!(
            "rule `{input}` needs at least {} join conditions for {} source tables",
            rule.source_tables.len() - 1,
            rule.source_tables.len()
        ));
    }

    for join in &rule.join_conditions {
        for table in [&join.left_table, &join.right_table] {
            if !rule
                .source_tables
                .iter()
                .any(|source_table| source_table == table)
            {
                return Err(format!(
                    "rule `{input}` references unknown source table `{table}` in join condition"
                ));
            }
        }
    }

    validate_source_field_references(&rule, input)?;
    validate_transforms(&rule, input)?;

    Ok(rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_single_source_rule() {
        let input = "(db1:table1)[field1,field2]<fn,fn2,fn3>(db2:table2)[field3,field4]";
        let result = parse_rule(input).unwrap();

        assert_eq!(result.source_db, "db1");
        assert_eq!(result.source_tables, vec!["table1"]);
        assert_eq!(result.source_fields, vec!["field1", "field2"]);
        assert_eq!(
            result
                .function_chain
                .iter()
                .map(RuleTransform::expression)
                .collect::<Vec<_>>(),
            vec!["fn", "fn2", "fn3"]
        );
        assert_eq!(result.destination_db, "db2");
        assert_eq!(result.destination_table, "table2");
        assert_eq!(result.destination_fields, vec!["field3", "field4"]);
    }

    #[test]
    fn test_parser_trims_values() {
        let input = "(origin:users)[ firstname , lastname ]< trim , uppercase >(destination:spot)[ name , surname ]";
        let result = parse_rule(input).unwrap();

        assert_eq!(result.source_fields, vec!["firstname", "lastname"]);
        assert_eq!(
            result
                .function_chain
                .iter()
                .map(RuleTransform::expression)
                .collect::<Vec<_>>(),
            vec!["trim", "uppercase"]
        );
        assert_eq!(result.destination_fields, vec!["name", "surname"]);
    }

    #[test]
    fn test_parser_supports_transform_arguments() {
        let input = "(origin:users)[age]<trim,sum(2),multiply(1.5)>(destination:spot)[score]";
        let result = parse_rule(input).unwrap();

        assert_eq!(
            result
                .function_chain
                .iter()
                .map(RuleTransform::expression)
                .collect::<Vec<_>>(),
            vec!["trim", "sum(2)", "multiply(1.5)"]
        );
    }

    #[test]
    fn test_parser_multi_source_rule() {
        let input = "(origin:users,address){users.address_id=address.id}[users.firstname,address.address,address.number]<trim>(destination:spot)[name,address,number]";
        let result = parse_rule(input).unwrap();

        assert_eq!(result.source_tables, vec!["users", "address"]);
        assert_eq!(result.join_conditions.len(), 1);
        assert_eq!(result.source_fields[0], "users.firstname");
        assert_eq!(result.destination_fields, vec!["name", "address", "number"]);
    }

    #[test]
    fn test_multi_source_requires_join() {
        let input = "(origin:users,address)[users.firstname,address.address]<trim>(destination:spot)[name,address]";
        assert!(parse_rule(input).is_err());
    }

    #[test]
    fn test_unique_transform_requires_known_destination_fields() {
        let input = "(origin:users)[firstname]<unique(name)>(destination:spot)[surname]";
        assert!(parse_rule(input).is_err());
    }
}
