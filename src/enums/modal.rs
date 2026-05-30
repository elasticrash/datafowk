use crate::ui::{
    geometry_preview::DataPreviewState, schema_preview::SchemaPreviewState,
    ConnectionEditorState, RuleEditorState,
};

pub(crate) enum Modal {
    RuleEditor(RuleEditorState),
    ConnectionEditor(ConnectionEditorState),
    SchemaPreview(SchemaPreviewState),
    DataPreview(DataPreviewState),
    Help,
}
