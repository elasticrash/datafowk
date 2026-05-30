use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::{
    config::Config,
    etl::fetch_data_preview,
    etl_rule_parser::parser::parse_rule,
    models::DataValue,
    transforms::geometry::{compute_area, compute_perimeter},
};

use super::utils::centered_rect;

// ── Data types ────────────────────────────────────────────────────────────────

pub(crate) struct DataPreviewData {
    pub(super) columns: Vec<String>,
    pub(super) rows: Vec<Vec<String>>,
}

pub(super) enum DataPreviewLoad {
    Loading,
    Done(Result<DataPreviewData, String>),
}

pub(crate) struct DataPreviewState {
    pub(super) load: DataPreviewLoad,
    pub(super) updates: Receiver<Result<DataPreviewData, String>>,
    pub(super) scroll: usize,
}

impl DataPreviewState {
    pub(super) fn apply_pending_updates(&mut self) {
        if let Ok(result) = self.updates.try_recv() {
            self.load = DataPreviewLoad::Done(result);
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc | KeyCode::Char('g') => true,
            KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                false
            }
            KeyCode::Down => {
                if let DataPreviewLoad::Done(Ok(data)) = &self.load {
                    self.scroll =
                        (self.scroll + 1).min(data.rows.len().saturating_sub(1));
                }
                false
            }
            _ => false,
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub(super) fn open_data_preview(config: &Config, rule_expression: &str) -> DataPreviewState {
    let (sender, receiver) = mpsc::channel();
    let connection = config.connection_properties_origin.clone();
    let expression = rule_expression.to_string();

    thread::spawn(move || {
        let result = run_data_preview_worker(&connection, &expression);
        let _ = sender.send(result);
    });

    DataPreviewState {
        load: DataPreviewLoad::Loading,
        updates: receiver,
        scroll: 0,
    }
}

fn data_value_to_display(value: &DataValue) -> String {
    match value {
        DataValue::Null => String::from("null"),
        DataValue::String(s) => s.clone(),
        DataValue::I64(n) => n.to_string(),
        DataValue::U64(n) => n.to_string(),
        DataValue::F64(n) => format!("{n:.4}"),
        DataValue::Bool(b) => b.to_string(),
        DataValue::Bytes(bytes) => format!("<bytes {}>", bytes.len()),
        DataValue::Date(d) => d.to_string(),
        DataValue::Time(t) => t.to_string(),
        DataValue::DateTime(dt) => dt.to_string(),
        DataValue::Geometry(polygons) => {
            let area = compute_area(polygons);
            let perimeter = compute_perimeter(polygons);
            format!(
                "geometry {} polygon(s) ~{:.2}cm² ~{:.2}cm",
                polygons.len(),
                area,
                perimeter
            )
        }
    }
}

fn run_data_preview_worker(
    connection: &crate::config::ConnectionProperties,
    expression: &str,
) -> Result<DataPreviewData, String> {
    let rule = parse_rule(expression)?;
    let columns = rule.source_fields.clone();
    let rows = fetch_data_preview(connection, &rule, 10)?;

    let stringified = rows
        .iter()
        .map(|row| row.iter().map(data_value_to_display).collect())
        .collect();

    Ok(DataPreviewData { columns, rows: stringified })
}

// ── Renderer ──────────────────────────────────────────────────────────────────

pub(super) fn draw_data_preview(frame: &mut ratatui::Frame, state: &DataPreviewState) {
    let area = centered_rect(90, 80, frame.size());
    frame.render_widget(Clear, area);

    let outer_block = Block::default()
        .title(Span::styled(
            " Data preview (↑/↓ scroll · g/esc closes) ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    match &state.load {
        DataPreviewLoad::Loading => {
            let msg = Paragraph::new("Loading data…")
                .style(Style::default().fg(Color::Cyan))
                .alignment(Alignment::Center);
            frame.render_widget(msg, inner);
        }
        DataPreviewLoad::Done(Err(error)) => {
            let msg = Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: true });
            frame.render_widget(msg, inner);
        }
        DataPreviewLoad::Done(Ok(data)) => {
            draw_data_table(frame, inner, data, state.scroll);
        }
    }
}

fn draw_data_table(frame: &mut ratatui::Frame, area: Rect, data: &DataPreviewData, scroll: usize) {
    if data.columns.is_empty() || area.height < 3 {
        return;
    }

    let header_cells: Vec<Cell> = data
        .columns
        .iter()
        .map(|col| {
            Cell::from(col.as_str())
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        })
        .collect();
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let visible = area.height.saturating_sub(3) as usize; // reserve header + border
    let data_rows: Vec<Row> = data
        .rows
        .iter()
        .skip(scroll)
        .take(visible)
        .map(|row| {
            let cells: Vec<Cell> = row
                .iter()
                .map(|val| Cell::from(val.as_str()).style(Style::default().fg(Color::White)))
                .collect();
            Row::new(cells).height(1)
        })
        .collect();

    let col_count = data.columns.len().max(1) as u32;
    let widths: Vec<Constraint> = (0..col_count)
        .map(|_| Constraint::Ratio(1, col_count))
        .collect();

    let table = Table::new(data_rows, widths)
        .header(header)
        .column_spacing(2);

    frame.render_widget(table, area);
}

