use super::input::InputSession;
use super::input_keys::{InputKeys, ReplaceKey, ReplaceKeys};
use super::keys::{NormalPrefix, PrefixAction, normal_motion};
use super::macro_input::MacroInput;
use super::operator::Operator;
use super::paste::normalize;
use super::{InputStart, VisualState, clip, screen_column};
use crate::{editor::Editor, recovery::Recovery};
use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
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
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Also runs when a read/write error or unwinding panic leaves the loop.
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            cursor::Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Default)]
struct CommandLine {
    input: Option<String>,
    message: Option<String>,
    search_direction: Option<bool>,
    input_error: Option<String>,
    failed: bool,
}

/// Prefixes and one-character replacement share cancellation, but not decoding.
#[derive(Default)]
struct NormalInput {
    prefix: NormalPrefix,
    replacement: Option<ReplaceKeys>,
    last_macro: Option<char>,
}

impl NormalInput {
    /// A paste cancels an unfinished command instead of supplying its argument.
    fn cancel(&mut self) -> bool {
        let pending = self.replacement.is_some() || self.prefix.is_pending();
        let last_macro = self.last_macro;
        *self = Self {
            last_macro,
            ..Self::default()
        };
        pending
    }

    fn take_register(&mut self) -> Option<char> {
        self.prefix.take_register()
    }

    fn macro_register(&mut self, name: char) -> Option<char> {
        if name == '@' {
            self.last_macro
        } else {
            let name = name.to_ascii_lowercase();
            self.last_macro = Some(name);
            Some(name)
        }
    }

    fn key(
        &mut self,
        key: KeyEvent,
        editor: &mut Editor,
        state: &mut VisualState,
        command: &mut CommandLine,
    ) -> Result<PrefixAction, String> {
        let Some(mut pending) = self.replacement.take() else {
            if !self.prefix.is_pending() {
                editor.finish_macro_command();
            }
            let had_operator = self.prefix.has_operator();
            let action = self.prefix.key(key).map_err(str::to_owned);
            if !had_operator && self.prefix.has_operator()
                || matches!(
                    action,
                    Ok(PrefixAction::Replace(_)
                        | PrefixAction::Insert(..)
                        | PrefixAction::DeleteCharacters(_))
                )
                || matches!(action, Ok(PrefixAction::Join(_)))
                    && state.row + 1 < editor.buffer().line_count()
            {
                editor.enter_macro_command();
            }
            return action;
        };
        match pending.key(key) {
            Ok(ReplaceKey::Pending) => {
                if pending.is_quoted() {
                    command.message = Some("[VIEW] quote: type one replacement character".into());
                }
                self.replacement = Some(pending);
            }
            Ok(ReplaceKey::Cancel) => {}
            Ok(ReplaceKey::Character(character)) => {
                state.replace_counted(editor, character, pending.count)?;
            }
            Err(error) => {
                if pending.is_quoted() {
                    self.replacement = Some(pending);
                }
                return Err(error);
            }
        }
        // The key belongs to r even when it cancels or completes replacement.
        Ok(PrefixAction::Pending)
    }
}

fn editor_title(editor: &Editor) -> String {
    editor
        .path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "[No file]".into())
}

fn editor_name(editor: &Editor) -> String {
    let name = editor_title(editor);
    if editor.is_modified() {
        format!("[+] {name}")
    } else {
        name
    }
}

fn draw_frame<W: Write>(output: &mut W, frame: &super::Frame, height: u16) -> io::Result<()> {
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
        queue!(output, cursor::MoveTo(0, height - 1), Print(&frame.status))?;
    }
    if let Some((x, y)) = frame.cursor {
        queue!(output, cursor::MoveTo(x, y), cursor::Show)?;
    }
    output.flush()
}

