use crate::models::TableSchema;

pub(crate) enum SchemaPanelState {
    Connecting,
    Loaded(Result<Vec<TableSchema>, String>),
}
