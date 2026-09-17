use super::{Motion, VisualState, clip, screen_column};
use crate::editor::Editor;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, IsTerminal, Write};

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Also runs when a read/write error or unwinding panic leaves the loop.
        let _ = execute!(io::stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Default)]
struct CommandLine {
    input: Option<String>,
    message: Option<String>,
    search_direction: Option<bool>,
}

impl CommandLine {
    fn execute(&mut self, text: &str, editor: &mut Editor, state: &mut VisualState) -> bool {
        let result = state.execute_ex(text, editor);
        match result {
            Ok(result) => {
                self.message = if result.output.is_empty() {
                    None
                } else {
                    Some(result.output.trim_end_matches('\n').to_owned())
                };
                result.quit
            }
            Err(error) => {
                self.message = Some(format!("? {error}"));
                false
            }
        }
    }

    // Called only while command input is active; editing keys never move the file cursor.
    fn key(&mut self, key: KeyEvent) {
        let input = self.input.as_mut().unwrap();
        match key.code {
            KeyCode::Esc => self.input = None,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input = None
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Enter => {
                // Enter is handled by the event loop so searches can update VisualState.
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                input.push(c)
            }
            _ => {}
        }
    }

    fn display(&self, width: u16) -> Option<(String, Option<u16>)> {
        let columns = usize::from(width.saturating_sub(1));
        if let Some(input) = &self.input {
            let prefix = self
                .search_direction
                .map_or(':', |forward| if forward { '/' } else { '?' });
            let text = format!("{prefix}{input}");
            let end = screen_column(&text, text.chars().count());
            let left = end.saturating_sub(columns.saturating_sub(1));
            Some((
                clip(&text, left, columns),
                (columns > 0).then_some((end - left) as u16),
            ))
        } else {
            self.message
                .as_ref()
                .map(|text| (clip(text, 0, columns), None))
        }
    }
}

fn read_event(width: u16, height: u16) -> io::Result<Event> {
    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            return event::read();
        }
        let (w, h) = terminal::size()?;
        if (w, h) != (width, height) {
            return Ok(Event::Resize(w, h));
        }
    }
}

fn insert_mode(
    editor: &mut Editor,
    state: &mut VisualState,
    mode: char,
    width: &mut u16,
    height: &mut u16,
) -> io::Result<()> {
    let name = editor
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No file]".into());
    let mut edit = state.begin_insert(editor, mode).map_err(io::Error::other)?;
    let result = (|| -> io::Result<()> {
        loop {
            let position = edit.cursor();
            state.row = position.row;
            state.column = position.column;
            let frame = state.insert_frame(edit.buffer(), &name, *width, *height);
            let mut output = io::stdout().lock();
            queue!(
                output,
                cursor::Hide,
                cursor::MoveTo(0, 0),
                Clear(ClearType::All)
            )?;
            for (row, line) in frame.lines.iter().enumerate() {
                queue!(output, cursor::MoveTo(0, row as u16), Print(line))?;
            }
            if *height > 0 {
                queue!(output, cursor::MoveTo(0, *height - 1), Print(frame.status))?;
            }
            if let Some((x, y)) = frame.cursor {
                queue!(output, cursor::MoveTo(x, y), cursor::Show)?;
            }
            output.flush()?;
            drop(output);
            match read_event(*width, *height)? {
                Event::Resize(w, h) => {
                    *width = w;
                    *height = h;
                }
                Event::Key(key)
                    if key.kind != KeyEventKind::Release
                        && insert_key(&mut edit, key).map_err(io::Error::other)? =>
                {
                    return Ok(());
                }
                _ => {}
            }
        }
    })();
    // Retain typed text in the editor even when terminal I/O fails.
    let position = edit.commit().map_err(io::Error::other)?;
    state.finish_insert(position);
    result
}

fn insert_key(edit: &mut crate::editor::TextEdit<'_>, key: KeyEvent) -> Result<bool, String> {
    match key.code {
        KeyCode::Esc => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Backspace => edit.backspace()?,
        KeyCode::Delete => edit.delete()?,
        KeyCode::Enter => edit.insert("\n")?,
        KeyCode::Tab => edit.insert("\t")?,
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !c.is_control() =>
        {
            edit.insert(&c.to_string())?
        }
        _ => {}
    }
    Ok(false)
}

