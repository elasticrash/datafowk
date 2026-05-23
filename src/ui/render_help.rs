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

    let lines = vec![
        Line::from(Span::styled(
            "Main",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                " Tab ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" switch pane  "),
        ]),
        Line::from(vec![
            Span::styled(
                " n ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" new rule     "),
            Span::styled(
                " c ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" clone rule   "),
            Span::styled(
                " e ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" edit rule    "),
            Span::styled(
                " d ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" delete rule  "),
        ]),
        Line::from(vec![
            Span::styled(
                " o ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" edit origin  "),
            Span::styled(
                " p ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" edit dest    "),
            Span::styled(
                " v ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" view schemas "),
        ]),
        Line::from(vec![
            Span::styled(
                " s ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" save         "),
            Span::styled(
                " t ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" dry-run      "),
            Span::styled(
                " r ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" run          "),
            Span::styled(
                " x ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" run+truncate "),
            Span::styled(
                " q ",
                Style::default().bg(Color::DarkGray).fg(Color::White).bold(),
            ),
            Span::raw(" quit         "),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Editors",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(" Rule Editor │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " ▲/▼ ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" move          "),
            Span::styled(
                " Enter ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" open/done     "),
            Span::styled(
                " Esc ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled("      Picker │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " A-Z ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" filter        "),
            Span::styled(
                " ▲/▼ ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" choose        "),
            Span::styled(
                " Enter ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" accept       "),
            Span::styled(
                " Esc ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" back"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Schema preview",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "  ▲▼◀▶  ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" pan viewport"),
        ]),
        Line::from(vec![
            Span::styled(
                "   1    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" tables only     "),
            Span::styled(
                "   2    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" columns         "),
            Span::styled(
                "   3    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" columns + types"),
        ]),
        Line::from(vec![
            Span::styled(
                "  + / - ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" cycle zoom      "),
            Span::styled(
                "   Esc  ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" close preview"),
        ]),
        Line::from(""),
        Line::from("Press esc to close"),
    ];

    let widget = Paragraph::new(lines)
        .block(Block::default().title(" Shortcuts ").borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}
