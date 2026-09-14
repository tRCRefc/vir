//! Parsing for the small ex command subset.

pub mod address;
pub mod command;
pub mod parser;

pub use parser::{Address, Command, ParseError, Range};
