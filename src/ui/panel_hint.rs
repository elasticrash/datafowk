use crate::enums::SchemaPanelState;

use super::shorten;

pub(super) fn schema_panel_hint(schema: &SchemaPanelState, side: &str) -> String {
    match schema {
        SchemaPanelState::Connecting => format!("{side} schema: connecting..."),
        SchemaPanelState::Loaded(Err(error)) => {
            format!("{side} schema error: {}", shorten(error, 60))
        }
        SchemaPanelState::Loaded(Ok(_)) => String::new(),
    }
}
