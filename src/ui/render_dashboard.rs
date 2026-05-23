use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::enums::Pane;

use super::super::{
    state::AppState,
    utils::{
        connection_summary, joins_to_string, pane_style, rule_diagram_lines, rule_title, shorten,
    },
};

pub(super) fn draw_rules_list(frame: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let items = if state.config.rules.is_empty() {
        vec![ListItem::new("No rules yet. Press n to create one.")]
    } else {
        state
            .config
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let title =
                    rule_title(&rule.expression).unwrap_or_else(|_| format!("Rule {}", index + 1));
                let subtitle = shorten(&rule.expression, 72);
                ListItem::new(vec![
                    Line::from(Span::styled(title, Style::default().fg(Color::White))),
                    Line::from(Span::styled(subtitle, Style::default().fg(Color::DarkGray))),
                ])
            })
            .collect::<Vec<_>>()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    " Rules ",
                    pane_style(state.active_pane == Pane::Rules),
                ))
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut state.rules_state);
}

pub(super) fn draw_rule_preview(frame: &mut ratatui::Frame, state: &AppState, area: Rect) {
    let lines = match state.selected_rule_preview() {
        Ok(rule) => rule_diagram_lines(&rule),
        Err(error) => vec![Line::from(Span::styled(
            error,
            Style::default().fg(Color::Yellow),
        ))],
    };

    let preview = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Rule preview ")
                .borders(Borders::ALL),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

    frame.render_widget(preview, area);
}

pub(super) fn draw_connections(
    frame: &mut ratatui::Frame,
    state: &AppState,
    area: Rect,
    config_path: &str,
) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Config: ", Style::default().fg(Color::DarkGray)),
            Span::raw(config_path),
        ]),
        Line::from(vec![
            Span::styled("Origin: ", Style::default().fg(Color::DarkGray)),
            Span::raw(connection_summary(
                &state.config.connection_properties_origin,
            )),
        ]),
        Line::from(vec![
            Span::styled("Destination: ", Style::default().fg(Color::DarkGray)),
            Span::raw(connection_summary(
                &state.config.connection_properties_destination,
            )),
        ]),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Connections ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

pub(super) fn draw_rule_details(frame: &mut ratatui::Frame, state: &AppState, area: Rect) {
    let lines = match state.selected_rule_preview() {
        Ok(rule) => {
            vec![
                Line::from(vec![
                    Span::styled("Source tables: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.source_tables.join(", ")),
                ]),
                Line::from(vec![
                    Span::styled("Join conditions: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(if rule.join_conditions.is_empty() {
                        String::from("(none)")
                    } else {
                        joins_to_string(&rule.join_conditions)
                    }),
                ]),
                Line::from(vec![
                    Span::styled("Source fields: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.source_fields.join(", ")),
                ]),
                Line::from(vec![
                    Span::styled("Transforms: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(
                        rule.function_chain
                            .iter()
                            .map(|transform| transform.expression())
                            .collect::<Vec<_>>()
                            .join(" -> "),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Destination table: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_table),
                ]),
                Line::from(vec![
                    Span::styled("Destination fields: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_fields.join(", ")),
                ]),
            ]
        }
        Err(error) => vec![Line::from(Span::styled(
            error,
            Style::default().fg(Color::Yellow),
        ))],
    };

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    " Rule details ",
                    pane_style(state.active_pane == Pane::Details),
                ))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

pub(super) fn draw_status(frame: &mut ratatui::Frame, status: &str, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(14)])
        .split(area);

    let status_widget = Paragraph::new(status)
        .block(Block::default().borders(Borders::TOP))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true });
    frame.render_widget(status_widget, sections[0]);

    let help_widget = Paragraph::new(Span::styled(
        "? shortcuts",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
    .block(Block::default().borders(Borders::TOP))
    .alignment(Alignment::Right);
    frame.render_widget(help_widget, sections[1]);
}
