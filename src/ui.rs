use ratatui::prelude::Stylize;
use std::io;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};

use crate::{
    config::{Config, ConnectionProperties, DatabaseKind, RuleConfig},
    etl::{load_config_or_default, preview_schema, run_config, save_config},
    etl_rule_parser::parser::{parse_rule, split_csv_values},
    models::{ExecutionSummary, Rules, SourceJoin, TableSchema, UiOptions},
};

const DEFAULT_SOURCE_TABLES: &str = "users";
const DEFAULT_JOIN_CONDITIONS: &str = "";
const DEFAULT_SOURCE_FIELDS: &str = "firstname,lastname";
const DEFAULT_TRANSFORMS: &str = "trim";
const DEFAULT_DESTINATION_TABLE: &str = "spot";
const DEFAULT_DESTINATION_FIELDS: &str = "name,surname";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Rules,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleField {
    SourceTables,
    JoinConditions,
    SourceFields,
    Transforms,
    DestinationTable,
    DestinationFields,
    Done,
}

impl RuleField {
    fn next(self) -> Self {
        match self {
            Self::SourceTables => Self::JoinConditions,
            Self::JoinConditions => Self::SourceFields,
            Self::SourceFields => Self::Transforms,
            Self::Transforms => Self::DestinationTable,
            Self::DestinationTable => Self::DestinationFields,
            Self::DestinationFields => Self::Done,
            Self::Done => Self::SourceTables,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::SourceTables => Self::DestinationFields,
            Self::JoinConditions => Self::SourceTables,
            Self::SourceFields => Self::JoinConditions,
            Self::Transforms => Self::SourceFields,
            Self::DestinationTable => Self::Transforms,
            Self::DestinationFields => Self::DestinationTable,
            Self::Done => Self::DestinationFields,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectionTarget {
    Origin,
    Destination,
}

impl ConnectionTarget {
    fn title(self) -> &'static str {
        match self {
            Self::Origin => "Origin connection",
            Self::Destination => "Destination connection",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectionField {
    Kind,
    Address,
    Port,
    User,
    Password,
    Schema,
}

impl ConnectionField {
    fn next(self) -> Self {
        match self {
            Self::Kind => Self::Address,
            Self::Address => Self::Port,
            Self::Port => Self::User,
            Self::User => Self::Password,
            Self::Password => Self::Schema,
            Self::Schema => Self::Kind,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Kind => Self::Schema,
            Self::Address => Self::Kind,
            Self::Port => Self::Address,
            Self::User => Self::Port,
            Self::Password => Self::User,
            Self::Schema => Self::Password,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuleEditorMode {
    New,
    Edit(usize),
}

#[derive(Clone)]
struct RuleDraft {
    source_tables: String,
    join_conditions: String,
    source_fields: String,
    transforms: String,
    destination_table: String,
    destination_fields: String,
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
    fn from_expression(expression: &str) -> Result<Self, String> {
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

    fn expression(&self) -> String {
        build_rule_expression(
            &self.source_tables,
            &self.join_conditions,
            &self.source_fields,
            &self.transforms,
            &self.destination_table,
            &self.destination_fields,
        )
    }

    fn validate(&self) -> Result<String, String> {
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

struct RuleEditorState {
    mode: RuleEditorMode,
    draft: RuleDraft,
    field: RuleField,
    origin_schema: SchemaPanelState,
    destination_schema: SchemaPanelState,
    updates: Receiver<SchemaPreviewMessage>,
    suggestion_index: usize,
    picker_open: bool,
}

#[derive(Clone)]
struct ConnectionDraft {
    kind: String,
    address: String,
    port: String,
    user: String,
    password: String,
    schema: String,
}

impl ConnectionDraft {
    fn from_connection(connection: &ConnectionProperties) -> Self {
        Self {
            kind: database_kind_label(connection.kind).to_string(),
            address: connection.address.clone(),
            port: connection.port.to_string(),
            user: connection.user.clone(),
            password: connection.password.clone(),
            schema: connection.schema.clone(),
        }
    }

    fn validate(&self) -> Result<ConnectionProperties, String> {
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

struct ConnectionEditorState {
    target: ConnectionTarget,
    draft: ConnectionDraft,
    field: ConnectionField,
}

struct SchemaPreviewState {
    origin: SchemaPanelState,
    destination: SchemaPanelState,
    updates: Receiver<SchemaPreviewMessage>,
    scroll_x: u16,
    scroll_y: u16,
    zoom: SchemaZoom,
}

enum SchemaPanelState {
    Connecting,
    Loaded(Result<Vec<TableSchema>, String>),
}

#[derive(Clone, Copy)]
enum SchemaSide {
    Origin,
    Destination,
}

struct SchemaPreviewMessage {
    side: SchemaSide,
    result: Result<Vec<TableSchema>, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SchemaZoom {
    Tables,
    Columns,
    Types,
}

impl SchemaZoom {
    fn next(self) -> Self {
        match self {
            Self::Tables => Self::Columns,
            Self::Columns => Self::Types,
            Self::Types => Self::Tables,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Tables => Self::Types,
            Self::Columns => Self::Tables,
            Self::Types => Self::Columns,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Tables => "1: tables",
            Self::Columns => "2: columns",
            Self::Types => "3: columns + types",
        }
    }
}

enum Modal {
    RuleEditor(RuleEditorState),
    ConnectionEditor(ConnectionEditorState),
    SchemaPreview(SchemaPreviewState),
    Help,
}

enum ModalAction {
    Stay,
    Close(Option<String>),
}

struct AppState {
    config: Config,
    rules_state: ListState,
    selected_rule: usize,
    active_pane: Pane,
    modal: Option<Modal>,
    status: String,
}

impl AppState {
    fn new(config: Config) -> Self {
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

    fn sync_selection(&mut self) {
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

    fn selected_rule_expression(&self) -> Option<&str> {
        self.config
            .rules
            .get(self.selected_rule)
            .map(|rule| rule.expression.as_str())
    }

    fn selected_rule_preview(&self) -> Result<Rules, String> {
        let expression = self
            .selected_rule_expression()
            .ok_or_else(|| String::from("no rules defined"))?;
        parse_rule(expression)
    }
}

fn open_rule_editor(
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

fn pump_background_updates(state: &mut AppState) {
    match &mut state.modal {
        Some(Modal::SchemaPreview(schema)) => {
            while let Ok(message) = schema.updates.try_recv() {
                let panel = match message.side {
                    SchemaSide::Origin => &mut schema.origin,
                    SchemaSide::Destination => &mut schema.destination,
                };
                *panel = SchemaPanelState::Loaded(message.result);
            }
        }
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

fn handle_main_input(
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
            state.modal = Some(Modal::SchemaPreview(open_schema_preview(&state.config)));
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

fn handle_modal_input(
    modal: &mut Modal,
    config: &mut Config,
    selected_rule: &mut usize,
    key: KeyEvent,
) -> Result<ModalAction, String> {
    match modal {
        Modal::RuleEditor(editor) => handle_rule_editor_input(editor, config, selected_rule, key),
        Modal::ConnectionEditor(editor) => handle_connection_editor_input(editor, config, key),
        Modal::SchemaPreview(schema) => match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('v') => Ok(ModalAction::Close(None)),
            KeyCode::Up => {
                schema.scroll_y = schema.scroll_y.saturating_sub(1);
                Ok(ModalAction::Stay)
            }
            KeyCode::Down => {
                schema.scroll_y = schema.scroll_y.saturating_add(1);
                Ok(ModalAction::Stay)
            }
            KeyCode::Left => {
                schema.scroll_x = schema.scroll_x.saturating_sub(4);
                Ok(ModalAction::Stay)
            }
            KeyCode::Right => {
                schema.scroll_x = schema.scroll_x.saturating_add(4);
                Ok(ModalAction::Stay)
            }
            KeyCode::Char('1') => {
                schema.zoom = SchemaZoom::Tables;
                Ok(ModalAction::Stay)
            }
            KeyCode::Char('2') => {
                schema.zoom = SchemaZoom::Columns;
                Ok(ModalAction::Stay)
            }
            KeyCode::Char('3') => {
                schema.zoom = SchemaZoom::Types;
                Ok(ModalAction::Stay)
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                schema.zoom = schema.zoom.next();
                Ok(ModalAction::Stay)
            }
            KeyCode::Char('-') => {
                schema.zoom = schema.zoom.previous();
                Ok(ModalAction::Stay)
            }
            _ => Ok(ModalAction::Stay),
        },
        Modal::Help => match key.code {
            KeyCode::Esc | KeyCode::Enter => Ok(ModalAction::Close(None)),
            _ => Ok(ModalAction::Stay),
        },
    }
}

fn handle_rule_editor_input(
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
            RuleField::JoinConditions | RuleField::Transforms => {}
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
        KeyCode::Backspace => {
            current_connection_field_mut(&mut editor.draft, editor.field).pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            current_connection_field_mut(&mut editor.draft, editor.field).push(c);
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

fn current_rule_field_mut(draft: &mut RuleDraft, field: RuleField) -> &mut String {
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

fn current_rule_field(editor: &RuleEditorState) -> &str {
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

fn rule_editor_suggestions(editor: &RuleEditorState) -> Vec<String> {
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
        RuleField::JoinConditions | RuleField::Transforms | RuleField::Done => Vec::new(),
    };

    let applied = applied_csv_tokens(current_value);
    candidates.retain(|candidate| !applied.iter().any(|value| value == candidate));
    if !query.is_empty() {
        candidates.retain(|candidate| candidate.to_ascii_lowercase().contains(&query));
    }
    candidates.sort();
    candidates
}

fn rule_editor_selected_suggestion(editor: &RuleEditorState) -> Option<String> {
    let suggestions = rule_editor_suggestions(editor);
    suggestions.get(editor.suggestion_index).cloned()
}

fn clamp_rule_editor_suggestion(editor: &mut RuleEditorState) {
    let len = rule_editor_suggestions(editor).len();
    if len == 0 {
        editor.suggestion_index = 0;
    } else if editor.suggestion_index >= len {
        editor.suggestion_index = len - 1;
    }
}

fn apply_rule_editor_suggestion(current: &str, field: RuleField, suggestion: &str) -> String {
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

fn current_connection_field_mut(
    draft: &mut ConnectionDraft,
    field: ConnectionField,
) -> &mut String {
    match field {
        ConnectionField::Kind => &mut draft.kind,
        ConnectionField::Address => &mut draft.address,
        ConnectionField::Port => &mut draft.port,
        ConnectionField::User => &mut draft.user,
        ConnectionField::Password => &mut draft.password,
        ConnectionField::Schema => &mut draft.schema,
    }
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

fn draw(frame: &mut ratatui::Frame, state: &mut AppState, config_path: &str) {
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

fn draw_rules_list(frame: &mut ratatui::Frame, state: &mut AppState, area: Rect) {
    let items = if state.config.rules.is_empty() {
        vec![ListItem::new("No rules yet. Press n to create one.")]
    } else {
        state
            .config
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let title =
                    rule_title(&rule.expression).unwrap_or_else(|_| format!("Rule {}", index + 1));
                let subtitle = shorten(&rule.expression, 72);
                ListItem::new(vec![
                    Line::from(Span::styled(title, Style::default().fg(Color::White))),
                    Line::from(Span::styled(subtitle, Style::default().fg(Color::DarkGray))),
                ])
            })
            .collect::<Vec<_>>()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    " Rules ",
                    pane_style(state.active_pane == Pane::Rules),
                ))
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut state.rules_state);
}

fn draw_rule_preview(frame: &mut ratatui::Frame, state: &AppState, area: Rect) {
    let lines = match state.selected_rule_preview() {
        Ok(rule) => rule_diagram_lines(&rule),
        Err(error) => vec![Line::from(Span::styled(
            error,
            Style::default().fg(Color::Yellow),
        ))],
    };

    let preview = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Rule preview ")
                .borders(Borders::ALL),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

    frame.render_widget(preview, area);
}

fn draw_connections(frame: &mut ratatui::Frame, state: &AppState, area: Rect, config_path: &str) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Config: ", Style::default().fg(Color::DarkGray)),
            Span::raw(config_path),
        ]),
        Line::from(vec![
            Span::styled("Origin: ", Style::default().fg(Color::DarkGray)),
            Span::raw(connection_summary(
                &state.config.connection_properties_origin,
            )),
        ]),
        Line::from(vec![
            Span::styled("Destination: ", Style::default().fg(Color::DarkGray)),
            Span::raw(connection_summary(
                &state.config.connection_properties_destination,
            )),
        ]),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Connections ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_rule_details(frame: &mut ratatui::Frame, state: &AppState, area: Rect) {
    let lines = match state.selected_rule_preview() {
        Ok(rule) => {
            vec![
                Line::from(vec![
                    Span::styled("Source tables: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.source_tables.join(", ")),
                ]),
                Line::from(vec![
                    Span::styled("Join conditions: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(if rule.join_conditions.is_empty() {
                        String::from("(none)")
                    } else {
                        joins_to_string(&rule.join_conditions)
                    }),
                ]),
                Line::from(vec![
                    Span::styled("Source fields: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.source_fields.join(", ")),
                ]),
                Line::from(vec![
                    Span::styled("Transforms: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(
                        rule.function_chain
                            .iter()
                            .map(|transform| transform.expression())
                            .collect::<Vec<_>>()
                            .join(" -> "),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Destination table: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_table),
                ]),
                Line::from(vec![
                    Span::styled("Destination fields: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_fields.join(", ")),
                ]),
            ]
        }
        Err(error) => vec![Line::from(Span::styled(
            error,
            Style::default().fg(Color::Yellow),
        ))],
    };

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    " Rule details ",
                    pane_style(state.active_pane == Pane::Details),
                ))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_status(frame: &mut ratatui::Frame, status: &str, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(14)])
        .split(area);

    let status_widget = Paragraph::new(status)
        .block(Block::default().borders(Borders::TOP))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true });
    frame.render_widget(status_widget, sections[0]);

    let help_widget = Paragraph::new(Span::styled(
        "? shortcuts",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
    .block(Block::default().borders(Borders::TOP))
    .alignment(Alignment::Right);
    frame.render_widget(help_widget, sections[1]);
}

fn draw_help_modal(frame: &mut ratatui::Frame) {
    let area = centered_rect(64, 58, frame.size());
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::from(Span::styled(
            "Main",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                " Tab ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" switch pane  "),
        ]),
        Line::from(vec![
            Span::styled(
                " n ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" new rule     "),
            Span::styled(
                " c ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" clone rule   "),
            Span::styled(
                " e ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" edit rule    "),
            Span::styled(
                " d ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" delete rule  "),
        ]),
        Line::from(vec![
            Span::styled(
                " o ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" edit origin  "),
            Span::styled(
                " p ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" edit dest    "),
            Span::styled(
                " v ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::raw(" view schemas "),
        ]),
        Line::from(vec![
            Span::styled(
                " s ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" save         "),
            Span::styled(
                " t ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" dry-run      "),
            Span::styled(
                " r ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" run          "),
            Span::styled(
                " x ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" run+truncate "),
            Span::styled(
                " q ",
                Style::default().bg(Color::DarkGray).fg(Color::White).bold(),
            ),
            Span::raw(" quit         "),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Editors",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(" Rule Editor │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " ▲/▼ ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" move          "),
            Span::styled(
                " Enter ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" open/done     "),
            Span::styled(
                " Esc ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled("      Picker │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " A-Z ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" filter        "),
            Span::styled(
                " ▲/▼ ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" choose        "),
            Span::styled(
                " Enter ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" accept       "),
            Span::styled(
                " Esc ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" back"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Schema preview",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "  ▲▼◀▶  ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" pan viewport"),
        ]),
        Line::from(vec![
            Span::styled(
                "   1    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" tables only     "),
            Span::styled(
                "   2    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" columns         "),
            Span::styled(
                "   3    ",
                Style::default().bg(Color::Yellow).fg(Color::Black).bold(),
            ),
            Span::raw(" columns + types"),
        ]),
        Line::from(vec![
            Span::styled(
                "  + / - ",
                Style::default().bg(Color::Magenta).fg(Color::White).bold(),
            ),
            Span::raw(" cycle zoom      "),
            Span::styled(
                "   Esc  ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" close preview"),
        ]),
        Line::from(""),
        Line::from("Press esc to close"),
    ];

    let widget = Paragraph::new(lines)
        .block(Block::default().title(" Shortcuts ").borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_rule_editor(frame: &mut ratatui::Frame, editor: &RuleEditorState) {
    let area = centered_rect(78, 60, frame.size());
    frame.render_widget(Clear, area);

    let rows = vec![
        rule_editor_line(
            "Source tables",
            &editor.draft.source_tables,
            editor.field == RuleField::SourceTables,
        ),
        rule_editor_line(
            "Join conditions",
            &editor.draft.join_conditions,
            editor.field == RuleField::JoinConditions,
        ),
        rule_editor_line(
            "Source fields",
            &editor.draft.source_fields,
            editor.field == RuleField::SourceFields,
        ),
        rule_editor_line(
            "Transforms",
            &editor.draft.transforms,
            editor.field == RuleField::Transforms,
        ),
        rule_editor_line(
            "Destination table",
            &editor.draft.destination_table,
            editor.field == RuleField::DestinationTable,
        ),
        rule_editor_line(
            "Destination fields",
            &editor.draft.destination_fields,
            editor.field == RuleField::DestinationFields,
        ),
        rule_editor_action_line("Done", editor.field == RuleField::Done),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Join Syntax │ ",
                Style::default().fg(Color::DarkGray).bold(),
            ),
            Span::styled(
                "table1.colA=table2.colB,table2.id=table3.id",
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Preview ",
                Style::default().bg(Color::Blue).fg(Color::White).bold(),
            ),
            Span::styled(
                " ────────────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(
                shorten(&editor.draft.expression(), 85),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(search_picker_hint(editor)),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " ▲/▼ ",
                Style::default().bg(Color::DarkGray).fg(Color::White).bold(),
            ),
            Span::raw(" move  "),
            Span::styled(
                " Enter ",
                Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
            ),
            Span::raw(" open picker  "),
            Span::styled(
                " Enter on Done ",
                Style::default().bg(Color::Green).fg(Color::Black).bold(),
            ),
            Span::raw(" save  "),
            Span::styled(
                " Esc ",
                Style::default().bg(Color::Red).fg(Color::White).bold(),
            ),
            Span::raw(" close"),
        ]),
    ];

    let title = match editor.mode {
        RuleEditorMode::New => " New rule ",
        RuleEditorMode::Edit(_) => " Edit rule ",
    };

    let widget = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);

    if editor.picker_open {
        draw_rule_picker(frame, editor);
    }
}

fn schema_panel_hint(schema: &SchemaPanelState, side: &str) -> String {
    match schema {
        SchemaPanelState::Connecting => format!("{side} schema: connecting..."),
        SchemaPanelState::Loaded(Err(error)) => {
            format!("{side} schema error: {}", shorten(error, 60))
        }
        SchemaPanelState::Loaded(Ok(_)) => String::new(),
    }
}

fn search_picker_hint(editor: &RuleEditorState) -> String {
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
        RuleField::JoinConditions | RuleField::Transforms | RuleField::Done => String::new(),
    }
}

fn draw_rule_picker(frame: &mut ratatui::Frame, editor: &RuleEditorState) {
    let area = centered_rect(56, 46, frame.size());
    frame.render_widget(Clear, area);

    let title = match editor.field {
        RuleField::SourceTables => " Select source table ",
        RuleField::SourceFields => " Select source field ",
        RuleField::DestinationTable => " Select destination table ",
        RuleField::DestinationFields => " Select destination field ",
        RuleField::JoinConditions | RuleField::Transforms | RuleField::Done => " Select value ",
    };

    let mut rows = vec![
        Line::from(format!("Filter: {}", current_rule_field(editor))),
        Line::from(""),
    ];

    let suggestions = rule_editor_suggestions(editor);
    if suggestions.is_empty() {
        rows.push(Line::from(search_picker_hint(editor)));
    } else {
        rows.extend(
            suggestions
                .iter()
                .take(10)
                .enumerate()
                .map(|(index, suggestion)| {
                    let style = if index == editor.suggestion_index {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(
                        format!(
                            "{} {}",
                            if index == editor.suggestion_index {
                                ">"
                            } else {
                                " "
                            },
                            suggestion
                        ),
                        style,
                    ))
                }),
        );
    }

    rows.push(Line::from(""));
    rows.push(Line::from(vec![
        Span::styled(
            " A-Z ",
            Style::default().bg(Color::Magenta).fg(Color::White).bold(),
        ),
        Span::raw(" filter  "),
        Span::styled(
            " ▲/▼ ",
            Style::default().bg(Color::Cyan).fg(Color::Black).bold(),
        ),
        Span::raw(" choose  "),
        Span::styled(
            " Enter ",
            Style::default().bg(Color::Green).fg(Color::Black).bold(),
        ),
        Span::raw(" accept  "),
        Span::styled(
            " Esc ",
            Style::default().bg(Color::Red).fg(Color::White).bold(),
        ),
        Span::raw(" back"),
    ]));

    let widget = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_connection_editor(frame: &mut ratatui::Frame, editor: &ConnectionEditorState) {
    let area = centered_rect(70, 52, frame.size());
    frame.render_widget(Clear, area);

    let rows = vec![
        connection_editor_line(
            "Kind",
            &editor.draft.kind,
            editor.field == ConnectionField::Kind,
        ),
        connection_editor_line(
            "Address",
            &editor.draft.address,
            editor.field == ConnectionField::Address,
        ),
        connection_editor_line(
            "Port",
            &editor.draft.port,
            editor.field == ConnectionField::Port,
        ),
        connection_editor_line(
            "User",
            &editor.draft.user,
            editor.field == ConnectionField::User,
        ),
        connection_editor_line(
            "Password",
            &editor.draft.password,
            editor.field == ConnectionField::Password,
        ),
        connection_editor_line(
            "Schema/DB",
            &editor.draft.schema,
            editor.field == ConnectionField::Schema,
        ),
        Line::from(""),
        Line::from("Kind must be `mysql` or `postgres`"),
        Line::from("tab/up/down move • enter save • esc close • backspace delete"),
    ];

    let widget = Paragraph::new(rows)
        .block(
            Block::default()
                .title(format!(" {} ", editor.target.title()))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_schema_preview(frame: &mut ratatui::Frame, schema: &SchemaPreviewState) {
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

fn open_schema_preview(config: &Config) -> SchemaPreviewState {
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

fn spawn_schema_preview_worker(
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

fn rule_editor_line(label: &str, value: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(vec![
        Span::styled(format!("{label:18}"), Style::default().fg(Color::DarkGray)),
        Span::styled(value.to_string(), style),
    ])
}

fn rule_editor_action_line(label: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    Line::from(Span::styled(format!("[ {label} ]"), style))
}

fn connection_editor_line(label: &str, value: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(vec![
        Span::styled(format!("{label:10}"), Style::default().fg(Color::DarkGray)),
        Span::styled(value.to_string(), style),
    ])
}

fn pane_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn rule_title(expression: &str) -> Result<String, String> {
    let rule = parse_rule(expression)?;
    Ok(format!(
        "{} -> {}",
        format!("{}.{}", rule.source_db, rule.source_tables.join("+")),
        format!("{}.{}", rule.destination_db, rule.destination_table)
    ))
}

fn rule_diagram_lines(rule: &Rules) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for table in &rule.source_tables {
        lines.push(Line::from(Span::styled(
            format!("origin.{table}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
    }

    if !rule.join_conditions.is_empty() {
        lines.push(Line::from(format!(
            "  join on {}",
            joins_to_string(&rule.join_conditions)
        )));
    }

    lines.push(Line::from(format!(
        "  read [{}]",
        rule.source_fields.join(", ")
    )));
    lines.push(Line::from("      |"));
    lines.push(Line::from(format!(
        "      +-- {}",
        rule.function_chain
            .iter()
            .map(|transform| transform.expression())
            .collect::<Vec<_>>()
            .join(" -> ")
    )));
    lines.push(Line::from("      v"));
    lines.push(Line::from(Span::styled(
        format!("destination.{}", rule.destination_table),
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "  write [{}]",
        rule.destination_fields.join(", ")
    )));

    lines
}

fn connection_summary(connection: &ConnectionProperties) -> String {
    format!(
        "{} {}@{}:{}/{}",
        database_kind_label(connection.kind),
        connection.user,
        connection.address,
        connection.port,
        connection.schema
    )
}

fn shorten(value: &str, max_len: usize) -> String {
    let count = value.chars().count();
    if count <= max_len {
        return value.to_string();
    }

    if max_len <= 1 {
        return "…".to_string();
    }

    let truncated = value.chars().take(max_len - 1).collect::<String>();
    format!("{truncated}…")
}

fn build_rule_expression(
    source_tables: &str,
    join_conditions: &str,
    source_fields: &str,
    transforms: &str,
    destination_table: &str,
    destination_fields: &str,
) -> String {
    let transforms = if transforms.trim().is_empty() {
        "copy"
    } else {
        transforms.trim()
    };

    let join_section = if join_conditions.trim().is_empty() {
        String::new()
    } else {
        format!("{{{}}}", normalize_csv(join_conditions))
    };

    format!(
        "(origin:{}){}[{}]<{}>(destination:{})[{}]",
        normalize_csv(source_tables),
        join_section,
        normalize_csv(source_fields),
        normalize_csv(transforms),
        destination_table.trim(),
        normalize_csv(destination_fields)
    )
}

fn normalize_csv(value: &str) -> String {
    split_csv_values(value).unwrap_or_default().join(",")
}

fn joins_to_string(joins: &[SourceJoin]) -> String {
    joins
        .iter()
        .map(|join| {
            format!(
                "{}.{}={}.{}",
                join.left_table, join.left_field, join.right_table, join.right_field
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_database_kind(value: &str) -> Result<DatabaseKind, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mysql" => Ok(DatabaseKind::Mysql),
        "postgres" | "postgresql" => Ok(DatabaseKind::Postgres),
        other => Err(format!("unsupported database kind `{other}`")),
    }
}

fn database_kind_label(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::Mysql => "mysql",
        DatabaseKind::Postgres => "postgres",
    }
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
    fn shorten_truncates_long_strings() {
        assert_eq!(shorten("abcdef", 4), "abc…");
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
            DatabaseKind::Postgres
        );
        assert_eq!(parse_database_kind("mysql").unwrap(), DatabaseKind::Mysql);
    }

    #[test]
    fn schema_graph_rows_support_zoom_levels() {
        let tables = vec![TableSchema {
            name: String::from("users"),
            columns: vec![
                crate::models::TableColumnSchema {
                    name: String::from("id"),
                    data_type: String::from("integer"),
                },
                crate::models::TableColumnSchema {
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
    fn enter_only_saves_rule_editor_on_done() {
        let mut editor = empty_editor(RuleField::SourceTables);
        let mut config = Config::default();
        let mut selected_rule = 0usize;

        let result = handle_rule_editor_input(
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
        let result = handle_rule_editor_input(
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
