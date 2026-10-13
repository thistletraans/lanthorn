//! The Journal's per-story Notes section and its small inline editor.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

use crate::state::AppState;

const UNDO_HISTORY_LIMIT: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditKind {
    Insert,
    Backspace,
    Delete,
    Atomic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditSnapshot {
    text: String,
    cursor: usize,
    selection: Option<usize>,
}

fn snapshot(state: &AppState) -> EditSnapshot {
    EditSnapshot {
        text: state.journal_notes.clone(),
        cursor: state.journal_notes_cursor,
        selection: state.journal_notes_selection,
    }
}

fn restore(state: &mut AppState, snapshot: EditSnapshot) {
    state.journal_notes = snapshot.text;
    state.journal_notes_cursor = snapshot.cursor;
    state.journal_notes_selection = snapshot.selection;
}

fn push_history(history: &mut Vec<EditSnapshot>, snapshot: EditSnapshot) {
    if history.len() >= UNDO_HISTORY_LIMIT {
        history.remove(0);
    }
    history.push(snapshot);
}

fn edit(state: &mut AppState, kind: EditKind, change: impl FnOnce(&mut AppState)) {
    let before = snapshot(state);
    change(state);
    if state.journal_notes != before.text {
        if kind == EditKind::Atomic || state.journal_notes_last_edit != Some(kind) {
            push_history(&mut state.journal_notes_undo, before);
        }
        state.journal_notes_redo.clear();
        state.journal_notes_last_edit = (kind != EditKind::Atomic).then_some(kind);
    }
}

fn undo(state: &mut AppState) {
    state.journal_notes_last_edit = None;
    if let Some(previous) = state.journal_notes_undo.pop() {
        let current = snapshot(state);
        push_history(&mut state.journal_notes_redo, current);
        restore(state, previous);
    }
}

fn redo(state: &mut AppState) {
    state.journal_notes_last_edit = None;
    if let Some(next) = state.journal_notes_redo.pop() {
        let current = snapshot(state);
        push_history(&mut state.journal_notes_undo, current);
        restore(state, next);
    }
}

fn byte_at(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(at, _)| at)
}

/// Wrap physical note lines into screen-width rows while retaining their
/// document character offsets for selection and caret placement.
fn wrapped_lines(text: &str, width: u16) -> Vec<(usize, usize, String)> {
    let physical: Vec<&str> = text.split('\n').collect();
    let mut rows = Vec::new();
    let mut offset = 0usize;
    for (line_index, line) in physical.iter().enumerate() {
        for (wrapped, start, end) in crate::render::transcript::wrap_line_ranges(line, width.max(1))
        {
            rows.push((offset + start, offset + end, wrapped));
        }
        offset += line.chars().count();
        if line_index + 1 < physical.len() {
            offset += 1; // the newline is a caret position in the document
        }
    }
    rows
}

fn selection(state: &AppState) -> Option<std::ops::Range<usize>> {
    state
        .journal_notes_selection
        .map(|anchor| {
            anchor.min(state.journal_notes_cursor)..anchor.max(state.journal_notes_cursor)
        })
        .filter(|range| range.start != range.end)
}

/// Return selected note text in Unicode scalar indices, matching the editor's
/// caret and selection representation.
pub fn copy_selection(state: &AppState) -> Option<String> {
    let range = selection(state)?;
    let start = byte_at(&state.journal_notes, range.start);
    let end = byte_at(&state.journal_notes, range.end);
    Some(state.journal_notes[start..end].to_string())
}

pub fn cut_selection(state: &mut AppState) -> Option<String> {
    let copied = copy_selection(state)?;
    replace_selection(state, "");
    Some(copied)
}

fn replace_selection(state: &mut AppState, insert: &str) {
    edit(state, EditKind::Atomic, |state| {
        replace_selection_inner(state, insert)
    });
}

fn replace_selection_inner(state: &mut AppState, insert: &str) {
    let range = selection(state);
    if let Some(range) = range {
        let start = byte_at(&state.journal_notes, range.start);
        let end = byte_at(&state.journal_notes, range.end);
        state.journal_notes.replace_range(start..end, insert);
        state.journal_notes_cursor = range.start + insert.chars().count();
        state.journal_notes_selection = None;
    } else {
        let at = byte_at(&state.journal_notes, state.journal_notes_cursor);
        state.journal_notes.insert_str(at, insert);
        state.journal_notes_cursor += insert.chars().count();
    }
}

