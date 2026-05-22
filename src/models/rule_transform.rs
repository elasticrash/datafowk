#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleTransform {
    pub name: String,
    pub arguments: Vec<String>,
}

impl RuleTransform {
    pub fn expression(&self) -> String {
        if self.arguments.is_empty() {
            self.name.clone()
        } else {
            format!("{}({})", self.name, self.arguments.join(","))
        }
    }
}
