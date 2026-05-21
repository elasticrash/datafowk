use std::io;

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
    config::{Config, ConnectionProperties, RuleConfig},
    etl::{load_config_or_default, run_config, save_config, ExecutionSummary, UiOptions},
    etl_rule_parser::parser::{parse_rule, Rules, SourceJoin},
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuleField {
    SourceTables,
    JoinConditions,
    SourceFields,
    Transforms,
    DestinationTable,
    DestinationFields,
}

impl RuleField {
    fn next(self) -> Self {
        match self {
            Self::SourceTables => Self::JoinConditions,
            Self::JoinConditions => Self::SourceFields,
            Self::SourceFields => Self::Transforms,
            Self::Transforms => Self::DestinationTable,
            Self::DestinationTable => Self::DestinationFields,
            Self::DestinationFields => Self::SourceTables,
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
    Address,
    Port,
    User,
    Password,
    Schema,
}

impl ConnectionField {
    fn next(self) -> Self {
        match self {
            Self::Address => Self::Port,
            Self::Port => Self::User,
            Self::User => Self::Password,
            Self::Password => Self::Schema,
            Self::Schema => Self::Address,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Address => Self::Schema,
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
            transforms: rule.function_chain.join(","),
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
}

struct ConnectionEditorState {
    target: ConnectionTarget,
    draft: ConnectionDraft,
    field: ConnectionField,
}

#[derive(Clone)]
struct ConnectionDraft {
    address: String,
    port: String,
    user: String,
    password: String,
    schema: String,
}

impl ConnectionDraft {
    fn from_connection(connection: &ConnectionProperties) -> Self {
        Self {
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

        Ok(ConnectionProperties {
            user: self.user.trim().to_string(),
            password: self.password.clone(),
            address: self.address.trim().to_string(),
            port,
            schema: self.schema.trim().to_string(),
        })
    }
}

enum Modal {
    RuleEditor(RuleEditorState),
    ConnectionEditor(ConnectionEditorState),
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
            status: String::from(
                "tab switch pane • n new • c clone • e edit • d delete • o/p edit connections • s save • t dry-run(simulate) • r run • x run+truncate • q quit",
            ),
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
        terminal
            .draw(|frame| draw(frame, &mut state, &options.config_path))
            .map_err(|error| format!("failed to draw UI: {error}"))?;

        if let Event::Key(key) =
            event::read().map_err(|error| format!("failed to read terminal event: {error}"))?
        {
            if let Some(modal) = &mut state.modal {
                if let Some(status) =
                    handle_modal_input(modal, &mut state.config, &mut state.selected_rule, key)?
                {
                    state.modal = None;
                    state.status = status;
                }
                state.sync_selection();
                continue;
            }

            should_quit = handle_main_input(&mut state, &options.config_path, key)?;
        }
    }

    terminal
        .show_cursor()
        .map_err(|error| format!("failed to restore cursor: {error}"))?;

    Ok(())
}

fn handle_main_input(
    state: &mut AppState,
    config_path: &str,
    key: KeyEvent,
) -> Result<bool, String> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => return Ok(true),
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
            state.modal = Some(Modal::RuleEditor(RuleEditorState {
                mode: RuleEditorMode::New,
                draft: RuleDraft::default(),
                field: RuleField::SourceTables,
            }));
            state.status = String::from(
                "New rule: type to edit fields • tab/up/down move • enter save • esc cancel",
            );
        }
        (KeyCode::Char('c'), KeyModifiers::NONE) => {
            if let Some(expression) = state.selected_rule_expression() {
                let draft = RuleDraft::from_expression(expression).unwrap_or_default();
                state.modal = Some(Modal::RuleEditor(RuleEditorState {
                    mode: RuleEditorMode::New,
                    draft,
                    field: RuleField::DestinationTable,
                }));
                state.status = String::from(
                    "Clone rule: edit destination • tab/up/down move • enter save • esc cancel",
                );
            }
        }
        (KeyCode::Char('e'), KeyModifiers::NONE) | (KeyCode::Enter, _) => {
            if let Some(expression) = state.selected_rule_expression() {
                let draft = RuleDraft::from_expression(expression).unwrap_or_default();
                state.modal = Some(Modal::RuleEditor(RuleEditorState {
                    mode: RuleEditorMode::Edit(state.selected_rule),
                    draft,
                    field: RuleField::SourceTables,
                }));
                state.status = String::from(
                    "Edit rule: type to edit fields • tab/up/down move • enter save • esc cancel",
                );
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
                field: ConnectionField::Address,
            }));
            state.status = String::from(
                "Edit origin connection: type to edit • tab/up/down move • enter save • esc cancel",
            );
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            state.modal = Some(Modal::ConnectionEditor(ConnectionEditorState {
                target: ConnectionTarget::Destination,
                draft: ConnectionDraft::from_connection(
                    &state.config.connection_properties_destination,
                ),
                field: ConnectionField::Address,
            }));
            state.status = String::from(
                "Edit destination connection: type to edit • tab/up/down move • enter save • esc cancel",
            );
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
) -> Result<Option<String>, String> {
    match modal {
        Modal::RuleEditor(editor) => handle_rule_editor_input(editor, config, selected_rule, key),
        Modal::ConnectionEditor(editor) => handle_connection_editor_input(editor, config, key),
    }
}

fn handle_rule_editor_input(
    editor: &mut RuleEditorState,
    config: &mut Config,
    selected_rule: &mut usize,
    key: KeyEvent,
) -> Result<Option<String>, String> {
    match key.code {
        KeyCode::Esc => return Ok(Some(String::from("Rule edit cancelled"))),
        KeyCode::Tab | KeyCode::Down => editor.field = editor.field.next(),
        KeyCode::Up => editor.field = editor.field.previous(),
        KeyCode::Backspace => {
            current_rule_field_mut(&mut editor.draft, editor.field).pop();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            current_rule_field_mut(&mut editor.draft, editor.field).push(c);
        }
        KeyCode::Enter => {
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
            return Ok(Some(status));
        }
        _ => {}
    }

    Ok(None)
}

fn handle_connection_editor_input(
    editor: &mut ConnectionEditorState,
    config: &mut Config,
    key: KeyEvent,
) -> Result<Option<String>, String> {
    match key.code {
        KeyCode::Esc => return Ok(Some(String::from("Connection edit cancelled"))),
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
            return Ok(Some(format!(
                "Saved {}",
                editor.target.title().to_lowercase()
            )));
        }
        _ => {}
    }

