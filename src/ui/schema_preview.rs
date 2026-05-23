use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    config::{Config, ConnectionProperties},
    enums::{SchemaPanelState, SchemaSide, SchemaZoom},
    etl::preview_schema,
    models::TableSchema,
};

use super::utils::centered_rect;

pub(crate) struct SchemaPreviewState {
    origin: SchemaPanelState,
    destination: SchemaPanelState,
    updates: Receiver<SchemaPreviewMessage>,
    scroll_x: u16,
    scroll_y: u16,
    zoom: SchemaZoom,
}

impl SchemaPreviewState {
    pub(super) fn apply_pending_updates(&mut self) {
        while let Ok(message) = self.updates.try_recv() {
            let panel = match message.side {
                SchemaSide::Origin => &mut self.origin,
                SchemaSide::Destination => &mut self.destination,
            };
            *panel = SchemaPanelState::Loaded(message.result);
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('v') => true,
            KeyCode::Up => {
                self.scroll_y = self.scroll_y.saturating_sub(1);
                false
            }
            KeyCode::Down => {
                self.scroll_y = self.scroll_y.saturating_add(1);
                false
            }
            KeyCode::Left => {
                self.scroll_x = self.scroll_x.saturating_sub(4);
                false
            }
            KeyCode::Right => {
                self.scroll_x = self.scroll_x.saturating_add(4);
                false
            }
            KeyCode::Char('1') => {
                self.zoom = SchemaZoom::Tables;
                false
            }
            KeyCode::Char('2') => {
                self.zoom = SchemaZoom::Columns;
                false
            }
            KeyCode::Char('3') => {
                self.zoom = SchemaZoom::Types;
                false
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.zoom = self.zoom.next();
                false
            }
            KeyCode::Char('-') => {
                self.zoom = self.zoom.previous();
                false
            }
            _ => false,
        }
    }
}

pub(super) struct SchemaPreviewMessage {
    pub(super) side: SchemaSide,
    pub(super) result: Result<Vec<TableSchema>, String>,
}

pub(super) fn draw_schema_preview(frame: &mut ratatui::Frame, schema: &SchemaPreviewState) {
    let area = centered_rect(88, 82, frame.size());
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(10)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let title = Paragraph::new(format!(
        "Schema preview • arrows pan • 1/2/3 zoom • +/- cycle • current {} • esc closes",
        schema.zoom.label()
    ))
    .block(
        Block::default()
            .title(" Database schemas ")
            .borders(Borders::ALL),
    )
    .alignment(Alignment::Center);
    frame.render_widget(title, chunks[0]);

    draw_schema_panel(frame, columns[0], "Origin schema", &schema.origin, schema);
    draw_schema_panel(
        frame,
        columns[1],
        "Destination schema",
        &schema.destination,
        schema,
    );
}

pub(super) fn open_schema_preview(config: &Config) -> SchemaPreviewState {
    let (sender, receiver) = mpsc::channel();

    spawn_schema_preview_worker(
        sender.clone(),
        SchemaSide::Origin,
        config.connection_properties_origin.clone(),
        "origin",
    );
    spawn_schema_preview_worker(
        sender,
        SchemaSide::Destination,
        config.connection_properties_destination.clone(),
        "destination",
    );

    SchemaPreviewState {
        origin: SchemaPanelState::Connecting,
        destination: SchemaPanelState::Connecting,
        updates: receiver,
        scroll_x: 0,
        scroll_y: 0,
        zoom: SchemaZoom::Columns,
    }
}

pub(super) fn spawn_schema_preview_worker(
    sender: mpsc::Sender<SchemaPreviewMessage>,
    side: SchemaSide,
    connection: ConnectionProperties,
    label: &'static str,
) {
    thread::spawn(move || {
        let result = preview_schema(&connection, label);
        let _ = sender.send(SchemaPreviewMessage { side, result });
    });
}

fn draw_schema_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    schema: &SchemaPanelState,
    preview: &SchemaPreviewState,
) {
    let lines = match schema {
        SchemaPanelState::Connecting => vec![Line::from(Span::styled(
            "Connecting...",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))],
        SchemaPanelState::Loaded(Ok(tables)) if tables.is_empty() => {
            vec![Line::from("No tables found")]
        }
        SchemaPanelState::Loaded(Ok(tables)) => schema_graph_lines(tables, preview.zoom),
        SchemaPanelState::Loaded(Err(error)) => vec![Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        ))],
    };

    let widget = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .scroll((preview.scroll_y, preview.scroll_x))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn schema_graph_lines(tables: &[TableSchema], zoom: SchemaZoom) -> Vec<Line<'static>> {
    schema_graph_rows(tables, zoom)
        .into_iter()
        .map(Line::from)
        .collect()
}

fn schema_graph_rows(tables: &[TableSchema], zoom: SchemaZoom) -> Vec<String> {
    let mut lines = Vec::new();

    for table in tables {
        let mut rows = vec![table.name.clone()];

        match zoom {
            SchemaZoom::Tables => {}
            SchemaZoom::Columns => {
                rows.extend(table.columns.iter().map(|column| column.name.clone()));
            }
            SchemaZoom::Types => {
                rows.extend(
                    table
                        .columns
                        .iter()
                        .map(|column| format!("{}: {}", column.name, column.data_type)),
                );
            }
        }

        let content_width = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        let horizontal = "─".repeat(content_width + 2);
        lines.push(format!("┌{horizontal}┐"));

        for (index, row) in rows.iter().enumerate() {
            lines.push(format!("│ {:width$} │", row, width = content_width));
            if index == 0 && rows.len() > 1 {
                lines.push(format!("├{horizontal}┤"));
            }
        }

        lines.push(format!("└{horizontal}┘"));
        lines.push(String::new());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TableColumnSchema;

    #[test]
    fn schema_graph_rows_support_zoom_levels() {
        let tables = vec![TableSchema {
            name: String::from("users"),
            columns: vec![
                TableColumnSchema {
                    name: String::from("id"),
                    data_type: String::from("integer"),
                },
                TableColumnSchema {
                    name: String::from("email"),
                    data_type: String::from("text"),
                },
            ],
        }];

        let zoom_one = schema_graph_rows(&tables, SchemaZoom::Tables).join("\n");
        let zoom_three = schema_graph_rows(&tables, SchemaZoom::Types).join("\n");

        assert!(zoom_one.contains("users"));
        assert!(!zoom_one.contains("email"));
        assert!(zoom_three.contains("email: text"));
    }
}
