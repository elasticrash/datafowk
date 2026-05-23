use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    config::{ConnectionProperties, DatabaseKind},
    etl_rule_parser::parser::split_csv_values,
    models::{Rules, SourceJoin},
};

pub(super) fn pane_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub(super) fn rule_title(expression: &str) -> Result<String, String> {
    let rule = crate::etl_rule_parser::parser::parse_rule(expression)?;
    Ok(format!(
        "{} -> {}",
        format!("{}.{}", rule.source_db, rule.source_tables.join("+")),
        format!("{}.{}", rule.destination_db, rule.destination_table)
    ))
}

pub(super) fn rule_diagram_lines(rule: &Rules) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for table in &rule.source_tables {
        lines.push(Line::from(Span::styled(
            format!("origin.{table}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
    }

    if !rule.join_conditions.is_empty() {
        lines.push(Line::from(format!(
            "  join on {}",
            joins_to_string(&rule.join_conditions)
        )));
    }

    lines.push(Line::from(format!(
        "  read [{}]",
        rule.source_fields.join(", ")
    )));
    lines.push(Line::from("      |"));
    lines.push(Line::from(format!(
        "      +-- {}",
        rule.function_chain
            .iter()
            .map(|transform| transform.expression())
            .collect::<Vec<_>>()
            .join(" -> ")
    )));
    lines.push(Line::from("      v"));
    lines.push(Line::from(Span::styled(
        format!("destination.{}", rule.destination_table),
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "  write [{}]",
        rule.destination_fields.join(", ")
    )));

    lines
}

pub(super) fn connection_summary(connection: &ConnectionProperties) -> String {
    format!(
        "{} {}@{}:{}/{}",
        database_kind_label(connection.kind),
        connection.user,
        connection.address,
        connection.port,
        connection.schema
    )
}

pub(crate) fn shorten(value: &str, max_len: usize) -> String {
    let count = value.chars().count();
    if count <= max_len {
        return value.to_string();
    }

    if max_len <= 1 {
        return "…".to_string();
    }

    let truncated = value.chars().take(max_len - 1).collect::<String>();
    format!("{truncated}…")
}

pub(super) fn build_rule_expression(
    source_tables: &str,
    join_conditions: &str,
    source_fields: &str,
    transforms: &str,
    destination_table: &str,
    destination_fields: &str,
) -> String {
    let transforms = if transforms.trim().is_empty() {
        "copy"
    } else {
        transforms.trim()
    };

    let join_section = if join_conditions.trim().is_empty() {
        String::new()
    } else {
        format!("{{{}}}", normalize_csv(join_conditions))
    };

    format!(
        "(origin:{}){}[{}]<{}>(destination:{})[{}]",
        normalize_csv(source_tables),
        join_section,
        normalize_csv(source_fields),
        normalize_csv(transforms),
        destination_table.trim(),
        normalize_csv(destination_fields)
    )
}

fn normalize_csv(value: &str) -> String {
    split_csv_values(value).unwrap_or_default().join(",")
}

pub(super) fn joins_to_string(joins: &[SourceJoin]) -> String {
    joins
        .iter()
        .map(|join| {
            format!(
                "{}.{}={}.{}",
                join.left_table, join.left_field, join.right_table, join.right_field
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn parse_database_kind(value: &str) -> Result<DatabaseKind, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mysql" => Ok(DatabaseKind::Mysql),
        "postgres" | "postgresql" => Ok(DatabaseKind::Postgres),
        other => Err(format!("unsupported database kind `{other}`")),
    }
}

pub(super) fn database_kind_label(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::Mysql => "mysql",
        DatabaseKind::Postgres => "postgres",
    }
}
