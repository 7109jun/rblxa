use std::{fmt, io};

pub type Result<T> = std::result::Result<T, RblxaError>;

#[derive(Debug)]
pub enum RblxaError {
    Io(io::Error),
    Toml(toml::de::Error),
    TomlSerialize(toml::ser::Error),
    Invalid(String),
    Compile(String),
}

impl fmt::Display for RblxaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Toml(e) => write!(f, "RBLXA100 invalid TOML/RBLXA syntax: {e}"),
            Self::TomlSerialize(e) => write!(f, "format error: {e}"),
            Self::Invalid(s) => write!(f, "{s}"),
            Self::Compile(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for RblxaError {}
impl From<io::Error> for RblxaError { fn from(e: io::Error) -> Self { Self::Io(e) } }
impl From<toml::de::Error> for RblxaError { fn from(e: toml::de::Error) -> Self { Self::Toml(e) } }
impl From<toml::ser::Error> for RblxaError { fn from(e: toml::ser::Error) -> Self { Self::TomlSerialize(e) } }
