use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::enums::{ConnectionField, RuleEditorMode, RuleField};

use super::super::{
    picker::{current_rule_field, rule_editor_suggestions, search_picker_hint},
    state::{ConnectionEditorState, RuleEditorState},
    utils::{centered_rect, shorten},
};

pub(super) fn draw_rule_editor(frame: &mut ratatui::Frame, editor: &RuleEditorState) {
    let area = centered_rect(78, 60, frame.size());
    frame.render_widget(Clear, area);

    let rows = vec![
        editor_line(
            "Source tables",
            &editor.draft.source_tables,
            editor.field == RuleField::SourceTables,
            18,
        ),
        editor_line(
            "Join conditions",
            &editor.draft.join_conditions,
            editor.field == RuleField::JoinConditions,
            18,
        ),
        editor_line(
            "Source fields",
            &editor.draft.source_fields,
            editor.field == RuleField::SourceFields,
            18,
        ),
        editor_line(
            "Transforms",
            &editor.draft.transforms,
            editor.field == RuleField::Transforms,
            18,
        ),
        editor_line(
            "Destination table",
            &editor.draft.destination_table,
            editor.field == RuleField::DestinationTable,
            18,
        ),
        editor_line(
            "Destination fields",
            &editor.draft.destination_fields,
            editor.field == RuleField::DestinationFields,
            18,
        ),
        action_line("Done", editor.field == RuleField::Done),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Join Syntax │ ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "table1.colA=table2.colB,table2.id=table3.id",
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Preview ",
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ────────────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(
                shorten(&editor.draft.expression(), 85),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(search_picker_hint(editor)),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " ▲/▼ ",
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" move  "),
            Span::styled(
                " Enter ",
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" open picker  "),
            Span::styled(
                " Enter on Done ",
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" save  "),
            Span::styled(
                " Esc ",
                Style::default()
                    .bg(Color::Red)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" close"),
        ]),
    ];

    let title = match editor.mode {
        RuleEditorMode::New => " New rule ",
        RuleEditorMode::Edit(_) => " Edit rule ",
    };

    let widget = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);

    if editor.picker_open {
        draw_rule_picker(frame, editor);
    }
}

fn draw_rule_picker(frame: &mut ratatui::Frame, editor: &RuleEditorState) {
    let area = centered_rect(56, 46, frame.size());
    frame.render_widget(Clear, area);

    let title = match editor.field {
        RuleField::SourceTables => " Select source table ",
        RuleField::SourceFields => " Select source field ",
        RuleField::Transforms => " Select transform ",
        RuleField::DestinationTable => " Select destination table ",
        RuleField::DestinationFields => " Select destination field ",
        RuleField::JoinConditions | RuleField::Done => " Select value ",
    };

    let mut rows = vec![
        Line::from(format!("Filter: {}", current_rule_field(editor))),
        Line::from(""),
    ];
    let suggestions = rule_editor_suggestions(editor);
    if suggestions.is_empty() {
        rows.push(Line::from(search_picker_hint(editor)));
    } else {
        rows.extend(
            suggestions
                .iter()
                .take(10)
                .enumerate()
                .map(|(index, suggestion)| {
                    let style = if index == editor.suggestion_index {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(
                        format!(
                            "{} {}",
                            if index == editor.suggestion_index {
                                ">"
                            } else {
                                " "
                            },
                            suggestion
                        ),
                        style,
                    ))
                }),
        );
    }

    rows.push(Line::from(""));
    rows.push(Line::from(vec![
        Span::styled(
            " A-Z ",
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" filter  "),
        Span::styled(
            " ▲/▼ ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" choose  "),
        Span::styled(
            " Enter ",
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" accept  "),
        Span::styled(
            " Esc ",
            Style::default()
                .bg(Color::Red)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" back"),
    ]));

    let widget = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

pub(super) fn draw_connection_editor(frame: &mut ratatui::Frame, editor: &ConnectionEditorState) {
    let area = centered_rect(70, 52, frame.size());
    frame.render_widget(Clear, area);

    let rows = vec![
        editor_line(
            "Kind",
            &editor.draft.kind,
            editor.field == ConnectionField::Kind,
            10,
        ),
        editor_line(
            "Address",
            &editor.draft.address,
            editor.field == ConnectionField::Address,
            10,
        ),
        editor_line(
            "Port",
            &editor.draft.port,
            editor.field == ConnectionField::Port,
            10,
        ),
        editor_line(
            "User",
            &editor.draft.user,
            editor.field == ConnectionField::User,
            10,
        ),
        editor_line(
            "Password",
            &editor.draft.password,
            editor.field == ConnectionField::Password,
            10,
        ),
        editor_line(
            "Schema/DB",
            &editor.draft.schema,
            editor.field == ConnectionField::Schema,
            10,
        ),
        Line::from(""),
        Line::from("Kind must be `mysql` or `postgres`"),
        Line::from("tab/up/down move • enter save • esc close • backspace delete"),
    ];

    let widget = Paragraph::new(rows)
        .block(
            Block::default()
                .title(format!(" {} ", editor.target.title()))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn editor_line(label: &str, value: &str, selected: bool, width: usize) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(vec![
        Span::styled(
            format!("{label:width$}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value.to_string(), style),
    ])
}

fn action_line(label: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    Line::from(Span::styled(format!("[ {label} ]"), style))
}
