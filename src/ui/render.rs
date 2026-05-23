#[path = "render_dashboard.rs"]
mod render_dashboard;
#[path = "render_editors.rs"]
mod render_editors;
#[path = "render_help.rs"]
mod render_help;

use ratatui::layout::{Constraint, Direction, Layout};

use crate::enums::Modal;

use super::{schema_preview::draw_schema_preview, state::AppState};
use render_dashboard::{
    draw_connections, draw_rule_details, draw_rule_preview, draw_rules_list, draw_status,
};
use render_editors::{draw_connection_editor, draw_rule_editor};
use render_help::draw_help_modal;

pub(super) fn draw(frame: &mut ratatui::Frame, state: &mut AppState, config_path: &str) {
    let size = frame.size();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(2)])
        .split(size);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(vertical[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Min(8),
        ])
        .split(body[1]);

    draw_rules_list(frame, state, body[0]);
    draw_rule_preview(frame, state, right[0]);
    draw_connections(frame, state, right[1], config_path);
    draw_rule_details(frame, state, right[2]);
    draw_status(frame, &state.status, vertical[1]);

    if let Some(modal) = &state.modal {
        match modal {
            Modal::RuleEditor(editor) => draw_rule_editor(frame, editor),
            Modal::ConnectionEditor(editor) => draw_connection_editor(frame, editor),
            Modal::SchemaPreview(schema) => draw_schema_preview(frame, schema),
            Modal::Help => draw_help_modal(frame),
        }
    }
}
