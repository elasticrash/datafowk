use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    config::{Config, RuleConfig},
    enums::{
        ConnectionField, ConnectionTarget, Modal, ModalAction, Pane, RuleEditorMode, RuleField,
        SchemaPanelState, SchemaSide,
    },
    etl::{run_config, save_config},
    models::ExecutionSummary,
};

use super::{
    picker::{
        apply_rule_editor_suggestion, clamp_rule_editor_suggestion, current_rule_field,
        current_rule_field_mut, rule_editor_selected_suggestion, rule_editor_suggestions,
    },
    state::{
        open_rule_editor, AppState, ConnectionDraft, ConnectionEditorState, RuleDraft,
        RuleEditorState,
    },
};

pub(super) fn pump_background_updates(state: &mut AppState) {
    match &mut state.modal {
        Some(Modal::SchemaPreview(schema)) => schema.apply_pending_updates(),
        Some(Modal::RuleEditor(editor)) => {
            while let Ok(message) = editor.updates.try_recv() {
                let panel = match message.side {
                    SchemaSide::Origin => &mut editor.origin_schema,
                    SchemaSide::Destination => &mut editor.destination_schema,
                };
                *panel = SchemaPanelState::Loaded(message.result);
                clamp_rule_editor_suggestion(editor);
            }
        }
        _ => {}
    }
}

pub(super) fn handle_main_input(
    state: &mut AppState,
    config_path: &str,
    key: KeyEvent,
) -> Result<bool, String> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => return Ok(true),
        (KeyCode::Char('?'), _) => {
            state.modal = Some(Modal::Help);
        }
        (KeyCode::Tab, _) => {
            state.active_pane = match state.active_pane {
                Pane::Rules => Pane::Details,
                Pane::Details => Pane::Rules,
            };
        }
        (KeyCode::Up, _) if state.active_pane == Pane::Rules => {
            if state.selected_rule > 0 {
                state.selected_rule -= 1;
                state.sync_selection();
            }
        }
        (KeyCode::Down, _) if state.active_pane == Pane::Rules => {
            if state.selected_rule + 1 < state.config.rules.len() {
                state.selected_rule += 1;
                state.sync_selection();
            }
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            state.modal = Some(Modal::RuleEditor(open_rule_editor(
                &state.config,
                RuleEditorMode::New,
                RuleDraft::default(),
                RuleField::SourceTables,
            )));
            state.status = String::from("Creating new rule");
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => {
            if let Some(expression) = state.selected_rule_expression() {
                let draft = RuleDraft::from_expression(expression).unwrap_or_default();
                state.modal = Some(Modal::RuleEditor(open_rule_editor(
                    &state.config,
                    RuleEditorMode::New,
                    draft,
                    RuleField::DestinationTable,
                )));
                state.status = String::from("Cloning selected rule");
            }
        }
        (KeyCode::Char('e'), KeyModifiers::NONE) | (KeyCode::Enter, _) => {
            if let Some(expression) = state.selected_rule_expression() {
                let draft = RuleDraft::from_expression(expression).unwrap_or_default();
                state.modal = Some(Modal::RuleEditor(open_rule_editor(
                    &state.config,
                    RuleEditorMode::Edit(state.selected_rule),
                    draft,
                    RuleField::SourceTables,
                )));
                state.status = String::from("Editing selected rule");
            }
        }
        (KeyCode::Char('d'), KeyModifiers::NONE) | (KeyCode::Delete, _) => {
            if let Some(rule) = state.config.rules.get(state.selected_rule) {
                let removed = rule.expression.clone();
                state.config.rules.remove(state.selected_rule);
                state.sync_selection();
                state.status = format!("Removed rule: {removed}");
            }
        }
        (KeyCode::Char('o'), KeyModifiers::NONE) => {
            state.modal = Some(Modal::ConnectionEditor(ConnectionEditorState {
                target: ConnectionTarget::Origin,
                draft: ConnectionDraft::from_connection(&state.config.connection_properties_origin),
                field: ConnectionField::Kind,
            }));
            state.status = String::from("Editing origin connection");
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            state.modal = Some(Modal::ConnectionEditor(ConnectionEditorState {
                target: ConnectionTarget::Destination,
                draft: ConnectionDraft::from_connection(
                    &state.config.connection_properties_destination,
                ),
                field: ConnectionField::Kind,
            }));
            state.status = String::from("Editing destination connection");
        }
        (KeyCode::Char('v'), KeyModifiers::NONE) => {
            state.modal = Some(Modal::SchemaPreview(
                super::schema_preview::open_schema_preview(&state.config),
            ));
            state.status = String::from("Schema preview opened");
        }
        (KeyCode::Char('s'), KeyModifiers::NONE) => {
            save_config(config_path, &state.config)?;
            state.status = format!("Saved {config_path}");
        }
        (KeyCode::Char('t'), KeyModifiers::NONE) => {
            state.status = summarize_run(run_config(&state.config, true, false)?, true);
        }
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            state.status = summarize_run(run_config(&state.config, false, false)?, false);
        }
        (KeyCode::Char('x'), KeyModifiers::NONE) => {
            state.status = summarize_run(run_config(&state.config, false, true)?, false);
        }
        _ => {}
    }

    Ok(false)
}

