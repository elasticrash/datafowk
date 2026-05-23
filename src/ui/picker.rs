use crate::{
    enums::{RuleField, SchemaPanelState},
    etl_rule_parser::parser::split_csv_values,
    transforms::SUPPORTED_TRANSFORM_NAMES,
};

use super::{panel_hint::schema_panel_hint, state::RuleEditorState};

pub(super) fn current_rule_field_mut(
    draft: &mut super::state::RuleDraft,
    field: RuleField,
) -> &mut String {
    match field {
        RuleField::SourceTables => &mut draft.source_tables,
        RuleField::JoinConditions => &mut draft.join_conditions,
        RuleField::SourceFields => &mut draft.source_fields,
        RuleField::Transforms => &mut draft.transforms,
        RuleField::DestinationTable => &mut draft.destination_table,
        RuleField::DestinationFields => &mut draft.destination_fields,
        RuleField::Done => &mut draft.destination_fields,
    }
}

pub(super) fn current_rule_field(editor: &RuleEditorState) -> &str {
    match editor.field {
        RuleField::SourceTables => &editor.draft.source_tables,
        RuleField::JoinConditions => &editor.draft.join_conditions,
        RuleField::SourceFields => &editor.draft.source_fields,
        RuleField::Transforms => &editor.draft.transforms,
        RuleField::DestinationTable => &editor.draft.destination_table,
        RuleField::DestinationFields => &editor.draft.destination_fields,
        RuleField::Done => "",
    }
}

fn is_searchable_rule_field(field: RuleField) -> bool {
    matches!(
        field,
        RuleField::SourceTables
            | RuleField::SourceFields
            | RuleField::Transforms
            | RuleField::DestinationTable
            | RuleField::DestinationFields
    )
}

fn current_csv_token(value: &str) -> String {
    value
        .rsplit_once(',')
        .map(|(_, token)| token.trim().to_string())
        .unwrap_or_else(|| value.trim().to_string())
}

fn applied_csv_tokens(value: &str) -> Vec<String> {
    let Ok(parts) = split_csv_values(value) else {
        return Vec::new();
    };

    if value.trim_end().ends_with(',') {
        return parts;
    }

    let mut parts = parts;
    parts.pop();
    parts
}

fn searchable_columns(schema: &SchemaPanelState, table: &str) -> Option<Vec<String>> {
    let SchemaPanelState::Loaded(Ok(tables)) = schema else {
        return None;
    };

    tables
        .iter()
        .find(|candidate| candidate.name == table)
        .map(|table| {
            table
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect()
        })
}

fn searchable_tables(schema: &SchemaPanelState) -> Option<Vec<String>> {
    let SchemaPanelState::Loaded(Ok(tables)) = schema else {
        return None;
    };

    Some(tables.iter().map(|table| table.name.clone()).collect())
}

pub(super) fn rule_editor_suggestions(editor: &RuleEditorState) -> Vec<String> {
    if !is_searchable_rule_field(editor.field) {
        return Vec::new();
    }

    let query = current_csv_token(current_rule_field(editor)).to_ascii_lowercase();
    let current_value = current_rule_field(editor);
    let mut candidates = match editor.field {
        RuleField::SourceTables => searchable_tables(&editor.origin_schema).unwrap_or_default(),
        RuleField::SourceFields => {
            let source_tables = split_csv_values(&editor.draft.source_tables).unwrap_or_default();
            if source_tables.len() <= 1 {
                let Some(table) = source_tables.first() else {
                    return Vec::new();
                };
                searchable_columns(&editor.origin_schema, table).unwrap_or_default()
            } else {
                let mut fields = Vec::new();
                for table in source_tables {
                    if let Some(columns) = searchable_columns(&editor.origin_schema, &table) {
                        fields.extend(
                            columns
                                .into_iter()
                                .map(|column| format!("{table}.{column}")),
                        );
                    }
                }
                fields
            }
        }
        RuleField::DestinationTable => {
            searchable_tables(&editor.destination_schema).unwrap_or_default()
        }
        RuleField::DestinationFields => searchable_columns(
            &editor.destination_schema,
            editor.draft.destination_table.trim(),
        )
        .unwrap_or_default(),
        RuleField::Transforms => SUPPORTED_TRANSFORM_NAMES
            .iter()
            .map(|transform| (*transform).to_string())
            .collect(),
        RuleField::JoinConditions | RuleField::Done => Vec::new(),
    };

    let applied = applied_csv_tokens(current_value);
    candidates.retain(|candidate| !applied.iter().any(|value| value == candidate));
    if !query.is_empty() {
        candidates.retain(|candidate| candidate.to_ascii_lowercase().contains(&query));
    }
    candidates.sort();
    candidates
}

pub(super) fn rule_editor_selected_suggestion(editor: &RuleEditorState) -> Option<String> {
    let suggestions = rule_editor_suggestions(editor);
    suggestions.get(editor.suggestion_index).cloned()
}

pub(super) fn clamp_rule_editor_suggestion(editor: &mut RuleEditorState) {
    let len = rule_editor_suggestions(editor).len();
    if len == 0 {
        editor.suggestion_index = 0;
    } else if editor.suggestion_index >= len {
        editor.suggestion_index = len - 1;
    }
}

pub(super) fn apply_rule_editor_suggestion(
    current: &str,
    field: RuleField,
    suggestion: &str,
) -> String {
    if matches!(field, RuleField::DestinationTable) {
        return suggestion.to_string();
    }

    let prefix = current
        .rsplit_once(',')
        .map(|(prefix, _)| prefix.trim())
        .unwrap_or("");

    if prefix.is_empty() {
        suggestion.to_string()
    } else {
        format!("{prefix},{suggestion}")
    }
}

pub(super) fn search_picker_hint(editor: &RuleEditorState) -> String {
    if !is_searchable_rule_field(editor.field) {
        return String::from("Selected field uses direct typing");
    }

    match editor.field {
        RuleField::SourceTables | RuleField::SourceFields => {
            let hint = schema_panel_hint(&editor.origin_schema, "origin");
            if hint.is_empty() {
                String::from("Press enter to choose from origin schema")
            } else {
                hint
            }
        }
        RuleField::DestinationTable | RuleField::DestinationFields => {
            let hint = schema_panel_hint(&editor.destination_schema, "destination");
            if hint.is_empty() {
                String::from("Press enter to choose from destination schema")
            } else {
                hint
            }
        }
        RuleField::Transforms => String::from("Press enter to choose from supported transforms"),
        RuleField::JoinConditions | RuleField::Done => String::new(),
    }
}