pub fn run(editor: &mut Editor) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "visual mode requires a terminal on stdin and stdout",
        ));
    }
    let _guard = TerminalGuard::enter()?;
    let mut state = VisualState::default();
    let mut command = CommandLine::default();
    let mut pending_count = 0usize;
    let mut pending_delete = false;
    let (mut width, mut height) = terminal::size()?;
    loop {
        let name = editor
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[No file]".into());
        let name = if editor.is_modified() {
            format!("[+] {name}")
        } else {
            name
        };
        let frame = state.frame(editor.buffer(), &name, width, height);
        if !editor.buffer().is_empty() {
            editor
                .set_current(state.row)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        let mut output = io::stdout().lock();
        queue!(
            output,
            cursor::Hide,
            cursor::MoveTo(0, 0),
            Clear(ClearType::All)
        )?;
        for (row, line) in frame.lines.iter().enumerate() {
            queue!(output, cursor::MoveTo(0, row as u16), Print(line))?;
        }
        if height > 0 {
            queue!(
                output,
                cursor::MoveTo(0, height - 1),
                Print(
                    command
                        .display(width)
                        .map_or(frame.status, |(text, _)| text)
                )
            )?;
        }
        let cursor = if command.input.is_some() {
            command
                .display(width)
                .and_then(|(_, x)| x)
                .filter(|_| height > 0)
                .map(|x| (x, height - 1))
        } else {
            frame.cursor
        };
        if let Some((x, y)) = cursor {
            queue!(output, cursor::MoveTo(x, y), cursor::Show)?;
        }
        output.flush()?;
        drop(output);
        // Some terminal environments do not deliver resize events reliably.
        // Check dimensions while idle as well, including during command input.
        let event = read_event(width, height)?;
        match event {
            Event::Resize(w, h) => {
                width = w;
                height = h;
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if command.input.is_some() {
                    if key.code == KeyCode::Enter {
                        let text = command.input.take().unwrap();
                        if let Some(forward) = command.search_direction {
                            if let Err(error) = state.search(editor, &text, forward) {
                                command.message = Some(format!("? {error}"));
                            }
                        } else if !text.trim().is_empty()
                            && command.execute(&text, editor, &mut state)
                        {
                            return Ok(());
                        }
                        continue;
                    }
                    command.key(key);
                    continue;
                }
                command.message = None;
                if let KeyCode::Char(c @ '1'..='9') = key.code {
                    pending_count = pending_count
                        .saturating_mul(10)
                        .saturating_add((c as u8 - b'0') as usize);
                    continue;
                }
                if key.code == KeyCode::Char('d') {
                    if pending_delete {
                        let count = pending_count.max(1);
                        if let Err(error) = state.delete_lines(editor, count) {
                            command.message = Some(format!("? {error}"));
                        }
                        pending_count = 0;
                        pending_delete = false;
                    } else {
                        pending_delete = true;
                    }
                    continue;
                }
                if pending_delete {
                    pending_delete = false;
                    pending_count = 0;
                }
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    if let KeyCode::Char(mode @ ('i' | 'a' | 'o' | 'I' | 'A' | 'O')) = key.code {
                        insert_mode(editor, &mut state, mode, &mut width, &mut height)?;
                        continue;
                    }
                    if key.code == KeyCode::Char('u') {
                        command.execute("u", editor, &mut state);
                        continue;
                    }
                    if key.code == KeyCode::Char('x') {
                        if let Err(error) = state.delete_character(editor) {
                            command.message = Some(format!("? {error}"));
                        }
                        continue;
                    }
                }

                if key.code == KeyCode::Char(':') {
                    command.input = Some(String::new());
                    command.search_direction = None;
                    continue;
                }
                if key.code == KeyCode::Char('/') || key.code == KeyCode::Char('?') {
                    command.input = Some(String::new());
                    command.search_direction = Some(key.code == KeyCode::Char('/'));
                    continue;
                }
                if key.code == KeyCode::Char('n') || key.code == KeyCode::Char('N') {
                    if let Err(error) = state.repeat_search(editor, key.code == KeyCode::Char('N'))
                    {
                        command.message = Some(format!("? {error}"));
                    }
                    continue;
                }
                if key.code == KeyCode::Char('q')
                    || (key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c'))
                {
                    if command.execute("q", editor, &mut state) {
                        return Ok(());
                    }
                    continue;
                }
                let motion = match key.code {
                    KeyCode::Char('h') | KeyCode::Left => Some(Motion::Left),
                    KeyCode::Char('j') | KeyCode::Down => Some(Motion::Down),
                    KeyCode::Char('k') | KeyCode::Up => Some(Motion::Up),
                    KeyCode::Char('l') | KeyCode::Right => Some(Motion::Right),
                    KeyCode::Char('0') | KeyCode::Home => Some(Motion::Home),
                    KeyCode::Char('$') | KeyCode::End => Some(Motion::End),
                    KeyCode::Char('g') => Some(Motion::First),
                    KeyCode::Char('G') => Some(Motion::Last),
                    KeyCode::PageUp => Some(Motion::PageUp),
                    KeyCode::PageDown => Some(Motion::PageDown),
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Motion::PageDown)
                    }
                    KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Motion::PageUp)
                    }
                    _ => None,
                };
                if let Some(motion) = motion {
                    state.move_cursor(motion, editor.buffer(), height);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_keys_treat_commands_as_text_and_commit_on_escape_or_control_c() {
        for exit in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut editor = Editor::default();
            let mut edit = editor
                .begin_text_edit(crate::buffer::Position::default())
                .unwrap();
            for c in "q:中".chars() {
                assert!(
                    !insert_key(
                        &mut edit,
                        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
                    )
                    .unwrap()
                );
            }
            insert_key(&mut edit, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)).unwrap();
            insert_key(&mut edit, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
            insert_key(
                &mut edit,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            )
            .unwrap();
            insert_key(&mut edit, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)).unwrap();
            insert_key(
                &mut edit,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
            )
            .unwrap();
            assert!(insert_key(&mut edit, exit).unwrap());
            edit.commit().unwrap();
            assert_eq!(editor.buffer().to_text(), "q:中\t\n");
            editor.undo().unwrap();
            assert!(editor.buffer().is_empty());
        }
    }

    #[test]
    fn command_input_cancel_unicode_and_narrow_display() {
        let mut command = CommandLine {
            input: Some(String::new()),
            message: None,
            search_direction: None,
        };
        for c in "w 中文".chars() {
            command.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        command.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(command.input.as_deref(), Some("w 中"));
        for width in 0..10 {
            let (text, cursor) = command.display(width).unwrap();
            assert!(
                unicode_width::UnicodeWidthStr::width(text.as_str())
                    <= usize::from(width.saturating_sub(1))
            );
            if let Some(x) = cursor {
                assert!(x < width - 1);
            }
        }
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            command.input = Some("w".into());
            command.key(key);
            assert!(command.input.is_none());
        }
    }

    #[test]
    fn commands_protect_edits_recover_after_failed_save_and_reject_other_ex_commands() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saved");
        let mut editor = Editor::default();
        editor.insert_lines(0, ["changed"]).unwrap();
        let before = editor.clone();
        let mut state = VisualState::default();
        let mut command = CommandLine::default();
        for text in ["q", "w", "wq", "a", "i", "c", "p", "w!", "1q"] {
            assert!(!command.execute(text, &mut editor, &mut state), "{text}");
            assert!(command.message.as_ref().unwrap().starts_with("? "));
            assert_eq!(editor, before);
        }
        assert!(command.execute("q!", &mut editor, &mut state));
        assert_eq!(editor, before);
        assert!(!command.execute(&format!("w {}", path.display()), &mut editor, &mut state));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\n");
        assert!(!editor.is_modified());
        assert!(command.execute("q", &mut editor, &mut state));
        editor.insert_lines(1, ["again"]).unwrap();
        assert!(command.execute("wq", &mut editor, &mut state));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\nagain\n");
    }

    #[test]
    #[ignore = "requires a PTY; run via tests/visual_pty.py"]
    fn restores_raw_mode_after_error_and_panic() {
        assert!(!terminal::is_raw_mode_enabled().unwrap());
        let error = (|| -> io::Result<()> {
            let _guard = TerminalGuard::enter()?;
            assert!(terminal::is_raw_mode_enabled()?);
            Err(io::Error::other("injected rendering error"))
        })();
        assert!(error.is_err());
        assert!(!terminal::is_raw_mode_enabled().unwrap());
        let panic = std::panic::catch_unwind(|| {
            let _guard = TerminalGuard::enter().unwrap();
            panic!("injected frontend panic");
        });
        assert!(panic.is_err());
        assert!(!terminal::is_raw_mode_enabled().unwrap());
    }
}
