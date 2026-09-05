use std::fmt;

/// Intended shape for the polished transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum FormatContext {
    Auto,
    Email,
    #[cfg_attr(feature = "cli", value(alias = "text"))]
    Message,
    #[cfg_attr(feature = "cli", value(alias = "tasks"))]
    Todo,
    #[cfg_attr(feature = "cli", value(alias = "note"))]
    Notes,
    Plain,
    Verbatim,
}

impl fmt::Display for FormatContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Email => "email",
            Self::Message => "message",
            Self::Todo => "todo",
            Self::Notes => "notes",
            Self::Plain => "plain",
            Self::Verbatim => "verbatim",
        })
    }
}
