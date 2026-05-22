use crate::models::TableColumnSchema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<TableColumnSchema>,
}
