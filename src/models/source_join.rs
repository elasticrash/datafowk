#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceJoin {
    pub left_table: String,
    pub left_field: String,
    pub right_table: String,
    pub right_field: String,
}
