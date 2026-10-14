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

fn word_left(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut target = cursor.min(chars.len());
    while target > 0 && chars[target - 1].is_whitespace() {
        target -= 1;
    }
    while target > 0 && !chars[target - 1].is_whitespace() {
        target -= 1;
    }
    target
}

fn word_right(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut target = cursor.min(chars.len());
    while target < chars.len() && !chars[target].is_whitespace() {
        target += 1;
    }
    while target < chars.len() && chars[target].is_whitespace() {
        target += 1;
    }
    target
}

/// Wrap physical note lines into screen-width rows while retaining their
/// document character offsets for selection and caret placement.
fn wrapped_lines(text: &str, width: u16) -> Vec<(usize, usize, String)> {
    let physical: Vec<&str> = text.split('\n').collect();
    let mut rows = Vec::new();
    let mut offset = 0usize;
    for (line_index, line) in physical.iter().enumerate() {
        let wrap_width = width.saturating_sub(1).max(1) as usize;
        let mut remaining: &str = line;
        let mut line_offset = 0usize;
        while !remaining.is_empty() {
            let br = crate::textwidth::row_break(remaining, wrap_width);
            let Some(overflow) = br.overflow else {
                let end = line_offset + remaining.chars().count();
                rows.push((offset + line_offset, offset + end, remaining.to_string()));
                line_offset = end;
                break;
            };
            if br.last_space == Some(overflow)
                && remaining[overflow..].chars().all(char::is_whitespace)
            {
                let head = &remaining[..overflow];
                if !head.is_empty() {
                    let count = head.chars().count();
                    rows.push((
                        offset + line_offset,
                        offset + line_offset + count,
                        head.to_string(),
                    ));
                    line_offset += count;
                }
                remaining = &remaining[overflow..];
                continue;
            }
            if let Some(space) = br.last_space {
                let head = &remaining[..space];
                let count = head.chars().count();
                rows.push((
                    offset + line_offset,
                    offset + line_offset + count,
                    head.to_string(),
                ));
                let next = space + ' '.len_utf8();
                line_offset += count + 1;
                remaining = &remaining[next..];
            } else {
                let mut split = overflow;
                if split == 0 {
                    split = remaining.chars().next().map_or(0, char::len_utf8);
                }
                let head = &remaining[..split];
                let count = head.chars().count();
                rows.push((
                    offset + line_offset,
                    offset + line_offset + count,
                    head.to_string(),
                ));
                line_offset += count;
                remaining = &remaining[split..];
            }
        }
        if line.is_empty() {
            rows.push((offset, offset, String::new()));
        }
        debug_assert_eq!(line_offset, line.chars().count());
        let line_len = line.chars().count();
        offset += line_len;
        if line_index + 1 < physical.len() {
            offset += 1; // the newline is a caret position in the document
        }
    }
    rows
}