pub(super) fn handle_modal_input(
    modal: &mut Modal,
    config: &mut Config,
    selected_rule: &mut usize,
    key: KeyEvent,
) -> Result<ModalAction, String> {
    match modal {
        Modal::RuleEditor(editor) => handle_rule_editor_input(editor, config, selected_rule, key),
        Modal::ConnectionEditor(editor) => handle_connection_editor_input(editor, config, key),
        Modal::SchemaPreview(schema) => Ok(if schema.handle_key(key.code) {
            ModalAction::Close(None)
        } else {
            ModalAction::Stay
        }),
        Modal::Help => match key.code {
            KeyCode::Esc | KeyCode::Enter => Ok(ModalAction::Close(None)),
            _ => Ok(ModalAction::Stay),
        },
    }
}

pub(super) fn handle_rule_editor_input(
    editor: &mut RuleEditorState,
    config: &mut Config,
    selected_rule: &mut usize,
    key: KeyEvent,
) -> Result<ModalAction, String> {
    if editor.picker_open {
        return handle_rule_picker_input(editor, key);
    }

    match key.code {
        KeyCode::Esc => {
            return Ok(ModalAction::Close(Some(String::from(
                "Rule edit cancelled",
            ))))
        }
        KeyCode::Tab | KeyCode::Down => {
            editor.field = editor.field.next();
            clamp_rule_editor_suggestion(editor);
        }
        KeyCode::BackTab | KeyCode::Up => {
            editor.field = editor.field.previous();
            clamp_rule_editor_suggestion(editor);
        }
        KeyCode::Right => {
            if let Some(suggestion) = rule_editor_selected_suggestion(editor) {
                *current_rule_field_mut(&mut editor.draft, editor.field) =
                    apply_rule_editor_suggestion(
                        current_rule_field(editor),
                        editor.field,
                        &suggestion,
                    );
                clamp_rule_editor_suggestion(editor);
            }
        }
        KeyCode::Backspace => {
            if editor.field != RuleField::Done {
                current_rule_field_mut(&mut editor.draft, editor.field).pop();
                clamp_rule_editor_suggestion(editor);
            }
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            if editor.field != RuleField::Done {
                current_rule_field_mut(&mut editor.draft, editor.field).push(c);
                clamp_rule_editor_suggestion(editor);
            }
        }
        KeyCode::Enter => match editor.field {
            RuleField::SourceTables
            | RuleField::SourceFields
            | RuleField::Transforms
            | RuleField::DestinationTable
            | RuleField::DestinationFields => {
                editor.picker_open = true;
                editor.suggestion_index = 0;
                clamp_rule_editor_suggestion(editor);
            }
            RuleField::Done => {
                let expression = editor.draft.validate()?;
                let status = match editor.mode {
                    RuleEditorMode::New => {
                        config.rules.push(RuleConfig {
                            expression: expression.clone(),
                        });
                        *selected_rule = config.rules.len() - 1;
                        format!("Created rule: {expression}")
                    }
                    RuleEditorMode::Edit(index) => {
                        if let Some(rule) = config.rules.get_mut(index) {
                            rule.expression = expression.clone();
                            *selected_rule = index;
                        }
                        format!("Updated rule: {expression}")
                    }
                };
                return Ok(ModalAction::Close(Some(status)));
            }
            RuleField::JoinConditions => {}
        },
        _ => {}
    }

    Ok(ModalAction::Stay)
}

