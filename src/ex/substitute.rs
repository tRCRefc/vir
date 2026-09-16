//! Single-line substitutions with ex replacement syntax over regex patterns.
use super::{address::LineRange, parser::Substitution};
use crate::editor::Editor;
use regex::Regex;

enum Part {
    Literal(char),
    Capture(usize),
}

pub fn execute(
    spec: &Substitution,
    range: LineRange,
    previous: Option<String>,
    editor: &mut Editor,
) -> Result<(), String> {
    let pattern = if spec.pattern.is_empty() {
        previous.ok_or_else(|| "no previous search pattern".to_owned())?
    } else {
        spec.pattern.clone()
    };
    let regex = Regex::new(&pattern).map_err(|e| format!("invalid search pattern: {e}"))?;
    let mut parts = Vec::new();
    let mut chars = spec.replacement.chars();
    while let Some(c) = chars.next() {
        parts.push(match c {
            '&' => Part::Capture(0),
            '\\' => match chars
                .next()
                .ok_or_else(|| "incomplete replacement escape".to_owned())?
            {
                c @ '1'..='9' => {
                    let group = c as usize - '0' as usize;
                    if group >= regex.captures_len() {
                        return Err("invalid replacement capture".into());
                    }
                    Part::Capture(group)
                }
                c @ ('&' | '\\' | '/') => Part::Literal(c),
                _ => return Err("unsupported replacement escape".into()),
            },
            '\n' | '\r' | '\0' => return Err("invalid replacement character".into()),
            c => Part::Literal(c),
        });
    }
    if range.start > range.end || range.end >= editor.buffer().line_count() {
        return Err("invalid substitution range".into());
    }
    let mut lines = Vec::new();
    let mut last_changed = None;
    for index in range.start..=range.end {
        let line = editor.buffer().line(index).unwrap();
        let mut result = String::new();
        let mut copied = 0;
        for captures in regex
            .captures_iter(line)
            .take(if spec.global { usize::MAX } else { 1 })
        {
            let matched = captures.get(0).unwrap();
            result.push_str(&line[copied..matched.start()]);
            for part in &parts {
                match part {
                    Part::Literal(c) => result.push(*c),
                    Part::Capture(n) => {
                        if let Some(group) = captures.get(*n) {
                            result.push_str(group.as_str());
                        }
                    }
                }
            }
            copied = matched.end();
            last_changed = Some(index);
        }
        result.push_str(&line[copied..]);
        lines.push(result);
    }
    let current = last_changed.ok_or_else(|| "pattern not found".to_owned())?;
    editor.replace_lines(range.start, range.end, lines)?;
    // Validated range, unchanged line count: this cursor move cannot fail.
    editor
        .set_current(current)
        .expect("substitution preserves line count");
    editor.remember_search(pattern);
    Ok(())
}