impl CommandLine {
    fn error(&mut self, error: impl std::fmt::Display) {
        self.message = Some(format!("? {error}"));
        self.failed = true;
    }

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
                self.error(error);
                false
            }
        }
    }

    // Called only while command input is active; editing keys never move the file cursor.
    fn key(&mut self, key: KeyEvent) {
        self.input_error = None;
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
                // Enter is handled by handle_event so searches can update VisualState.
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

    /// Handle one key while command or search input is active.
    /// Returns true when the command requests leaving visual mode.
    fn handle_event(
        &mut self,
        key: KeyEvent,
        editor: &mut Editor,
        state: &mut VisualState,
    ) -> bool {
        self.input_error = None;
        if key.code != KeyCode::Enter {
            self.key(key);
            return false;
        }
        let text = self.input.take().unwrap();
        if let Some(forward) = self.search_direction {
            if let Err(error) = state.search(editor, &text, forward) {
                self.error(error);
            }
        } else if !text.trim().is_empty() && self.execute(&text, editor, state) {
            return true;
        }
        false
    }

    fn paste(&mut self, text: &str) {
        let result = normalize(text).and_then(|text| {
            if text.chars().any(|c| c.is_control() && c != '\t') {
                return Err("command-line paste must be a single line without control keys".into());
            }
            self.input.as_mut().unwrap().push_str(&text);
            Ok(())
        });
        self.input_error = result.err();
    }

    fn display(&self, width: u16) -> Option<(String, Option<u16>)> {
        let columns = usize::from(width.saturating_sub(1));
        if let Some(error) = &self.input_error {
            return Some((clip(&format!("? {error}"), 0, columns), None));
        }
        if let Some(input) = &self.input {
            return Some(self.display_input(input, columns));
        }
        self.message
            .as_ref()
            .map(|text| (clip(text, 0, columns), None))
    }

    fn display_input(&self, input: &str, columns: usize) -> (String, Option<u16>) {
        let prefix = self
            .search_direction
            .map_or(':', |forward| if forward { '/' } else { '?' });
        let text = format!("{prefix}{input}");
        let end = screen_column(&text, text.chars().count());
        let left = end.saturating_sub(columns.saturating_sub(1));
        (
            clip(&text, left, columns),
            (columns > 0).then_some((end - left) as u16),
        )
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
    request: (char, usize, Option<char>),
    width: &mut u16,
    height: &mut u16,
    recovery: &mut Recovery,
    macros: &mut MacroInput,
) -> io::Result<Option<String>> {
    let name = editor_title(editor);
    let (mode, count, register) = request;
    let session = match state.begin_counted_insert_register(editor, mode, count, register) {
        Ok(session) => session,
        Err(error) => return Ok(Some(error)),
    };
    input_session(session, state, &name, width, height, recovery, macros)?;
    Ok(None)
}

fn input_session(
    mut edit: InputSession<'_>,
    state: &mut VisualState,
    name: &str,
    width: &mut u16,
    height: &mut u16,
    recovery: &mut Recovery,
    macros: &mut MacroInput,
) -> io::Result<()> {
    let mode = edit.mode();
    let mut input_error = None;
    let mut keys = InputKeys::default();
    let result = (|| -> io::Result<()> {
        loop {
            let position = edit.cursor();
            state.row = position.row;
            state.column = position.column;
            let recovery_error = recovery
                .checkpoint(
                    edit.original().path(),
                    edit.buffer(),
                    edit.original().is_modified() || edit.has_changes(),
                )
                .err();
            let mut frame = if mode == 'R' {
                state.replace_frame(edit.buffer(), name, *width, *height)
            } else {
                state.insert_frame(edit.buffer(), name, *width, *height)
            };
            if keys.is_quoted() {
                frame.status = clip(
                    &format!(
                        "[{}] quote: type one character",
                        if mode == 'R' { "REPLACE" } else { "INSERT" }
                    ),
                    0,
                    usize::from(width.saturating_sub(1)),
                );
            }
            if let Some(error) = recovery_error {
                frame.status = clip(
                    &format!(
                        "[{}] ? recovery unavailable: {error}",
                        if mode == 'R' { "REPLACE" } else { "INSERT" }
                    ),
                    0,
                    usize::from(width.saturating_sub(1)),
                );
            }
            if let Some(error) = &input_error {
                frame.status = clip(
                    &format!("? {error}"),
                    0,
                    usize::from(width.saturating_sub(1)),
                );
            }
            let mut output = io::stdout().lock();
            draw_frame(&mut output, &frame, *height)?;
            drop(output);
            match next_event(*width, *height, macros)? {
                Event::Resize(w, h) => {
                    *width = w;
                    *height = h;
                }
                Event::Paste(text) => {
                    input_error = if keys.cancel_quote() {
                        Some("quoted input cancelled; nothing pasted".into())
                    } else {
                        edit.paste(&text).err()
                    };
                }
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    input_error = None;
                    let done = keys.key(&mut edit, mode == 'R', key);
                    match done {
                        Ok(true) => return Ok(()),
                        Ok(false) => {}
                        Err(error) => {
                            macros.clear();
                            input_error = Some(error);
                        }
                    }
                }
                _ => {}
            }
        }
    })();
    // Retain typed text in the editor even when terminal I/O fails.
    if result.is_ok() {
        edit.commit(state)
    } else {
        edit.interrupt(state)
    }
    .map_err(io::Error::other)?;
    result
}

