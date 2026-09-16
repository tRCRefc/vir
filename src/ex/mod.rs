//! Parsing for the small ex command subset.

pub mod address;
pub mod command;
pub mod parser;
pub mod repl;

pub use parser::{Address, Command, ParseError, Range};

pub mod search;
