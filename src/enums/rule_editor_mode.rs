#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleEditorMode {
    New,
    Edit(usize),
}
