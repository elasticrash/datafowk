mod input;
mod panel_hint;
mod picker;
mod render;
#[path = "ui/schema_preview.rs"]
pub(crate) mod schema_preview;
mod state;
mod utils;

use std::io;
use std::time::Duration;
use crate::etl::load_config_or_default;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    enums::ModalAction ,
    models::UiOptions,
};
use input::{handle_main_input, handle_modal_input, pump_background_updates};
use render::draw;
use state::AppState;
pub(crate) use state::{ConnectionEditorState, RuleEditorState};

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn run_ui(options: UiOptions) -> Result<(), String> {
    let config = load_config_or_default(&options.config_path)?;

    enable_raw_mode().map_err(|error| format!("failed to enable raw mode: {error}"))?;
    execute!(io::stdout(), EnterAlternateScreen)
        .map_err(|error| format!("failed to open alternate screen: {error}"))?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|error| format!("failed to create terminal: {error}"))?;

    let mut state = AppState::new(config);
    let mut should_quit = false;

    while !should_quit {
        pump_background_updates(&mut state);
        terminal
            .draw(|frame| draw(frame, &mut state, &options.config_path))
            .map_err(|error| format!("failed to draw UI: {error}"))?;

        if event::poll(Duration::from_millis(100))
            .map_err(|error| format!("failed to poll terminal event: {error}"))?
        {
            if let Event::Key(key) =
                event::read().map_err(|error| format!("failed to read terminal event: {error}"))?
            {
                if let Some(modal) = &mut state.modal {
                    match handle_modal_input(
                        modal,
                        &mut state.config,
                        &mut state.selected_rule,
                        key,
                    )? {
                        ModalAction::Stay => {}
                        ModalAction::Close(status) => {
                            state.modal = None;
                            if let Some(status) = status {
                                state.status = status;
                            }
                        }
                    }
                    state.sync_selection();
                    continue;
                }

                should_quit = handle_main_input(&mut state, &options.config_path, key)?;
            }
        }
    }

    terminal
        .show_cursor()
        .map_err(|error| format!("failed to restore cursor: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rule_expression_defaults_transform_to_copy() {
        assert_eq!(
            build_rule_expression("users", "", "firstname", "", "spot", "name"),
            "(origin:users)[firstname]<copy>(destination:spot)[name]"
        );
    }

    #[test]
    fn rule_draft_rejects_mismatched_fields() {
        let draft = RuleDraft {
            source_tables: String::from("users"),
            join_conditions: String::new(),
            source_fields: String::from("firstname,lastname"),
            transforms: String::from("trim"),
            destination_table: String::from("spot"),
            destination_fields: String::from("name"),
        };

        assert!(draft.validate().is_err());
    }

    #[test]
    fn build_rule_expression_supports_join_conditions() {
        assert_eq!(
            build_rule_expression(
                "users,address",
                "users.address_id=address.id",
                "users.firstname,address.address",
                "trim",
                "spot",
                "name,address"
            ),
            "(origin:users,address){users.address_id=address.id}[users.firstname,address.address]<trim>(destination:spot)[name,address]"
        );
    }

    #[test]
    fn parse_database_kind_accepts_aliases() {
        assert_eq!(
            parse_database_kind("postgresql").unwrap(),
            crate::config::DatabaseKind::Postgres
        );
        assert_eq!(
            parse_database_kind("mysql").unwrap(),
            crate::config::DatabaseKind::Mysql
        );
    }

    fn loaded_schema(tables: &[(&str, &[&str])]) -> SchemaPanelState {
        SchemaPanelState::Loaded(Ok(tables
            .iter()
            .map(|(table, columns)| TableSchema {
                name: (*table).to_string(),
                columns: columns
                    .iter()
                    .map(|column| crate::models::TableColumnSchema {
                        name: (*column).to_string(),
                        data_type: String::from("text"),
                    })
                    .collect(),
            })
            .collect()))
    }

    fn empty_editor(field: RuleField) -> RuleEditorState {
        let (_sender, receiver) = mpsc::channel();
        RuleEditorState {
            mode: RuleEditorMode::New,
            draft: RuleDraft::default(),
            field,
            origin_schema: SchemaPanelState::Connecting,
            destination_schema: SchemaPanelState::Connecting,
            updates: receiver,
            suggestion_index: 0,
            picker_open: false,
        }
    }

    #[test]
    fn rule_editor_suggests_source_tables_from_origin_schema() {
        let mut editor = empty_editor(RuleField::SourceTables);
        editor.draft.source_tables = String::from("or");
        editor.origin_schema = loaded_schema(&[("order_totals", &["amount"]), ("users", &["id"])]);

        assert_eq!(
            rule_editor_suggestions(&editor),
            vec![String::from("order_totals")]
        );
    }

    #[test]
    fn rule_editor_suggests_qualified_fields_for_multi_table_sources() {
        let mut editor = empty_editor(RuleField::SourceFields);
        editor.draft.source_tables = String::from("users,address");
        editor.draft.source_fields = String::from("users.fi");
        editor.origin_schema = loaded_schema(&[
            ("users", &["firstname", "lastname"]),
            ("address", &["address"]),
        ]);

        assert_eq!(
            rule_editor_suggestions(&editor),
            vec![String::from("users.firstname")]
        );
    }

    #[test]
    fn apply_rule_editor_suggestion_replaces_current_csv_token() {
        assert_eq!(
            apply_rule_editor_suggestion(
                "users,address.nu",
                RuleField::SourceFields,
                "address.number"
            ),
            "users,address.number"
        );
    }

    #[test]
    fn rule_field_navigation_includes_done() {
        assert_eq!(RuleField::DestinationFields.next(), RuleField::Done);
        assert_eq!(RuleField::Done.previous(), RuleField::DestinationFields);
    }

    #[test]
    fn rule_editor_suggests_supported_transforms() {
        let mut editor = empty_editor(RuleField::Transforms);
        editor.draft.transforms = String::from("up");

        assert_eq!(
            rule_editor_suggestions(&editor),
            vec![String::from("uppercase")]
        );
    }

    #[test]
    fn transform_picker_hint_is_specific() {
        let editor = empty_editor(RuleField::Transforms);
        assert_eq!(
            search_picker_hint(&editor),
            "Press enter to choose from supported transforms"
        );
    }

    #[test]
    fn enter_opens_transform_picker_before_done() {
        let mut editor = empty_editor(RuleField::Transforms);
        let mut config = Config::default();
        let mut selected_rule = 0usize;

        let result = handle_modal_input(
            &mut Modal::RuleEditor(editor),
            &mut config,
            &mut selected_rule,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();

        assert!(matches!(result, ModalAction::Stay));
        assert!(config.rules.is_empty());

        editor = empty_editor(RuleField::Transforms);
        let result = input::handle_rule_editor_input(
            &mut editor,
            &mut config,
            &mut selected_rule,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();

        assert!(matches!(result, ModalAction::Stay));
        assert!(editor.picker_open);
    }

    #[test]
    fn enter_only_saves_rule_editor_on_done() {
        let mut editor = empty_editor(RuleField::SourceTables);
        let mut config = Config::default();
        let mut selected_rule = 0usize;

        let result = input::handle_rule_editor_input(
            &mut editor,
            &mut config,
            &mut selected_rule,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();

        assert!(matches!(result, ModalAction::Stay));
        assert!(config.rules.is_empty());
        assert!(editor.picker_open);

        editor.picker_open = false;
        editor.field = RuleField::Done;
        let result = input::handle_rule_editor_input(
            &mut editor,
            &mut config,
            &mut selected_rule,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .unwrap();

        assert!(matches!(result, ModalAction::Close(Some(_))));
        assert_eq!(config.rules.len(), 1);
    }
}