pub fn run(editor: &mut Editor) -> io::Result<()> {
    let mut recovery = Recovery::for_editor(editor)?;
    let result = run_session(editor, &mut recovery);
    editor.end_macro_undo();
    result?;
    recovery.finish()
}

fn next_event(width: u16, height: u16, macros: &mut MacroInput) -> io::Result<Event> {
    macros
        .pop()
        .map(Ok)
        .unwrap_or_else(|| read_event(width, height))
}

fn run_session(editor: &mut Editor, recovery: &mut Recovery) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "visual mode requires a terminal on stdin and stdout",
        ));
    }
    let _guard = TerminalGuard::enter()?;
    let mut state = VisualState::default();
    let mut command = CommandLine::default();
    let mut normal = NormalInput::default();
    let mut macros = MacroInput::default();
    let (mut width, mut height) = terminal::size()?;
    loop {
        // ex_voper.c errlab discards the pending macro after a failed motion.
        if std::mem::take(&mut command.failed) {
            macros.clear();
        }
        // An input transaction can consume the end of the stream while it
        // borrows Editor. Finish its edit, then close the invocation here.
        if !macros.is_active() {
            editor.end_macro_undo();
        }
        if let Err(error) = recovery.observe(editor) {
            command.message = Some(format!("? recovery unavailable: {error}"));
        }
        let name = editor_name(editor);
        let mut frame = state.frame(editor.buffer(), &name, width, height);
        if !editor.buffer().is_empty() {
            editor
                .set_current(state.row)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        state.checkpoint_line(editor);
        if let Some((status, column)) = command.display(width) {
            frame.status = status;
            if command.input.is_some() {
                frame.cursor = column.filter(|_| height > 0).map(|x| (x, height - 1));
            }
        }
        let mut output = io::stdout().lock();
        draw_frame(&mut output, &frame, height)?;
        drop(output);
        // Some terminal environments do not deliver resize events reliably.
        // Check dimensions while idle as well, including during command input.
        let event = next_event(width, height, &mut macros)?;
        if !macros.is_active() {
            editor.end_macro_undo();
        }
        match event {
            Event::Resize(w, h) => {
                width = w;
                height = h;
            }
            Event::Paste(text) => {
                if command.input.is_some() {
                    command.paste(&text);
                } else {
                    let pending = normal.cancel();
                    command.message = if pending {
                        Some("? paste cancelled pending command; nothing pasted".into())
                    } else {
                        state
                            .paste(editor, &text)
                            .err()
                            .map(|error| format!("? {error}"))
                    };
                }
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if command.input.is_some() {
                    if command.handle_event(key, editor, &mut state) {
                        return Ok(());
                    }
                    continue;
                }
                command.message = None;
                let action = normal.key(key, editor, &mut state, &mut command);
                let selected_register = normal.take_register();
                let action = match action {
                    Ok(PrefixAction::Find {
                        forward,
                        till,
                        character,
                        count: _,
                        operator: Some((operator, operator_count)),
                    }) => state
                        .prepare_find_motion(
                            editor,
                            forward,
                            till,
                            character,
                            operator_count.unwrap_or(1),
                        )
                        .map(|motion| {
                            PrefixAction::Operate(operator, Some(motion), operator_count)
                        }),
                    Ok(PrefixAction::RepeatFind {
                        reverse,
                        count: _,
                        operator: Some((operator, operator_count)),
                    }) => state
                        .prepare_repeat_find_motion(editor, reverse, operator_count.unwrap_or(1))
                        .map(|motion| {
                            PrefixAction::Operate(operator, Some(motion), operator_count)
                        }),
                    other => other,
                };
                match action {
                    Ok(PrefixAction::Pending) => continue,
                    Ok(PrefixAction::Insert(mode, count)) => {
                        if let Some(error) = insert_mode(
                            editor,
                            &mut state,
                            (mode, count, selected_register),
                            &mut width,
                            &mut height,
                            recovery,
                            &mut macros,
                        )? {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Replace(count)) => {
                        match state.replacement_end(editor, count) {
                            Ok(_) => normal.replacement = Some(ReplaceKeys::new(count)),
                            Err(error) => command.error(error),
                        }
                        continue;
                    }
                    Ok(PrefixAction::DeleteCharacters(count)) => {
                        if let Err(error) =
                            state.delete_characters(editor, count, selected_register)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Repeat(count)) => {
                        if let Err(error) =
                            state.repeat_change_register(editor, count, selected_register)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Macro(name)) => {
                        let Some(name) = normal.macro_register(name) else {
                            normal.cancel();
                            command.error("no previous macro");
                            continue;
                        };
                        if let Err(error) = macros.expand(editor, name) {
                            macros.clear();
                            normal.cancel();
                            normal.last_macro = None;
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Find {
                        forward,
                        till,
                        character,
                        count,
                        operator: None,
                    }) => {
                        if let Err(error) =
                            state.find_character(editor, forward, till, character, count)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::RepeatFind {
                        reverse,
                        count,
                        operator: None,
                    }) => {
                        if let Err(error) = state.repeat_find(editor, reverse, count) {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Find {
                        operator: Some(_), ..
                    })
                    | Ok(PrefixAction::RepeatFind {
                        operator: Some(_), ..
                    }) => {
                        command.error("invalid character search");
                        continue;
                    }
                    Ok(PrefixAction::Operate(Operator::Yank, motion, count)) => {
                        if let Err(error) =
                            state.yank_register(editor, motion, count, selected_register)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Put(before, count)) => {
                        // ex_vmain rejects an empty unnamed put before
                        // vmacchng; an explicit empty register fails later.
                        if selected_register.is_some() || editor.unnamed_register().is_some() {
                            editor.enter_macro_command();
                        }
                        if let Err(error) =
                            state.put_selected(editor, before, count, selected_register)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Operate(Operator::Change, motion, count)) => {
                        let name = editor_title(editor);
                        match state.begin_change_register(editor, motion, count, selected_register)
                        {
                            Ok(edit) => input_session(
                                InputSession::new(
                                    edit,
                                    match selected_register {
                                        Some(register) => InputStart::ChangeNamed {
                                            motion,
                                            count,
                                            register,
                                        },
                                        None => InputStart::Change { motion, count },
                                    },
                                    1,
                                )
                                .map_err(io::Error::other)?,
                                &mut state,
                                &name,
                                &mut width,
                                &mut height,
                                recovery,
                                &mut macros,
                            )?,
                            Err(error) => command.error(error),
                        }
                        continue;
                    }
                    Ok(PrefixAction::Operate(Operator::Delete, Some(motion), count)) => {
                        if let Err(error) =
                            state.delete_motion_register(editor, motion, count, selected_register)
                        {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Move(motion, count)) => {
                        state.move_counted(motion, Some(count), editor.buffer(), height);
                        continue;
                    }
                    Ok(PrefixAction::Join(count)) => {
                        if let Err(error) = state.join_lines(editor, count) {
                            command.error(error);
                        }
                        continue;
                    }
                    Ok(PrefixAction::Operate(Operator::Delete, None, count)) => {
                        if let Err(error) = state.delete_lines_register(
                            editor,
                            count.unwrap_or(1),
                            selected_register,
                        ) {
                            command.error(error);
                        }
                        continue;
                    }
                    Err(error) => {
                        command.error(error);
                        continue;
                    }
                    Ok(PrefixAction::Pass) => {}
                }
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    if key.code == KeyCode::Char('U') {
                        if let Err(error) = state.restore_line(editor) {
                            command.error(error);
                        }
                        continue;
                    }
                    if key.code == KeyCode::Char('u') {
                        if let Err(error) = state.undo(editor) {
                            command.error(error);
                        }
                        continue;
                    }
                }

                if key.code == KeyCode::Char(':') {
                    state.sync_line_undo();
                    command.input = Some(String::new());
                    command.search_direction = None;
                    continue;
                }
                if key.code == KeyCode::Char('/') || key.code == KeyCode::Char('?') {
                    state.sync_line_undo();
                    command.input = Some(String::new());
                    command.search_direction = Some(key.code == KeyCode::Char('/'));
                    continue;
                }
                if key.code == KeyCode::Char('n') || key.code == KeyCode::Char('N') {
                    if let Err(error) = state.repeat_search(editor, key.code == KeyCode::Char('N'))
                    {
                        command.error(error);
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
                let motion = normal_motion(key);
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
    fn invalid_join_does_not_promote_macro_undo_before_validation() {
        let mut editor = Editor::default();
        editor.insert_lines(0, ["abc"]).unwrap();
        editor.begin_macro_undo();
        let mut state = VisualState::default();
        state.delete_character(&mut editor).unwrap();
        let before = editor.clone();
        let action = NormalInput::default()
            .key(
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
                &mut editor,
                &mut state,
                &mut CommandLine::default(),
            )
            .unwrap();
        assert_eq!(action, PrefixAction::Join(2));
        assert_eq!(editor, before);
        assert!(state.join_lines(&mut editor, 2).is_err());
        assert_eq!(editor, before);
    }

    #[test]
    fn replacement_retry_completion_and_paste_cancel_do_not_leak_prefixes() {
        let mut editor = Editor::default();
        editor.insert_lines(0, ["abc"]).unwrap();
        let mut state = VisualState::default();
        let mut command = CommandLine::default();
        let mut normal = NormalInput {
            replacement: Some(ReplaceKeys::new(2)),
            ..NormalInput::default()
        };
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        normal
            .key(
                KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
                &mut editor,
                &mut state,
                &mut command,
            )
            .unwrap();
        assert!(
            normal
                .key(key(KeyCode::Left), &mut editor, &mut state, &mut command)
                .is_err()
        );
        assert_eq!(editor.buffer().line(0), Some("abc"));
        assert_eq!(
            normal
                .key(
                    key(KeyCode::Char('x')),
                    &mut editor,
                    &mut state,
                    &mut command
                )
                .unwrap(),
            PrefixAction::Pending
        );
        assert_eq!(editor.buffer().line(0), Some("xxc"));
        assert!(!normal.cancel());
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().line(0), Some("abc"));

        normal.replacement = Some(ReplaceKeys::new(1));
        assert!(
            normal
                .key(key(KeyCode::Left), &mut editor, &mut state, &mut command)
                .is_err()
        );
        assert!(!normal.cancel());
        normal.replacement = Some(ReplaceKeys::new(1));
        assert!(normal.cancel());
        normal
            .key(
                key(KeyCode::Char('2')),
                &mut editor,
                &mut state,
                &mut command,
            )
            .unwrap();
        normal
            .key(
                key(KeyCode::Char('d')),
                &mut editor,
                &mut state,
                &mut command,
            )
            .unwrap();
        assert!(normal.cancel());
        assert_eq!(
            normal
                .key(
                    key(KeyCode::Char('x')),
                    &mut editor,
                    &mut state,
                    &mut command
                )
                .unwrap(),
            PrefixAction::DeleteCharacters(Some(1))
        );
        assert_eq!(editor.buffer().line(0), Some("abc"));
    }

    #[test]
    fn command_paste_is_literal_single_line_and_requires_enter() {
        for search_direction in [None, Some(true), Some(false)] {
            let mut command = CommandLine {
                input: Some("prefix".into()),
                search_direction,
                ..CommandLine::default()
            };
            command.paste("中:quit");
            assert_eq!(command.input.as_deref(), Some("prefix中:quit"));
            for invalid in ["\r\nq!", "\x1b:q!", "\0", "\x03"] {
                command.paste(invalid);
                assert_eq!(command.input.as_deref(), Some("prefix中:quit"));
                assert!(command.display(120).unwrap().0.contains("single line"));
            }
            command.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
            assert!(command.input_error.is_none());
            assert_eq!(command.input.as_deref(), Some("prefix中:quitx"));
        }
    }

    #[test]
    fn command_input_cancel_unicode_and_narrow_display() {
        let mut command = CommandLine {
            input: Some(String::new()),
            message: None,
            search_direction: None,
            input_error: None,
            failed: false,
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
