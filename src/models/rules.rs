use crate::models::{RuleTransform, SourceJoin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    pub source_db: String,
    pub source_tables: Vec<String>,
    pub join_conditions: Vec<SourceJoin>,
    pub source_fields: Vec<String>,
    pub function_chain: Vec<RuleTransform>,
    pub destination_db: String,
    pub destination_table: String,
    pub destination_fields: Vec<String>,
}