fn move_caret(state: &mut AppState, target: usize, extend: bool) {
    if extend {
        state
            .journal_notes_selection
            .get_or_insert(state.journal_notes_cursor);
    } else {
        state.journal_notes_selection = None;
    }
    state.journal_notes_cursor = target.min(state.journal_notes.chars().count());
}

/// Start editing the currently displayed Notes tab.
pub(crate) fn begin_edit(state: &mut AppState) {
    state.journal_notes_cursor = state.journal_notes.chars().count();
    state.journal_notes_selection = None;
    state.journal_notes_undo.clear();
    state.journal_notes_redo.clear();
    state.journal_notes_last_edit = None;
    state.journal_notes_editing = true;
    state.focus = crate::state::Focus::Map;
}

/// Save and leave the Notes editor, returning keyboard focus to the game.
pub fn finish_edit(state: &mut AppState) {
    if !state.journal_notes_editing {
        return;
    }
    if let Err(error) =
        crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes)
    {
        state.set_status(format!("could not save journal notes: {error}"));
    }
    state.journal_notes_editing = false;
    state.journal_notes_selection = None;
    state.focus = crate::state::Focus::Game;
}

/// Handle a key owned by the Notes editor. Escape saves and returns to the game.
pub(crate) fn handle_key(state: &mut AppState, key: KeyEvent) {
    let merge_kind = if !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        match key.code {
            KeyCode::Char(c) if c.is_alphanumeric() => Some(EditKind::Insert),
            KeyCode::Backspace => Some(EditKind::Backspace),
            KeyCode::Delete => Some(EditKind::Delete),
            _ => None,
        }
    } else {
        None
    };
    if merge_kind.is_none() {
        state.journal_notes_last_edit = None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A'))
    {
        let len = state.journal_notes.chars().count();
        state.journal_notes_selection = (len > 0).then_some(0);
        state.journal_notes_cursor = len;
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
    {
        state.set_status(
            match crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes)
            {
                Ok(()) => "journal notes saved".to_string(),
                Err(error) => format!("could not save journal notes: {error}"),
            },
        );
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z'))
    {
        undo(state);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'))
    {
        redo(state);
        return;
    }
    if key.code == KeyCode::Esc {
        finish_edit(state);
        return;
    }
    let extend = key.modifiers.contains(KeyModifiers::SHIFT);
    let len = state.journal_notes.chars().count();
    let cursor = state.journal_notes_cursor.min(len);
    match key.code {
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut buf = [0; 4];
            let kind = if selection(state).is_some() || !c.is_alphanumeric() {
                EditKind::Atomic
            } else {
                EditKind::Insert
            };
            edit(state, kind, |state| {
                replace_selection_inner(state, c.encode_utf8(&mut buf))
            });
        }
        KeyCode::Enter => replace_selection(state, "\n"),
        KeyCode::Left if cursor > 0 => move_caret(state, cursor - 1, extend),
        KeyCode::Right if cursor < len => move_caret(state, cursor + 1, extend),
        KeyCode::Home | KeyCode::End => {
            let line_start = state.journal_notes[..byte_at(&state.journal_notes, cursor)]
                .rfind('\n')
                .map_or(0, |i| state.journal_notes[..=i].chars().count());
            let target = if key.code == KeyCode::Home {
                line_start
            } else {
                line_start
                    + state.journal_notes[byte_at(&state.journal_notes, line_start)..]
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .count()
            };
            move_caret(state, target, extend);
        }
        KeyCode::Up | KeyCode::Down => {
            let before = &state.journal_notes[..byte_at(&state.journal_notes, cursor)];
            let current_start = before
                .rfind('\n')
                .map_or(0, |i| before[..=i].chars().count());
            let col = cursor - current_start;
            let lines: Vec<&str> = state.journal_notes.split('\n').collect();
            let line = state.journal_notes[..byte_at(&state.journal_notes, cursor)]
                .matches('\n')
                .count();
            let target_line = if key.code == KeyCode::Up {
                line.saturating_sub(1)
            } else {
                (line + 1).min(lines.len() - 1)
            };
            let target_start: usize = lines[..target_line]
                .iter()
                .map(|s| s.chars().count() + 1)
                .sum();
            move_caret(
                state,
                target_start + col.min(lines[target_line].chars().count()),
                extend,
            );
        }
        KeyCode::Backspace => {
            let kind = if selection(state).is_some() {
                EditKind::Atomic
            } else {
                EditKind::Backspace
            };
            edit(state, kind, |state| {
                if selection(state).is_some() {
                    replace_selection_inner(state, "");
                } else if cursor > 0 {
                    let start = byte_at(&state.journal_notes, cursor - 1);
                    let end = byte_at(&state.journal_notes, cursor);
                    state.journal_notes.replace_range(start..end, "");
                    state.journal_notes_cursor -= 1;
                }
            });
        }
        KeyCode::Delete => {
            let kind = if selection(state).is_some() {
                EditKind::Atomic
            } else {
                EditKind::Delete
            };
            edit(state, kind, |state| {
                if selection(state).is_some() {
                    replace_selection_inner(state, "");
                } else if cursor < len {
                    let start = byte_at(&state.journal_notes, cursor);
                    let end = byte_at(&state.journal_notes, cursor + 1);
                    state.journal_notes.replace_range(start..end, "");
                }
            });
        }
        _ => {}
    }
}

/// Insert terminal bracketed-paste text at the caret, replacing any selection.
/// Normalize line endings and omit non-printing controls other than tabs/newlines.
pub fn paste(state: &mut AppState, text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let clean: String = normalized
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect();
    if clean.is_empty() {
        return;
    }
    replace_selection(state, &clean);
}

pub fn draw(state: &AppState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let area = crate::journal::frame_body(buf, area, "Notes", &state.colors, false);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let body = state.colors.theme.get("journal.docs.row").style;
    let muted = state.colors.theme.get("muted").style;
    let accent = state.colors.theme.get("accent").style;
    let hint = if state.journal_notes_editing {
        "Ctrl+A all · Ctrl+C copy · Ctrl+X cut · Ctrl+Shift+V paste"
    } else {
        "Shift+Enter or click: edit notes"
    };
    crate::render::draw_str_clipped(buf, area.x, area.y, hint, muted, area);
    if state.journal_notes_editing {
        crate::render::draw_str_clipped(
            buf,
            area.x,
            area.y.saturating_add(1),
            "Enter newline · Ctrl+Z undo · Ctrl+Y redo · Ctrl+S save · Esc exit",
            muted,
            area,
        );
    }
    let content_top = area.y.saturating_add(2);
    let available = area.bottom().saturating_sub(content_top);
    let selected = selection(state);
    let rows = wrapped_lines(&state.journal_notes, area.width);
    let cursor_row = rows
        .iter()
        .enumerate()
        .rfind(|(_, (start, end, _))| {
            *start <= state.journal_notes_cursor && state.journal_notes_cursor <= *end
        })
        .map_or(0, |(row, _)| row);
    let first_row = if state.journal_notes_editing {
        cursor_row
            .saturating_add(1)
            .saturating_sub(available as usize)
    } else {
        0
    };
    for (row, (start, end, line)) in rows
        .iter()
        .enumerate()
        .skip(first_row)
        .take(available as usize)
    {
        let y = content_top + (row - first_row) as u16;
        if state.journal_notes_editing {
            let len = end - start;
            let (prefix_end, sel_end) = selected
                .as_ref()
                .map(|r| (r.start.max(*start).min(*end), r.end.max(*start).min(*end)))
                .filter(|(a, b)| a < b)
                .map(|(a, b)| (a - *start, b - *start))
                .unwrap_or((len, len));
            let prefix: String = line.chars().take(prefix_end).collect();
            let selected_text: String = line
                .chars()
                .skip(prefix_end)
                .take(sel_end - prefix_end)
                .collect();
            let suffix: String = line.chars().skip(sel_end).collect();
            let mut x = area.x;
            crate::render::draw_str_clipped(buf, x, y, &prefix, body, area);
            x = x.saturating_add(crate::textwidth::str_cells(&prefix) as u16);
            if !selected_text.is_empty() {
                crate::render::draw_str_clipped(
                    buf,
                    x,
                    y,
                    &selected_text,
                    body.add_modifier(Modifier::REVERSED),
                    area,
                );
                x = x.saturating_add(crate::textwidth::str_cells(&selected_text) as u16);
            }
            crate::render::draw_str_clipped(buf, x, y, &suffix, body, area);
            if state.journal_notes_cursor >= *start && state.journal_notes_cursor <= *end {
                let caret_col = state.journal_notes_cursor - *start;
                let before: String = line.chars().take(caret_col).collect();
                let cx = area.x.saturating_add(
                    (crate::textwidth::str_cells(&before) as u16).min(area.width.saturating_sub(1)),
                );
                if cx < area.right() {
                    if let Some(cell) = buf.cell_mut((cx, y)) {
                        cell.set_symbol("▏").set_style(accent);
                    }
                }
            }
        } else {
            crate::render::draw_str_clipped(buf, area.x, y, line, body, area);
        }
    }
    if state.journal_notes.is_empty() && !state.journal_notes_editing && available > 0 {
        crate::render::draw_str_clipped(buf, area.x, content_top, "No notes yet.", muted, area);
    }
}

#[cfg(all(test, feature = "t-input"))]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn shift_arrows_select_and_typing_replaces_selection() {
        let mut state = AppState::default();
        let game_dir = crate::scratch_dir("journal-notes-input");
        state.game_dir = game_dir.clone();
        state.journal_notes = "notes".into();
        state.journal_notes_cursor = 2;
        state.journal_notes_editing = true;
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT),
        );
        assert_eq!(state.journal_notes, "noXes");
        assert_eq!(crate::styles::read_per_game_journal_notes(&game_dir), "");
        state.focus = crate::state::Focus::Map;
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.focus, crate::state::Focus::Game);
        assert!(!state.journal_notes_editing);
        assert_eq!(
            crate::styles::read_per_game_journal_notes(&game_dir),
            "noXes"
        );
        let _ = std::fs::remove_dir_all(game_dir);
    }

    #[test]
    fn wraps_before_a_word_instead_of_splitting_it() {
        assert_eq!(
            wrapped_lines("The foobar", 7),
            vec![(0, 3, "The".into()), (4, 10, "foobar".into())]
        );
    }

    #[test]
    fn cut_copies_and_removes_the_selection() {
        let mut state = AppState::default();
        let game_dir = crate::scratch_dir("journal-notes-cut");
        state.game_dir = game_dir.clone();
        state.journal_notes = "take the brass key".into();
        state.journal_notes_cursor = 8;
        state.journal_notes_selection = Some(4);
        state.journal_notes_editing = true;

        assert_eq!(cut_selection(&mut state).as_deref(), Some(" the"));
        assert_eq!(state.journal_notes, "take brass key");
        assert_eq!(state.journal_notes_cursor, 4);
        assert_eq!(crate::styles::read_per_game_journal_notes(&game_dir), "");
        finish_edit(&mut state);
        assert_eq!(
            crate::styles::read_per_game_journal_notes(&game_dir),
            "take brass key"
        );
        let _ = std::fs::remove_dir_all(game_dir);
    }

    #[test]
    fn undo_and_redo_group_adjacent_typing_and_clear_redo_after_a_new_edit() {
        let mut state = AppState::default();
        state.journal_notes = "a".into();
        state.journal_notes_cursor = 1;
        state.journal_notes_editing = true;

        for c in ['b', 'c', 'd'] {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "a", "a run of typing is one undo step");

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "abcd");

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "ax", "a new edit clears redo history");
    }

    #[test]
    fn undo_and_redo_keep_the_thirty_most_recent_edit_actions() {
        let mut state = AppState::default();
        for _ in 0..31 {
            paste(&mut state, "x");
        }
        assert_eq!(state.journal_notes.chars().count(), 31);

        for _ in 0..30 {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            );
        }
        assert_eq!(
            state.journal_notes, "x",
            "history retains the last 30 actions"
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            state.journal_notes, "x",
            "older actions are no longer retained"
        );

        for _ in 0..30 {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            );
        }
        assert_eq!(state.journal_notes.chars().count(), 31);
    }

    #[test]
    fn adjacent_backspaces_group_but_caret_movement_starts_a_new_action() {
        let mut state = AppState::default();
        state.journal_notes = "word".into();
        state.journal_notes_cursor = 4;
        state.journal_notes_editing = true;

        for _ in 0..3 {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            );
        }
        assert_eq!(state.journal_notes, "w");
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "word", "backspaces undo as a group");

        for c in [' ', 'n', 'e', 'w'] {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_key(&mut state, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            state.journal_notes, "word new",
            "caret movement separates adjacent typing into distinct undo actions"
        );
    }

    #[test]
    fn undo_and_redo_restore_selection_and_treat_paste_as_one_edit() {
        let mut state = AppState::default();
        state.journal_notes = "hello".into();
        state.journal_notes_cursor = 5;
        state.journal_notes_selection = Some(0);
        state.journal_notes_editing = true;

        paste(&mut state, "world");
        assert_eq!(state.journal_notes, "world");
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "hello");
        assert_eq!(state.journal_notes_cursor, 5);
        assert_eq!(state.journal_notes_selection, Some(0));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "world");
        assert_eq!(state.journal_notes_cursor, 5);
        assert_eq!(state.journal_notes_selection, None);
    }
}
