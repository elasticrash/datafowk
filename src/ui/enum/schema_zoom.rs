#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaZoom {
    Tables,
    Columns,
    Types,
}

impl SchemaZoom {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Tables => Self::Columns,
            Self::Columns => Self::Types,
            Self::Types => Self::Tables,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Tables => Self::Types,
            Self::Columns => Self::Tables,
            Self::Types => Self::Columns,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tables => "1: tables",
            Self::Columns => "2: columns",
            Self::Types => "3: columns + types",
        }
    }
}
