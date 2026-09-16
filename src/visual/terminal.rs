use super::{Motion, VisualState};
use crate::editor::Editor;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
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

pub fn run(editor: &mut Editor) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "visual mode requires a terminal on stdin and stdout",
        ));
    }
    let _guard = TerminalGuard::enter()?;
    let mut state = VisualState::default();
    let name = editor
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "[No file]".into());
    let (mut width, mut height) = terminal::size()?;
    loop {
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
            queue!(output, cursor::MoveTo(0, height - 1), Print(frame.status))?;
        }
        if let Some((x, y)) = frame.cursor {
            queue!(output, cursor::MoveTo(x, y), cursor::Show)?;
        }
        output.flush()?;
        drop(output);
        match event::read()? {
            Event::Resize(w, h) => {
                width = w;
                height = h;
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.code == KeyCode::Char('q')
                    || (key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c'))
                {
                    return Ok(());
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
