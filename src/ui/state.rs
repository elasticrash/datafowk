use std::sync::mpsc::{self, Receiver};

use ratatui::widgets::ListState;

use crate::{
    config::{Config, ConnectionProperties},
    enums::{
        ConnectionField, ConnectionTarget, Modal, Pane, RuleEditorMode, RuleField,
        SchemaPanelState, SchemaSide,
    },
    etl_rule_parser::parser::parse_rule,
    models::Rules,
};

use super::{
    schema_preview::{spawn_schema_preview_worker, SchemaPreviewMessage},
    utils::{build_rule_expression, database_kind_label, joins_to_string, parse_database_kind},
};

const DEFAULT_SOURCE_TABLES: &str = "users";
const DEFAULT_JOIN_CONDITIONS: &str = "";
const DEFAULT_SOURCE_FIELDS: &str = "firstname,lastname";
const DEFAULT_TRANSFORMS: &str = "trim";
const DEFAULT_DESTINATION_TABLE: &str = "spot";
const DEFAULT_DESTINATION_FIELDS: &str = "name,surname";

#[derive(Clone)]
pub(super) struct RuleDraft {
    pub(super) source_tables: String,
    pub(super) join_conditions: String,
    pub(super) source_fields: String,
    pub(super) transforms: String,
    pub(super) destination_table: String,
    pub(super) destination_fields: String,
}

impl Default for RuleDraft {
    fn default() -> Self {
        Self {
            source_tables: DEFAULT_SOURCE_TABLES.to_string(),
            join_conditions: DEFAULT_JOIN_CONDITIONS.to_string(),
            source_fields: DEFAULT_SOURCE_FIELDS.to_string(),
            transforms: DEFAULT_TRANSFORMS.to_string(),
            destination_table: DEFAULT_DESTINATION_TABLE.to_string(),
            destination_fields: DEFAULT_DESTINATION_FIELDS.to_string(),
        }
    }
}

impl RuleDraft {
    pub(super) fn from_expression(expression: &str) -> Result<Self, String> {
        let rule = parse_rule(expression)?;
        Ok(Self::from_rule(&rule))
    }

    fn from_rule(rule: &Rules) -> Self {
        Self {
            source_tables: rule.source_tables.join(","),
            join_conditions: joins_to_string(&rule.join_conditions),
            source_fields: rule.source_fields.join(","),
            transforms: rule
                .function_chain
                .iter()
                .map(|transform| transform.expression())
                .collect::<Vec<_>>()
                .join(","),
            destination_table: rule.destination_table.clone(),
            destination_fields: rule.destination_fields.join(","),
        }
    }

    pub(super) fn expression(&self) -> String {
        build_rule_expression(
            &self.source_tables,
            &self.join_conditions,
            &self.source_fields,
            &self.transforms,
            &self.destination_table,
            &self.destination_fields,
        )
    }

    pub(super) fn validate(&self) -> Result<String, String> {
        let expression = self.expression();
        let parsed = parse_rule(&expression)?;

        if parsed.source_fields.len() != parsed.destination_fields.len() {
            return Err(String::from(
                "source and destination field counts must match",
            ));
        }

        Ok(expression)
    }
}

pub(crate) struct RuleEditorState {
    pub(super) mode: RuleEditorMode,
    pub(super) draft: RuleDraft,
    pub(super) field: RuleField,
    pub(super) origin_schema: SchemaPanelState,
    pub(super) destination_schema: SchemaPanelState,
    pub(super) updates: Receiver<SchemaPreviewMessage>,
    pub(super) suggestion_index: usize,
    pub(super) picker_open: bool,
}

#[derive(Clone)]
pub(super) struct ConnectionDraft {
    pub(super) kind: String,
    pub(super) address: String,
    pub(super) port: String,
    pub(super) user: String,
    pub(super) password: String,
    pub(super) schema: String,
}

impl ConnectionDraft {
    pub(super) fn from_connection(connection: &ConnectionProperties) -> Self {
        Self {
            kind: database_kind_label(connection.kind).to_string(),
            address: connection.address.clone(),
            port: connection.port.to_string(),
            user: connection.user.clone(),
            password: connection.password.clone(),
            schema: connection.schema.clone(),
        }
    }

    pub(super) fn validate(&self) -> Result<ConnectionProperties, String> {
        if self.address.trim().is_empty() {
            return Err(String::from("address cannot be empty"));
        }
        if self.user.trim().is_empty() {
            return Err(String::from("user cannot be empty"));
        }
        if self.schema.trim().is_empty() {
            return Err(String::from("schema cannot be empty"));
        }

        let port = self
            .port
            .trim()
            .parse::<u16>()
            .map_err(|error| format!("invalid port: {error}"))?;

        let kind = parse_database_kind(&self.kind)?;

        Ok(ConnectionProperties {
            kind,
            user: self.user.trim().to_string(),
            password: self.password.clone(),
            address: self.address.trim().to_string(),
            port,
            schema: self.schema.trim().to_string(),
        })
    }
}

pub(crate) struct ConnectionEditorState {
    pub(super) target: ConnectionTarget,
    pub(super) draft: ConnectionDraft,
    pub(super) field: ConnectionField,
}

pub(super) struct AppState {
    pub(super) config: Config,
    pub(super) rules_state: ListState,
    pub(super) selected_rule: usize,
    pub(super) active_pane: Pane,
    pub(super) modal: Option<Modal>,
    pub(super) status: String,
}

impl AppState {
    pub(super) fn new(config: Config) -> Self {
        let mut rules_state = ListState::default();
        if !config.rules.is_empty() {
            rules_state.select(Some(0));
        }

        Self {
            config,
            rules_state,
            selected_rule: 0,
            active_pane: Pane::Rules,
            modal: None,
            status: String::from("Ready"),
        }
    }

    pub(super) fn sync_selection(&mut self) {
        if self.config.rules.is_empty() {
            self.selected_rule = 0;
            self.rules_state.select(None);
        } else {
            if self.selected_rule >= self.config.rules.len() {
                self.selected_rule = self.config.rules.len() - 1;
            }
            self.rules_state.select(Some(self.selected_rule));
        }
    }

    pub(super) fn selected_rule_expression(&self) -> Option<&str> {
        self.config
            .rules
            .get(self.selected_rule)
            .map(|rule| rule.expression.as_str())
    }

    pub(super) fn selected_rule_preview(&self) -> Result<Rules, String> {
        let expression = self
            .selected_rule_expression()
            .ok_or_else(|| String::from("no rules defined"))?;
        parse_rule(expression)
    }
}

pub(super) fn open_rule_editor(
    config: &Config,
    mode: RuleEditorMode,
    draft: RuleDraft,
    field: RuleField,
) -> RuleEditorState {
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

    RuleEditorState {
        mode,
        draft,
        field,
        origin_schema: SchemaPanelState::Connecting,
        destination_schema: SchemaPanelState::Connecting,
        updates: receiver,
        suggestion_index: 0,
        picker_open: false,
    }
}
