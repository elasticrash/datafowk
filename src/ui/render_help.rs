use ratatui::prelude::Stylize;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::utils::centered_rect;

pub(super) fn draw_help_modal(frame: &mut ratatui::Frame) {
    let area = centered_rect(64, 58, frame.size());
    frame.render_widget(Clear, area);

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let cyan_key = Style::default().bg(Color::Cyan).fg(Color::Black).bold();
    let yellow_key = Style::default().bg(Color::Yellow).fg(Color::Black).bold();
    let red_key = Style::default().bg(Color::Red).fg(Color::White).bold();
    let blue_key = Style::default().bg(Color::Blue).fg(Color::White).bold();
    let green_key = Style::default().bg(Color::Green).fg(Color::Black).bold();
    let magenta_key = Style::default().bg(Color::Magenta).fg(Color::White).bold();
    let gray_key = Style::default().bg(Color::DarkGray).fg(Color::White).bold();
    let prefix_style = Style::default().fg(Color::DarkGray);

    let lines = vec![
        // --- Main ---
        Line::from(Span::styled("Main", title_style)),
        Line::from(vec![
            Span::styled(" Tab ", cyan_key),
            Span::raw(" switch pane"),
        ]),
        Line::from(vec![
            Span::styled(" n ", yellow_key),
            Span::raw(" new rule       "),
            Span::styled(" c ", yellow_key),
            Span::raw(" clone rule     "),
            Span::styled(" e ", yellow_key),
            Span::raw(" edit rule      "),
            Span::styled(" d ", red_key),
            Span::raw(" delete rule"),
        ]),
        Line::from(vec![
            Span::styled(" o ", blue_key),
            Span::raw(" edit origin    "),
            Span::styled(" p ", blue_key),
            Span::raw(" edit dest      "),
            Span::styled(" v ", blue_key),
            Span::raw(" view schemas"),
        ]),
        Line::from(vec![
            Span::styled(" s ", green_key),
            Span::raw(" save           "),
            Span::styled(" t ", magenta_key),
            Span::raw(" dry-run        "),
            Span::styled(" r ", green_key),
            Span::raw(" run            "),
            Span::styled(" x ", red_key),
            Span::raw(" run+truncate"),
        ]),
        Line::from(vec![Span::styled(" q ", gray_key), Span::raw(" quit")]),
        Line::from(""),
        // --- Editors ---
        Line::from(Span::styled("Editors", title_style)),
        Line::from(vec![
            Span::styled(" Rule Editor │ ", prefix_style),
            Span::styled(" ▲/▼ ", cyan_key),
            Span::raw(" move          "),
            Span::styled(" Enter ", green_key),
            Span::raw(" open/done     "),
            Span::styled(" Esc ", red_key),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled("      Picker │ ", prefix_style),
            Span::styled(" A-Z ", magenta_key),
            Span::raw(" filter        "),
            Span::styled(" ▲/▼ ", cyan_key),
            Span::raw(" choose        "),
            Span::styled(" Enter ", green_key),
            Span::raw(" accept        "),
            Span::styled(" Esc ", red_key),
            Span::raw(" back"),
        ]),
        Line::from(""),
        // --- Schema Preview ---
        Line::from(Span::styled("Schema preview", title_style)),
        Line::from(vec![
            Span::styled("  ▲▼◀▶  ", cyan_key),
            Span::raw(" pan viewport"),
        ]),
        Line::from(vec![
            Span::styled("   1    ", yellow_key),
            Span::raw(" tables only    "),
            Span::styled("   2    ", yellow_key),
            Span::raw(" columns        "),
            Span::styled("   3    ", yellow_key),
            Span::raw(" columns+types"),
        ]),
        Line::from(vec![
            Span::styled("  + / - ", magenta_key),
            Span::raw(" cycle zoom     "),
            Span::styled("   Esc  ", red_key),
            Span::raw(" close preview"),
        ]),
        Line::from(""),
        // --- Footer ---
        Line::from("Press esc to close").fg(Color::DarkGray),
    ];

    let widget = Paragraph::new(lines)
        .block(Block::default().title(" Shortcuts ").borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}
