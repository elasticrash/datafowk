use crate::{
    config::DatabaseKind,
    models::{FieldReference, Rules, SourceJoin},
};

pub(super) fn build_select_statement(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let fields = rule
        .source_fields
        .iter()
        .map(|field| build_source_field_expression(kind, field, &rule.source_tables))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let from_clause = build_source_from_clause(kind, rule)?;

    Ok(format!("SELECT {fields} FROM {from_clause}"))
}

fn build_source_from_clause(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let Some(first_table) = rule.source_tables.first() else {
        return Err(String::from("at least one source table is required"));
    };

    let mut joined_tables = vec![first_table.clone()];
    let mut remaining_conditions = rule.join_conditions.clone();
    let mut from_clause = quote_identifier(kind, first_table)?;

    for table in rule.source_tables.iter().skip(1) {
        let mut join_conditions = Vec::new();
        let mut next_remaining = Vec::new();

        for condition in remaining_conditions {
            if join_condition_reaches_joined_table(&condition, table, &joined_tables) {
                join_conditions.push(condition);
            } else {
                next_remaining.push(condition);
            }
        }

        if join_conditions.is_empty() {
            return Err(format!(
                "source table `{table}` is not connected to the existing join path"
            ));
        }

        from_clause.push_str(&format!(" JOIN {} ON ", quote_identifier(kind, table)?));
        from_clause.push_str(
            &join_conditions
                .iter()
                .map(|condition| join_condition_to_sql(kind, condition))
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND "),
        );

        joined_tables.push(table.clone());
        remaining_conditions = next_remaining;
    }

    if !remaining_conditions.is_empty() {
        from_clause.push_str(" WHERE ");
        from_clause.push_str(
            &remaining_conditions
                .iter()
                .map(|condition| join_condition_to_sql(kind, condition))
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND "),
        );
    }

    Ok(from_clause)
}

fn join_condition_reaches_joined_table(
    condition: &SourceJoin,
    current_table: &str,
    joined_tables: &[String],
) -> bool {
    (condition.left_table == current_table
        && joined_tables
            .iter()
            .any(|table| table == &condition.right_table))
        || (condition.right_table == current_table
            && joined_tables
                .iter()
                .any(|table| table == &condition.left_table))
}

fn join_condition_to_sql(kind: DatabaseKind, condition: &SourceJoin) -> Result<String, String> {
    Ok(format!(
        "{} = {}",
        qualify_identifier(kind, &condition.left_table, &condition.left_field)?,
        qualify_identifier(kind, &condition.right_table, &condition.right_field)?
    ))
}

fn build_source_field_expression(
    kind: DatabaseKind,
    field: &str,
    source_tables: &[String],
) -> Result<String, String> {
    let reference = parse_source_field_reference(field, source_tables)?;
    match reference.table {
        Some(table) => qualify_identifier(kind, &table, &reference.field),
        None => quote_identifier(kind, &reference.field),
    }
}

fn parse_source_field_reference(
    field: &str,
    source_tables: &[String],
) -> Result<FieldReference, String> {
    if let Some((table, column)) = field.split_once('.') {
        let table = table.trim().to_string();
        let column = column.trim().to_string();

        if !source_tables
            .iter()
            .any(|source_table| source_table == &table)
        {
            return Err(format!(
                "source field `{field}` references unknown source table `{table}`"
            ));
        }

        if column.is_empty() {
            return Err(format!("source field `{field}` is missing a column name"));
        }

        Ok(FieldReference {
            table: Some(table),
            field: column,
        })
    } else if source_tables.len() == 1 {
        Ok(FieldReference {
            table: None,
            field: field.trim().to_string(),
        })
    } else {
        Err(format!(
            "source field `{field}` must use `table.column` when multiple source tables are configured"
        ))
    }
}

pub(super) fn build_insert_statement(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let columns = rule
        .destination_fields
        .iter()
        .map(|field| quote_identifier(kind, field))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let placeholders = match kind {
        DatabaseKind::Mysql => vec!["?".to_string(); rule.destination_fields.len()],
        DatabaseKind::Postgres => (1..=rule.destination_fields.len())
            .map(|index| format!("${index}"))
            .collect(),
    }
    .join(", ");

    let table_name = quote_identifier(kind, &rule.destination_table)?;

    Ok(format!(
        "INSERT INTO {table_name} ({columns}) VALUES ({placeholders})"
    ))
}

pub(super) fn quote_identifier(kind: DatabaseKind, identifier: &str) -> Result<String, String> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!(
            "unsupported identifier `{identifier}`; only letters, numbers, and underscores are allowed"
        ));
    }

    Ok(match kind {
        DatabaseKind::Mysql => format!("`{identifier}`"),
        DatabaseKind::Postgres => format!("\"{identifier}\""),
    })
}

pub(super) fn qualify_identifier(
    kind: DatabaseKind,
    table: &str,
    field: &str,
) -> Result<String, String> {
    Ok(format!(
        "{}.{}",
        quote_identifier(kind, table)?,
        quote_identifier(kind, field)?
    ))
}

pub(super) fn ensure_matches_database(
    rule_database: &str,
    configured_schema: &str,
    alias: &str,
) -> Result<(), String> {
    if rule_database == alias || rule_database == configured_schema {
        Ok(())
    } else {
        Err(format!(
            "rule database `{rule_database}` does not match the configured {alias} schema `{configured_schema}`"
        ))
    }
}
