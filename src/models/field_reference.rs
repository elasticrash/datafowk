#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldReference {
    pub table: Option<String>,
    pub field: String,
}
