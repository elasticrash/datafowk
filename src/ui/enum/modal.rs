use super::super::{schema_preview::SchemaPreviewState, ConnectionEditorState, RuleEditorState};

pub(crate) enum Modal {
    RuleEditor(RuleEditorState),
    ConnectionEditor(ConnectionEditorState),
    SchemaPreview(SchemaPreviewState),
    Help,
}
