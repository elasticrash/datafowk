use crate::etl::load_config_or_default;
use crate::{enums::ModalAction, models::UiOptions};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use super::{
    input::{handle_main_input, handle_modal_input, pump_background_updates},
    project_picker::run_project_picker,
    render::draw,
    state::AppState,
};

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub(crate) fn run_ui(options: UiOptions) -> Result<(), String> {
    enable_raw_mode().map_err(|error| format!("failed to enable raw mode: {error}"))?;
    execute!(io::stdout(), EnterAlternateScreen)
        .map_err(|error| format!("failed to open alternate screen: {error}"))?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal =
        Terminal::new(backend).map_err(|error| format!("failed to create terminal: {error}"))?;

    let config_path = match options.config_path {
        Some(path) => path,
        None => match run_project_picker(&mut terminal)? {
            Some(path) => path,
            None => return Ok(()),
        },
    };

    let config = load_config_or_default(&config_path)?;

    let mut state = AppState::new(config);
    let mut should_quit = false;

    while !should_quit {
        pump_background_updates(&mut state);
        terminal
            .draw(|frame| draw(frame, &mut state, &config_path))
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

                should_quit = handle_main_input(&mut state, &config_path, key)?;
            }
        }
    }

    terminal
        .show_cursor()
        .map_err(|error| format!("failed to restore cursor: {error}"))?;

    Ok(())
}