fn handle_rule_picker_input(
    editor: &mut RuleEditorState,
    key: KeyEvent,
) -> Result<ModalAction, String> {
    match key.code {
        KeyCode::Esc => {
            editor.picker_open = false;
            clamp_rule_editor_suggestion(editor);
        }
        KeyCode::Up => {
            let suggestions = rule_editor_suggestions(editor);
            if !suggestions.is_empty() {
                editor.suggestion_index = editor.suggestion_index.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            let suggestions = rule_editor_suggestions(editor);
            if !suggestions.is_empty() {
                editor.suggestion_index = (editor.suggestion_index + 1).min(suggestions.len() - 1);
            }
        }
        KeyCode::Enter => {
            if let Some(suggestion) = rule_editor_selected_suggestion(editor) {
                *current_rule_field_mut(&mut editor.draft, editor.field) =
                    apply_rule_editor_suggestion(
                        current_rule_field(editor),
                        editor.field,
                        &suggestion,
                    );
            }
            editor.picker_open = false;
            clamp_rule_editor_suggestion(editor);
        }
        KeyCode::Backspace => {
            current_rule_field_mut(&mut editor.draft, editor.field).pop();
            clamp_rule_editor_suggestion(editor);
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            current_rule_field_mut(&mut editor.draft, editor.field).push(c);
            clamp_rule_editor_suggestion(editor);
        }
        _ => {}
    }

    Ok(ModalAction::Stay)
}

fn handle_connection_editor_input(
    editor: &mut ConnectionEditorState,
    config: &mut Config,
    key: KeyEvent,
) -> Result<ModalAction, String> {
    match key.code {
        KeyCode::Esc => {
            return Ok(ModalAction::Close(Some(String::from(
                "Connection edit cancelled",
            ))))
        }
        KeyCode::Tab | KeyCode::Down => editor.field = editor.field.next(),
        KeyCode::Up => editor.field = editor.field.previous(),
        KeyCode::Backspace => match editor.field {
            ConnectionField::Kind => {
                editor.draft.kind.pop();
            }
            ConnectionField::Address => {
                editor.draft.address.pop();
            }
            ConnectionField::Port => {
                editor.draft.port.pop();
            }
            ConnectionField::User => {
                editor.draft.user.pop();
            }
            ConnectionField::Password => {
                editor.draft.password.pop();
            }
            ConnectionField::Schema => {
                editor.draft.schema.pop();
            }
        },
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            match editor.field {
                ConnectionField::Kind => editor.draft.kind.push(c),
                ConnectionField::Address => editor.draft.address.push(c),
                ConnectionField::Port => editor.draft.port.push(c),
                ConnectionField::User => editor.draft.user.push(c),
                ConnectionField::Password => editor.draft.password.push(c),
                ConnectionField::Schema => editor.draft.schema.push(c),
            }
        }
        KeyCode::Enter => {
            let connection = editor.draft.validate()?;
            match editor.target {
                ConnectionTarget::Origin => config.connection_properties_origin = connection,
                ConnectionTarget::Destination => {
                    config.connection_properties_destination = connection
                }
            }
            return Ok(ModalAction::Close(Some(format!(
                "Saved {}",
                editor.target.title().to_lowercase()
            ))));
        }
        _ => {}
    }

    Ok(ModalAction::Stay)
}

fn summarize_run(summary: ExecutionSummary, dry_run: bool) -> String {
    if dry_run {
        format!(
            "Dry run simulation completed: {} rule(s), {} row(s) read, {} row(s) fully validated, {} skipped as duplicates",
            summary.rules_processed, summary.rows_read, summary.rows_inserted, summary.rows_skipped
        )
    } else {
        format!(
            "ETL completed: {} rule(s), {} row(s) read, {} row(s) inserted, {} skipped as duplicates",
            summary.rules_processed, summary.rows_read, summary.rows_inserted, summary.rows_skipped
        )
    }
}
