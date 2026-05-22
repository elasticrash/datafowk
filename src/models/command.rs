use crate::models::{CliOptions, UiOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Run(CliOptions),
    Ui(UiOptions),
}
