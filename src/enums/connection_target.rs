#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionTarget {
    Origin,
    Destination,
}

impl ConnectionTarget {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Origin => "Origin connection",
            Self::Destination => "Destination connection",
        }
    }
}