fn caret_position(rows: &[(usize, usize, String)], cursor: usize) -> (usize, usize) {
    let Some((row, (start, _, text))) = rows
        .iter()
        .enumerate()
        .rfind(|(_, (start, end, _))| *start <= cursor && cursor <= *end)
    else {
        return (0, 0);
    };
    let before: String = text.chars().take(cursor.saturating_sub(*start)).collect();
    (row, crate::textwidth::str_cells(&before))
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
        state.journal_notes_selection = None;
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

fn content_area(area: Rect, colors: &crate::colors::ColorScheme) -> Rect {
    let mut scratch = Buffer::empty(area);
    crate::journal::frame_body(&mut scratch, area, "Notes", colors, false)
}

pub fn content_width(area: Rect, colors: &crate::colors::ColorScheme) -> u16 {
    content_area(area, colors).width
}

fn text_area(area: Rect, colors: &crate::colors::ColorScheme) -> Rect {
    content_area(area, colors)
}

fn mouse_target(state: &AppState, area: Rect, x: u16, y: u16, clamp: bool) -> Option<usize> {
    let target_area = text_area(area, &state.colors);
    if target_area.width == 0 || target_area.height == 0 {
        return None;
    }
    let position = if clamp {
        ratatui::layout::Position {
            x: x.clamp(target_area.x, target_area.right().saturating_sub(1)),
            y: y.clamp(target_area.y, target_area.bottom().saturating_sub(1)),
        }
    } else {
        ratatui::layout::Position { x, y }
    };
    if !target_area.contains(position) {
        return None;
    }

    let width = target_area.width.max(1);
    let rows = wrapped_lines(&state.journal_notes, width);
    let (cursor_row, _) = caret_position(&rows, state.journal_notes_cursor);
    let first_row = if state.journal_notes_editing {
        cursor_row
            .saturating_add(1)
            .saturating_sub(target_area.height as usize)
    } else {
        0
    };
    let row_index = first_row
        .saturating_add(position.y.saturating_sub(target_area.y) as usize)
        .min(rows.len().saturating_sub(1));
    let (start, _, line) = &rows[row_index];
    let column = position.x.saturating_sub(target_area.x) as usize;
    Some(*start + crate::textwidth::col_to_char_idx(line, column))
}

pub fn set_mouse_cursor(state: &mut AppState, target: usize, extend: bool) {
    state.journal_notes_last_edit = None;
    state.journal_notes_vertical_goal = None;
    if extend {
        state
            .journal_notes_selection
            .get_or_insert(state.journal_notes_cursor);
    } else {
        state.journal_notes_selection = None;
    }
    state.journal_notes_cursor = target.min(state.journal_notes.chars().count());
    state.journal_notes_mouse_selecting = true;
}

pub fn mouse_target_at(state: &AppState, area: Rect, x: u16, y: u16) -> Option<usize> {
    mouse_target(state, area, x, y, false)
}

pub fn mouse_target_drag(state: &AppState, area: Rect, x: u16, y: u16) -> Option<usize> {
    mouse_target(state, area, x, y, true)
}

pub fn finish_mouse_selection(state: &mut AppState) {
    state.journal_notes_mouse_selecting = false;
    if selection(state).is_none() {
        state.journal_notes_selection = None;
    }
}

/// Start editing the currently displayed Notes tab.
pub(crate) fn begin_edit(state: &mut AppState) {
    state.journal_notes_cursor = state.journal_notes.chars().count();
    state.journal_notes_selection = None;
    state.journal_notes_undo.clear();
    state.journal_notes_redo.clear();
    state.journal_notes_last_edit = None;
    state.journal_notes_vertical_goal = None;
    state.journal_notes_mouse_selecting = false;
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
    state.journal_notes_mouse_selecting = false;
    state.journal_notes_vertical_goal = None;
    state.focus = crate::state::Focus::Game;
}

/// Handle a key owned by the Notes editor. Escape saves and returns to the game.
#[cfg(all(test, feature = "t-input"))]
fn handle_key(state: &mut AppState, key: KeyEvent) {
    handle_key_with_width(state, key, state.journal_notes_wrap_width.max(1));
}

pub(crate) fn handle_key_with_width(state: &mut AppState, key: KeyEvent, width: u16) {
    state.journal_notes_mouse_selecting = false;
    let merge_kind = if !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        match key.code {
            KeyCode::Char(_) => Some(EditKind::Insert),
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
    if merge_kind.is_some() {
        state.journal_notes_vertical_goal = None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A'))
    {
        let len = state.journal_notes.chars().count();
        state.journal_notes_selection = (len > 0).then_some(0);
        state.journal_notes_cursor = len;
        state.journal_notes_vertical_goal = None;
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
    let by_word =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let len = state.journal_notes.chars().count();
    let cursor = state.journal_notes_cursor.min(len);
    match key.code {
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut buf = [0; 4];
            edit(state, EditKind::Insert, |state| {
                replace_selection_inner(state, c.encode_utf8(&mut buf))
            });
        }
        KeyCode::Enter => replace_selection(state, "\n"),
        KeyCode::Left if by_word => {
            state.journal_notes_vertical_goal = None;
            move_caret(state, word_left(&state.journal_notes, cursor), extend);
        }
        KeyCode::Right if by_word => {
            state.journal_notes_vertical_goal = None;
            move_caret(state, word_right(&state.journal_notes, cursor), extend);
        }
        KeyCode::Left if cursor > 0 => {
            state.journal_notes_vertical_goal = None;
            move_caret(state, cursor - 1, extend);
        }
        KeyCode::Right if cursor < len => {
            state.journal_notes_vertical_goal = None;
            move_caret(state, cursor + 1, extend);
        }
        KeyCode::Home | KeyCode::End => {
            let rows = wrapped_lines(&state.journal_notes, width);
            let (row, _) = caret_position(&rows, cursor);
            let target = if key.code == KeyCode::Home {
                rows[row].0
            } else {
                rows[row].1
            };
            state.journal_notes_vertical_goal = None;
            move_caret(state, target, extend);
        }
        KeyCode::Up | KeyCode::Down => {
            let rows = wrapped_lines(&state.journal_notes, width);
            let (row, col) = caret_position(&rows, cursor);
            let goal = *state.journal_notes_vertical_goal.get_or_insert(col);
            let target_row = if key.code == KeyCode::Up {
                row.saturating_sub(1)
            } else {
                (row + 1).min(rows.len().saturating_sub(1))
            };
            let (start, _, line) = &rows[target_row];
            let target = *start + crate::textwidth::col_to_char_idx(line, goal);
            move_caret(state, target, extend);
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
    let muted = state
        .colors
        .theme
        .get("muted")
        .style
        .add_modifier(Modifier::DIM);
    let content_top = area.y;
    let available = area.height;
    let selected = selection(state);
    let rows = wrapped_lines(&state.journal_notes, area.width);
    let (cursor_row, cursor_col) = caret_position(&rows, state.journal_notes_cursor);
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
        } else {
            crate::render::draw_str_clipped(buf, area.x, y, line, body, area);
        }
    }
    if state.journal_notes_editing && cursor_row >= first_row {
        let caret_y = content_top.saturating_add((cursor_row - first_row) as u16);
        if caret_y < area.bottom() {
            let line = &rows[cursor_row].2;
            let line_width = crate::textwidth::str_cells(line);
            let caret_col = if cursor_col < area.width as usize {
                if cursor_col < line_width {
                    crate::textwidth::cols_of_chars(
                        line,
                        crate::textwidth::col_to_char_idx(line, cursor_col),
                    )
                } else {
                    cursor_col
                }
            } else {
                crate::textwidth::cols_of_chars(
                    line,
                    crate::textwidth::col_to_char_idx(line, area.width.saturating_sub(1) as usize),
                )
            };
            let caret_x = area
                .x
                .saturating_add(caret_col.min(area.width.saturating_sub(1) as usize) as u16);
            if let Some(cell) = buf.cell_mut((caret_x, caret_y)) {
                if cell.symbol() == " " || cell.symbol().is_empty() {
                    cell.set_symbol(" ")
                        .set_style(body.add_modifier(Modifier::REVERSED));
                } else {
                    cell.set_style(cell.style().add_modifier(Modifier::REVERSED));
                }
            }
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
    fn control_arrows_move_by_word_and_shift_control_selects_by_word() {
        let mut state = AppState::default();
        state.journal_notes = "one two, three".into();
        state.journal_notes_cursor = 0;
        state.journal_notes_editing = true;

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes_cursor, 4);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
        );
        assert_eq!(state.journal_notes_cursor, 9);
        assert_eq!(copy_selection(&state).as_deref(), Some("two, "));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes_cursor, 4);
        assert_eq!(state.journal_notes_selection, None);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes_cursor, 0);
    }

    #[test]
    fn wraps_before_a_word_instead_of_splitting_it() {
        assert_eq!(
            wrapped_lines("The foobar", 7),
            vec![(0, 3, "The".into()), (4, 10, "foobar".into())]
        );
    }

    #[test]
    fn boundary_space_does_not_indent_the_next_visual_row() {
        assert_eq!(
            wrapped_lines("123456 next", 7),
            vec![(0, 6, "123456".into()), (7, 11, "next".into())]
        );
    }

    #[test]
    fn notes_wrap_before_the_caret_column() {
        let area = Rect::new(0, 0, 20, 10);
        let mut state = AppState::default();
        state.journal_notes_editing = true;
        let mut buf = Buffer::empty(area);
        let body = crate::journal::frame_body(&mut buf, area, "Notes", &state.colors, false);
        state.journal_notes = format!("{} ", "x".repeat(body.width.saturating_sub(2) as usize));
        state.journal_notes_cursor = state.journal_notes.chars().count();

        draw(&state, area, &mut buf);

        let content_top = body.y;
        let text_last = buf.cell((body.right() - 3, content_top)).unwrap();
        let trailing_space = buf.cell((body.right() - 2, content_top)).unwrap();
        let caret = buf.cell((body.right() - 1, content_top)).unwrap();
        assert_eq!(text_last.symbol(), "x");
        assert_eq!(trailing_space.symbol(), " ");
        assert_eq!(caret.symbol(), " ");
        assert!(caret.modifier.contains(Modifier::REVERSED));
        assert!(!text_last
            .style()
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert_eq!(buf.cell((body.x, content_top + 1)).unwrap().symbol(), " ");
    }

    #[test]
    fn cursor_on_a_trailing_empty_line_stays_at_the_bottom_of_the_visible_text() {
        let area = Rect::new(0, 0, 20, 10);
        let mut state = AppState::default();
        state.journal_notes = "line one\n".into();
        state.journal_notes_cursor = state.journal_notes.chars().count();
        state.journal_notes_editing = true;
        let mut buf = Buffer::empty(area);
        let body = content_area(area, &state.colors);
        let text_top = body.y;

        draw(&state, area, &mut buf);

        assert!(body.bottom() - text_top > 1);
        assert_eq!(buf.cell((body.x, text_top + 1)).unwrap().symbol(), " ");
        assert!(buf
            .cell((body.x, text_top + 1))
            .unwrap()
            .modifier
            .contains(Modifier::REVERSED));
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

    #[test]
    fn up_and_down_navigate_soft_wrapped_rows_and_keep_the_column() {
        let mut state = AppState::default();
        state.journal_notes = "one two three".into();
        state.journal_notes_cursor = 1;
        state.journal_notes_editing = true;
        state.journal_notes_wrap_width = 8;

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.journal_notes_cursor, 9);
        handle_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.journal_notes_cursor, 1);

        handle_key(&mut state, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.journal_notes_cursor, 7);
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.journal_notes_cursor, 13);
    }

    #[test]
    fn mouse_click_places_caret_and_drag_selects_text() {
        let area = Rect::new(0, 0, 30, 10);
        let mut state = AppState::default();
        state.journal_notes = "alpha beta".into();
        state.journal_notes_editing = true;
        let body = content_area(area, &state.colors);
        let text_y = text_area(area, &state.colors).y;

        let start = mouse_target_at(&state, area, body.x + 1, text_y).unwrap();
        set_mouse_cursor(&mut state, start, false);
        let end = mouse_target_drag(&state, area, body.x + 6, text_y).unwrap();
        set_mouse_cursor(&mut state, end, true);
        finish_mouse_selection(&mut state);

        assert_eq!(start, 1);
        assert_eq!(end, 6);
        assert_eq!(copy_selection(&state).as_deref(), Some("lpha "));
    }

    #[test]
    fn clicking_sets_a_plain_caret_and_drag_from_it_starts_selection() {
        let area = Rect::new(0, 0, 30, 10);
        let mut state = AppState::default();
        state.journal_notes = "alpha".into();
        state.journal_notes_editing = true;
        let body = content_area(area, &state.colors);

        let click_target = mouse_target_at(&state, area, body.x + 2, body.y).unwrap();
        set_mouse_cursor(&mut state, click_target, false);
        assert_eq!(state.journal_notes_selection, None);

        let drag_target = mouse_target_drag(&state, area, body.x + 4, body.y).unwrap();
        set_mouse_cursor(&mut state, drag_target, true);
        finish_mouse_selection(&mut state);
        assert_eq!(copy_selection(&state).as_deref(), Some("ph"));
    }

    #[test]
    fn shift_selection_is_highlighted_without_replacing_the_selected_glyph() {
        let area = Rect::new(0, 0, 30, 10);
        let mut state = AppState::default();
        state.journal_notes = "notes".into();
        state.journal_notes_cursor = 1;
        state.journal_notes_selection = Some(4);
        state.journal_notes_editing = true;
        let mut buf = Buffer::empty(area);
        let body = content_area(area, &state.colors);
        let y = body.y;

        draw(&state, area, &mut buf);

        let selected = buf.cell((body.x + 1, y)).unwrap();
        assert_eq!(selected.symbol(), "o");
        assert!(selected.style().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn mouse_click_maps_to_a_soft_wrapped_row() {
        let area = Rect::new(0, 0, 12, 12);
        let mut state = AppState::default();
        state.journal_notes = "one two three".into();
        let body = content_area(area, &state.colors);
        let text_area = text_area(area, &state.colors);
        let target = mouse_target_at(&state, area, body.x + 2, text_area.y + 1).unwrap();
        assert_eq!(target, 10);
    }

    #[test]
    fn notes_pane_has_no_shortcut_instructions() {
        let area = Rect::new(0, 0, 24, 12);
        let mut state = AppState::default();
        state.journal_notes_editing = true;
        state.journal_notes = "notes".into();
        let mut buf = Buffer::empty(area);

        draw(&state, area, &mut buf);

        let body = content_area(area, &state.colors);
        let mut content = String::new();
        for y in body.y..body.bottom() {
            for x in body.x..body.right() {
                content.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        assert!(!content.contains("Ctrl+"));
        assert!(!content.contains("Enter newline"));
        assert!(content.contains("notes"));
    }

    #[test]
    fn trailing_wrap_space_moves_to_the_next_row_without_an_extra_row_patch() {
        let mut state = AppState::default();
        state.journal_notes = "123456".into();
        state.journal_notes_wrap_width = 7;
        state.journal_notes_cursor = state.journal_notes.chars().count();
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        let width = state.journal_notes_wrap_width;
        assert_eq!(state.journal_notes, "123456 ");
        state.journal_notes_cursor = state.journal_notes.chars().count();
        let rows = wrapped_lines(&state.journal_notes, width);

        assert_eq!(rows, vec![(0, 6, "123456".into()), (6, 7, " ".into())]);
        assert_eq!(caret_position(&rows, 7), (1, 1));
    }

    #[test]
    fn undo_recovers_text_after_selection_delete_and_a_long_paste() {
        let mut state = AppState::default();
        state.journal_notes = "the original note".into();
        state.journal_notes_cursor = state.journal_notes.chars().count();
        state.journal_notes_editing = true;
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        let long_text = "a long replacement pasted into the notes pane ".repeat(25);
        paste(&mut state, &long_text);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert!(
            state.journal_notes.is_empty(),
            "the paste undoes as one action"
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.journal_notes, "the original note");
    }

    #[test]
    fn adjacent_character_input_including_spaces_and_punctuation_is_one_undo_action() {
        let mut state = AppState::default();
        state.journal_notes_editing = true;
        for ch in "A sentence, with spaces.".chars() {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        );
        assert!(state.journal_notes.is_empty());
    }
}
