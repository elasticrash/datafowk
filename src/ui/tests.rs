use super::{
    input::{self, handle_modal_input},
    picker::{apply_rule_editor_suggestion, rule_editor_suggestions, search_picker_hint},
    state::RuleDraft,
    utils::{build_rule_expression, parse_database_kind},
    RuleEditorState,
};
use crate::{
    config::Config,
    enums::{Modal, ModalAction, RuleEditorMode, RuleField, SchemaPanelState},
    models::TableSchema,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;

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
