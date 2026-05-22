#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleField {
    SourceTables,
    JoinConditions,
    SourceFields,
    Transforms,
    DestinationTable,
    DestinationFields,
    Done,
}

impl RuleField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::SourceTables => Self::JoinConditions,
            Self::JoinConditions => Self::SourceFields,
            Self::SourceFields => Self::Transforms,
            Self::Transforms => Self::DestinationTable,
            Self::DestinationTable => Self::DestinationFields,
            Self::DestinationFields => Self::Done,
            Self::Done => Self::SourceTables,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::SourceTables => Self::DestinationFields,
            Self::JoinConditions => Self::SourceTables,
            Self::SourceFields => Self::JoinConditions,
            Self::Transforms => Self::SourceFields,
            Self::DestinationTable => Self::Transforms,
            Self::DestinationFields => Self::DestinationTable,
            Self::Done => Self::DestinationFields,
        }
    }
}
