//! RBLXA compiler library. Windows-only.

#[cfg(not(windows))]
compile_error!("RBLXA is Windows-only; build this project on a Windows target.");

pub mod build;
pub mod document;
pub mod error;
pub mod format;
pub mod inspect;
pub mod parse;
pub mod resolver;
pub mod schema;
pub mod script;

pub use build::{build_file, build_dom};
pub use document::{RblxaDocument, RblxaObject};
pub use error::{RblxaError, Result};
pub use format::format_file;
pub use parse::parse_file;
