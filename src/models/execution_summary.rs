#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub rules_processed: usize,
    pub rows_read: usize,
    pub rows_inserted: usize,
    pub rows_skipped: usize,
    pub dry_run: bool,
}
