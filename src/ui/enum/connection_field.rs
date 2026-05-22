#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionField {
    Kind,
    Address,
    Port,
    User,
    Password,
    Schema,
}

impl ConnectionField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Kind => Self::Address,
            Self::Address => Self::Port,
            Self::Port => Self::User,
            Self::User => Self::Password,
            Self::Password => Self::Schema,
            Self::Schema => Self::Kind,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Kind => Self::Schema,
            Self::Address => Self::Kind,
            Self::Port => Self::Address,
            Self::User => Self::Port,
            Self::Password => Self::User,
            Self::Schema => Self::Password,
        }
    }
}
