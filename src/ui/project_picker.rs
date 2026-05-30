use std::fs;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io;

pub(crate) fn discover_projects() -> Vec<(String, String)> {
    let mut projects = Vec::new();

    if let Ok(entries) = fs::read_dir("./projects") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let display = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let full_path = path.to_string_lossy().into_owned();
                if !display.is_empty() {
                    projects.push((display, full_path));
                }
            }
        }
    }

    projects.sort_by(|a, b| a.0.cmp(&b.0));
    projects
}

/// Show a TUI project-picker and return the selected config path, or `None` if the user quits.
pub(crate) fn run_project_picker(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<Option<String>, String> {
    let projects = discover_projects();

    if projects.is_empty() {
        return Err(String::from(
            "No projects found in ./projects/. Create a .toml config there first.",
        ));
    }

    let mut list_state = ListState::default();
    list_state.select(Some(0));

    loop {
        let projects_ref = &projects;
        terminal
            .draw(|frame| draw_project_picker(frame, projects_ref, &mut list_state))
            .map_err(|e| format!("failed to draw project picker: {e}"))?;

        if event::poll(Duration::from_millis(100))
            .map_err(|e| format!("failed to poll events: {e}"))?
        {
            if let Event::Key(key) =
                event::read().map_err(|e| format!("failed to read event: {e}"))?
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let i = list_state.selected().unwrap_or(0);
                        list_state.select(Some(i.saturating_sub(1)));
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < projects.len() {
                            list_state.select(Some(i + 1));
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(idx) = list_state.selected() {
                            return Ok(Some(projects[idx].1.clone()));
                        }
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Ok(None);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw_project_picker(
    frame: &mut ratatui::Frame,
    projects: &[(String, String)],
    list_state: &mut ListState,
) {
    let size = frame.size();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(size);

    let title = Paragraph::new(Line::from(vec![
        Span::styled("datafowk", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" — select a project"),
    ]));
    frame.render_widget(title, layout[0]);

    let items: Vec<ListItem> = projects
        .iter()
        .map(|(name, path)| {
            ListItem::new(Line::from(vec![
                Span::styled(name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {path}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title("Projects").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, layout[1], list_state);

    let hint = Paragraph::new(
        "↑/↓  navigate    Enter  open    q/Esc  quit",
    )
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(hint, layout[2]);
}
