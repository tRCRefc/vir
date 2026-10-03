//! Resolve search addresses without mutating the editor. Patterns use regex
//! crate syntax, not historical vi BRE. Successful commands publish history.
use regex::Regex;

use super::parser::{Address, ParsedCommand};
use crate::editor::Editor;

pub fn resolve(
    mut parsed: ParsedCommand,
    editor: &Editor,
) -> Result<(ParsedCommand, Option<String>), String> {
    let mut previous = editor.last_search().map(str::to_owned);
    let mut searched = false;
    for address in [
        &mut parsed.range.start,
        &mut parsed.range.end,
        &mut parsed.destination,
    ] {
        if let Some(Address::Mark { name, offset }) = address.as_ref() {
            let row = editor.mark(*name)?.row;
            let line = (row + 1).checked_add_signed(*offset).ok_or("OutOfBounds")?;
            *address = Some(Address::Line(line));
            continue;
        }
        let Some(Address::Search {
            pattern,
            forward,
            offset,
        }) = address.as_ref()
        else {
            continue;
        };
        let pattern = if pattern.is_empty() {
            previous
                .clone()
                .ok_or_else(|| "no previous search pattern".to_owned())?
        } else {
            pattern.clone()
        };
        let regex = Regex::new(&pattern).map_err(|e| format!("invalid search pattern: {e}"))?;
        let count = editor.buffer().line_count();
        let mut index = editor.current().ok_or_else(|| "EmptyBuffer".to_owned())?;
        let mut found = None;
        for _ in 0..count {
            index = if *forward {
                if index + 1 == count { 0 } else { index + 1 }
            } else if index == 0 {
                count - 1
            } else {
                index - 1
            };
            if regex.is_match(editor.buffer().line(index).expect("valid search index")) {
                found = Some(index + 1);
                break;
            }
        }
        let line = found
            .ok_or_else(|| "pattern not found".to_owned())?
            .checked_add_signed(*offset)
            .ok_or_else(|| "OutOfBounds".to_owned())?;
        // The command's address policy decides whether zero is valid (e.g. 0a).
        *address = Some(Address::Line(line));
        previous = Some(pattern);
        searched = true;
    }
    Ok((parsed, if searched { previous } else { None }))
}