    Ok(None)
}

fn current_rule_field_mut(draft: &mut RuleDraft, field: RuleField) -> &mut String {
    match field {
        RuleField::SourceTables => &mut draft.source_tables,
        RuleField::JoinConditions => &mut draft.join_conditions,
        RuleField::SourceFields => &mut draft.source_fields,
        RuleField::Transforms => &mut draft.transforms,
        RuleField::DestinationTable => &mut draft.destination_table,
        RuleField::DestinationFields => &mut draft.destination_fields,
    }
}

fn current_connection_field_mut(
    draft: &mut ConnectionDraft,
    field: ConnectionField,
) -> &mut String {
    match field {
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
            "Dry run simulation completed: {} rule(s), {} row(s) read, {} row(s) fully validated",
            summary.rules_processed, summary.rows_read, summary.rows_inserted
        )
    } else {
        format!(
            "ETL completed: {} rule(s), {} row(s) read, {} row(s) inserted",
            summary.rules_processed, summary.rows_read, summary.rows_inserted
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
        Line::from("o: edit origin   p: edit destination"),
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
                    Span::raw(rule.function_chain.join(" -> ")),
                ]),
                Line::from(vec![
                    Span::styled("Destination table: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_table),
                ]),
                Line::from(vec![
                    Span::styled("Destination fields: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(rule.destination_fields.join(", ")),
                ]),
                Line::from(""),
                Line::from("n new   c clone   e edit   d delete   s save"),
                Line::from("t dry-run(simulate)   r run   x run+truncate"),
            ]
        }
        Err(error) => vec![
            Line::from(Span::styled(error, Style::default().fg(Color::Yellow))),
            Line::from(""),
            Line::from("n new   c clone   s save   o/p edit connections"),
        ],
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
    let widget = Paragraph::new(status)
        .block(Block::default().borders(Borders::TOP))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true });

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
        Line::from(""),
        Line::from("Join syntax: users.address_id=address.id,users.id=profile.user_id"),
        Line::from("Preview:"),
        Line::from(shorten(&editor.draft.expression(), 88)),
        Line::from(""),
        Line::from("tab/up/down move • enter save • esc close • backspace delete"),
    ];

    let title = match editor.mode {
        RuleEditorMode::New => " New rule ",
        RuleEditorMode::Edit(_) => " Edit rule ",
    };

    let widget = Paragraph::new(rows)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

fn draw_connection_editor(frame: &mut ratatui::Frame, editor: &ConnectionEditorState) {
    let area = centered_rect(70, 48, frame.size());
    frame.render_widget(Clear, area);

    let rows = vec![
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
            "Schema",
            &editor.draft.schema,
            editor.field == ConnectionField::Schema,
        ),
        Line::from(""),
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
        rule.function_chain.join(" -> ")
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
        "{}@{}:{}/{}",
        connection.user, connection.address, connection.port, connection.schema
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
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",")
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
}
