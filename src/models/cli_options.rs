#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOptions {
    pub config_path: String,
    pub dry_run: bool,
    pub truncate_destination: bool,
}
