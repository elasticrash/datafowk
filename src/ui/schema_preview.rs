use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::{
    config::{Config, ConnectionProperties},
    enums::{SchemaPanelState, SchemaSide, SchemaZoom},
    etl::preview_schema,
    etl_rule_parser::parser::parse_rule,
    models::TableSchema,
};

use super::utils::centered_rect;

/// Fixed width of each table box (outer, including borders).
const BOX_W: u16 = 26;

pub(crate) struct SchemaPreviewState {
    pub(super) origin: SchemaPanelState,
    pub(super) destination: SchemaPanelState,
    pub(super) updates: Receiver<SchemaPreviewMessage>,
    /// (source_tables, destination_table) pairs parsed from rules.
    connections: Vec<(Vec<String>, String)>,
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

pub(super) fn draw_schema_preview(frame: &mut ratatui::Frame, state: &SchemaPreviewState) {
    let area = centered_rect(92, 88, frame.size());
    frame.render_widget(Clear, area);

    // Outer border — makes it look like a proper popup.
    let outer_block = Block::default()
        .title(Span::styled(
            " Database schemas ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(10)])
        .split(inner);

    let hint = Paragraph::new(format!(
        "↑↓ scroll  •  1/2/3 or +/- zoom  •  current: {}  •  esc closes",
        state.zoom.label()
    ))
    .block(Block::default().borders(Borders::BOTTOM))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    frame.render_widget(hint, chunks[0]);

    frame.render_widget(SchemaCanvasWidget { state }, chunks[1]);
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

    let connections = config
        .rules
        .iter()
        .filter_map(|rc| parse_rule(&rc.expression).ok())
        .map(|rule| (rule.source_tables, rule.destination_table))
        .collect();

    SchemaPreviewState {
        origin: SchemaPanelState::Connecting,
        destination: SchemaPanelState::Connecting,
        updates: receiver,
        connections,
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

// ── Canvas widget ─────────────────────────────────────────────────────────────

struct SchemaCanvasWidget<'a> {
    state: &'a SchemaPreviewState,
}

impl Widget for SchemaCanvasWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 2 {
            return;
        }

        let state = self.state;
        let half_w = area.width / 2;

        // Left half for origin, right half for destination.
        let origin_area = Rect { x: area.x, y: area.y, width: half_w, height: area.height };
        let dest_area = Rect {
            x: area.x + half_w,
            y: area.y,
            width: area.width - half_w,
            height: area.height,
        };

        // Fit boxes within their halves; leave 2 chars margin each side.
        let box_w = BOX_W.min(half_w.saturating_sub(4));
        let origin_box_x = origin_area.x + 1;
        // Leave 3 chars on the left of the dest half for the arrow tip.
        let dest_box_x = dest_area.x + 3;

        // Panel header labels.
        draw_panel_header(buf, origin_area, "─ Origin", Color::Green);
        draw_panel_header(buf, dest_area, "─ Destination", Color::Blue);

        // Canvas rows start one line below the header.
        let canvas_y = area.y + 1;
        let canvas_h = area.height.saturating_sub(1);
        let origin_canvas =
            Rect { x: origin_area.x, y: canvas_y, width: half_w, height: canvas_h };
        let dest_canvas = Rect {
            x: dest_area.x,
            y: canvas_y,
            width: area.width - half_w,
            height: canvas_h,
        };

        let origin_tables = panel_tables(&state.origin);
        let dest_tables = panel_tables(&state.destination);

        // Compute the screen Y of each table's title row for connector routing.
        let origin_pos =
            table_title_y_positions(&origin_tables, state.zoom, state.scroll_y, origin_canvas);
        let dest_pos =
            table_title_y_positions(&dest_tables, state.zoom, state.scroll_y, dest_canvas);

        // Draw connectors behind the boxes.
        let conn_style = Style::default().fg(Color::DarkGray);
        for (src_tables, dst_table) in &state.connections {
            for src_table in src_tables {
                if let (Some(&fy), Some(&ty)) =
                    (origin_pos.get(src_table.as_str()), dest_pos.get(dst_table.as_str()))
                {
                    // fx: one past the right border of the origin box.
                    // tx: one before the left border of the dest box (where ▶ lands).
                    let fx = origin_box_x + box_w;
                    let tx = dest_box_x - 1;
                    draw_connector(buf, fx, fy, tx, ty, conn_style, area);
                }
            }
        }

        // Draw the table boxes on top.
        draw_table_boxes(
            buf, &state.origin, &origin_tables, origin_box_x, box_w,
            origin_canvas, state.zoom, state.scroll_y, Color::Green,
        );
        draw_table_boxes(
            buf, &state.destination, &dest_tables, dest_box_x, box_w,
            dest_canvas, state.zoom, state.scroll_y, Color::Blue,
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn draw_panel_header(buf: &mut Buffer, area: Rect, label: &str, color: Color) {
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    for (i, ch) in label.chars().enumerate() {
        let x = area.x + i as u16;
        if x < area.x + area.width {
            buf.get_mut(x, area.y).set_char(ch).set_style(style);
        }
    }
    let filler_style = Style::default().fg(Color::DarkGray);
    for x in (area.x + label.len() as u16)..(area.x + area.width) {
        buf.get_mut(x, area.y).set_char('─').set_style(filler_style);
    }
}

fn panel_tables(panel: &SchemaPanelState) -> Vec<TableSchema> {
    match panel {
        SchemaPanelState::Loaded(Ok(tables)) => tables.clone(),
        _ => Vec::new(),
    }
}

/// Returns the box height (including the trailing blank line) for a table at the given zoom.
fn table_box_height(table: &TableSchema, zoom: SchemaZoom) -> u16 {
    let n_col_rows = match zoom {
        SchemaZoom::Tables => 0,
        SchemaZoom::Columns | SchemaZoom::Types => table.columns.len() as u16,
    };
    // top + title + bottom  (+separator + col rows when zoomed in)
    let box_h = if n_col_rows == 0 { 3 } else { 3 + 1 + n_col_rows };
    box_h + 1 // blank line between tables
}

/// Maps each table name to the absolute screen Y of its title row (if visible).
fn table_title_y_positions(
    tables: &[TableSchema],
    zoom: SchemaZoom,
    scroll_y: u16,
    area: Rect,
) -> HashMap<String, u16> {
    let mut map = HashMap::new();
    let mut row_offset: i32 = 0;
    for table in tables {
        // Title is one row below the top border.
        let title_row = row_offset + 1;
        let sy = area.y as i32 + title_row - scroll_y as i32;
        if sy >= area.y as i32 && sy < (area.y + area.height) as i32 {
            map.insert(table.name.clone(), sy as u16);
        }
        row_offset += table_box_height(table, zoom) as i32;
    }
    map
}

/// Draw all table boxes for one side. Falls back to a status message when not yet loaded.
fn draw_table_boxes(
    buf: &mut Buffer,
    panel: &SchemaPanelState,
    tables: &[TableSchema],
    box_x: u16,
    box_w: u16,
    area: Rect,
    zoom: SchemaZoom,
    scroll_y: u16,
    color: Color,
) {
    // Show status messages when not loaded.
    match panel {
        SchemaPanelState::Connecting => {
            draw_status_line(buf, box_x, area.y, "Connecting…", Color::Cyan, area);
            return;
        }
        SchemaPanelState::Loaded(Err(err)) => {
            draw_status_line(buf, box_x, area.y, err, Color::Yellow, area);
            return;
        }
        SchemaPanelState::Loaded(Ok(t)) if t.is_empty() => {
            draw_status_line(buf, box_x, area.y, "No tables found", Color::DarkGray, area);
            return;
        }
        _ => {}
    }

    let border_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(Color::DarkGray);
    let inner_w = box_w.saturating_sub(2) as usize;
    let h_bar: String = "─".repeat(inner_w);
    let mut row_offset: i32 = 0;

    for table in tables {
        let col_rows: Vec<String> = match zoom {
            SchemaZoom::Tables => vec![],
            SchemaZoom::Columns => table.columns.iter().map(|c| c.name.clone()).collect(),
            SchemaZoom::Types => table
                .columns
                .iter()
                .map(|c| format!("{}: {}", c.name, c.data_type))
                .collect(),
        };

        let box_h = table_box_height(table, zoom);
        let vis_top = scroll_y as i32;
        let vis_bot = vis_top + area.height as i32;
        // Skip boxes entirely outside the visible range.
        if row_offset + box_h as i32 <= vis_top || row_offset >= vis_bot {
            row_offset += box_h as i32;
            continue;
        }

        // Top border
        draw_box_row(buf, box_x, box_w, row_offset, 0, scroll_y, area,
            '┌', &h_bar, border_style, '┐', border_style);
        // Title
        let title = box_centre_str(&table.name, inner_w);
        draw_box_row(buf, box_x, box_w, row_offset, 1, scroll_y, area,
            '│', &title, border_style, '│', border_style);

        if col_rows.is_empty() {
            // Bottom border without separator.
            draw_box_row(buf, box_x, box_w, row_offset, 2, scroll_y, area,
                '└', &h_bar, border_style, '┘', border_style);
        } else {
            // Separator
            draw_box_row(buf, box_x, box_w, row_offset, 2, scroll_y, area,
                '├', &h_bar, border_style, '┤', border_style);
            for (i, row) in col_rows.iter().enumerate() {
                let padded = box_pad_str(row, inner_w);
                draw_box_row(buf, box_x, box_w, row_offset, 3 + i as i32, scroll_y, area,
                    '│', &padded, dim_style, '│', border_style);
            }
            // Bottom border
            draw_box_row(buf, box_x, box_w, row_offset, 3 + col_rows.len() as i32,
                scroll_y, area, '└', &h_bar, border_style, '┘', border_style);
        }

        row_offset += box_h as i32;
    }
}

/// Draw one horizontal row of a table box at the correct screen position.
#[allow(clippy::too_many_arguments)]
fn draw_box_row(
    buf: &mut Buffer,
    box_x: u16,
    box_w: u16,
    box_top: i32,
    row_in_box: i32,
    scroll_y: u16,
    area: Rect,
    ch_left: char,
    content: &str,
    content_style: Style,
    ch_right: char,
    border_style: Style,
) {
    let sy = area.y as i32 + box_top + row_in_box - scroll_y as i32;
    if sy < area.y as i32 || sy >= (area.y + area.height) as i32 {
        return;
    }
    let y = sy as u16;
    let right_x = box_x + box_w - 1;

    if box_x < area.x + area.width {
        buf.get_mut(box_x, y).set_char(ch_left).set_style(border_style);
    }
    for (i, ch) in content.chars().enumerate() {
        let x = box_x + 1 + i as u16;
        if x >= area.x + area.width || x >= right_x {
            break;
        }
        buf.get_mut(x, y).set_char(ch).set_style(content_style);
    }
    if right_x < area.x + area.width {
        buf.get_mut(right_x, y).set_char(ch_right).set_style(border_style);
    }
}

fn draw_status_line(buf: &mut Buffer, x: u16, y: u16, text: &str, color: Color, area: Rect) {
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    for (i, ch) in text.chars().enumerate() {
        let cx = x + i as u16;
        if cx >= area.x + area.width {
            break;
        }
        buf.get_mut(cx, y).set_char(ch).set_style(style);
    }
}

/// Draw an elbow connector from (fx, fy) → (tx, ty) with a ▶ tip at tx.
fn draw_connector(
    buf: &mut Buffer,
    fx: u16,
    fy: u16,
    tx: u16,
    ty: u16,
    style: Style,
    area: Rect,
) {
    let set = |buf: &mut Buffer, x: u16, y: u16, ch: char| {
        if x >= area.x && y >= area.y && x < area.x + area.width && y < area.y + area.height {
            buf.get_mut(x, y).set_char(ch).set_style(style);
        }
    };

    if fx >= tx {
        return;
    }

    if fy == ty {
        for x in fx..tx {
            set(buf, x, fy, '─');
        }
        set(buf, tx, fy, '▶');
        return;
    }

    let mid_x = fx + (tx.saturating_sub(fx)) / 2;

    // Horizontal from source to midpoint.
    for x in fx..mid_x {
        set(buf, x, fy, '─');
    }

    // Top corner.
    let (top_corner, bot_corner) = if fy < ty { ('╮', '╰') } else { ('╯', '╭') };
    set(buf, mid_x, fy, top_corner);

    // Vertical segment.
    let (y0, y1) = if fy < ty { (fy, ty) } else { (ty, fy) };
    for y in (y0 + 1)..y1 {
        set(buf, mid_x, y, '│');
    }

    // Bottom corner.
    set(buf, mid_x, ty, bot_corner);

    // Horizontal from midpoint to arrow tip.
    for x in (mid_x + 1)..tx {
        set(buf, x, ty, '─');
    }
    set(buf, tx, ty, '▶');
}

fn box_centre_str(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.chars().take(width).collect();
    }
    let total_pad = width - len;
    let left_pad = total_pad / 2;
    let right_pad = total_pad - left_pad;
    format!("{}{}{}", " ".repeat(left_pad), s, " ".repeat(right_pad))
}

fn box_pad_str(s: &str, width: usize) -> String {
    let s = format!(" {s}");
    let len = s.chars().count();
    if len >= width {
        return s.chars().take(width).collect();
    }
    format!("{s:<width$}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TableColumnSchema;

    fn make_table(name: &str, columns: &[(&str, &str)]) -> TableSchema {
        TableSchema {
            name: name.to_string(),
            columns: columns
                .iter()
                .map(|(n, t)| TableColumnSchema {
                    name: n.to_string(),
                    data_type: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn table_box_height_tables_zoom() {
        let t = make_table("users", &[("id", "integer"), ("email", "text")]);
        // Tables zoom: no col rows → top+title+bottom + blank = 4
        assert_eq!(table_box_height(&t, SchemaZoom::Tables), 4);
    }

    #[test]
    fn table_box_height_columns_zoom() {
        let t = make_table("users", &[("id", "integer"), ("email", "text")]);
        // Columns zoom: top+title+sep+2cols+bottom + blank = 7
        assert_eq!(table_box_height(&t, SchemaZoom::Columns), 7);
    }

    #[test]
    fn table_title_positions_visible_tables() {
        let tables = vec![
            make_table("users", &[]),
            make_table("orders", &[]),
        ];
        let area = Rect { x: 0, y: 0, width: 40, height: 20 };
        let pos = table_title_y_positions(&tables, SchemaZoom::Tables, 0, area);
        // First table title at row 1 (top border = 0).
        assert_eq!(pos.get("users"), Some(&1));
        // Second table starts at row 4 (height = 4), title at row 5.
        assert_eq!(pos.get("orders"), Some(&5));
    }

    #[test]
    fn table_title_positions_respects_scroll() {
        let tables = vec![make_table("users", &[])];
        let area = Rect { x: 0, y: 0, width: 40, height: 20 };
        // Scroll past the first table (height=4) → title row 1 < scroll 4 → not visible.
        let pos = table_title_y_positions(&tables, SchemaZoom::Tables, 4, area);
        assert!(pos.get("users").is_none());
    }
}
