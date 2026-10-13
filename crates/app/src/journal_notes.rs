//! The Journal's per-story Notes section and its small inline editor.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use unicode_segmentation::UnicodeSegmentation;

use crate::state::AppState;

fn byte_at(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(at, _)| at)
}

/// Wrap physical note lines into screen-width rows while retaining their
/// document character offsets for selection and caret placement.
fn wrapped_lines(text: &str, width: u16) -> Vec<(usize, usize, String)> {
    let width = usize::from(width.max(1));
    let physical: Vec<&str> = text.split('\n').collect();
    let mut rows = Vec::new();
    let mut offset = 0usize;
    for (line_index, line) in physical.iter().enumerate() {
        let mut start = offset;
        let mut cells = 0usize;
        let mut chunk = String::new();
        for grapheme in line.graphemes(true) {
            let glyph_width = crate::textwidth::str_cells(grapheme).max(1);
            if cells > 0 && cells + glyph_width > width {
                rows.push((start, offset, std::mem::take(&mut chunk)));
                start = offset;
                cells = 0;
            }
            chunk.push_str(grapheme);
            offset += grapheme.chars().count();
            cells += glyph_width;
            if cells >= width {
                rows.push((start, offset, std::mem::take(&mut chunk)));
                start = offset;
                cells = 0;
            }
        }
        if !chunk.is_empty() || line.is_empty() {
            rows.push((start, offset, chunk));
        }
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

fn replace_selection(state: &mut AppState, insert: &str) {
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
    state.journal_notes_editing = true;
    state.focus = crate::state::Focus::Map;
}

/// Save and leave the Notes editor, returning keyboard focus to the game.
pub fn finish_edit(state: &mut AppState) {
    if !state.journal_notes_editing {
        return;
    }
    let _ = crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes);
    state.journal_notes_editing = false;
    state.journal_notes_selection = None;
    state.focus = crate::state::Focus::Game;
}

/// Handle a key owned by the Notes editor. Escape saves and returns to the game.
pub(crate) fn handle_key(state: &mut AppState, key: KeyEvent) {
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
        let _ = crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes);
        state.set_status("journal notes saved");
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
            replace_selection(state, c.encode_utf8(&mut buf));
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
            if selection(state).is_some() {
                replace_selection(state, "");
            } else if cursor > 0 {
                let start = byte_at(&state.journal_notes, cursor - 1);
                let end = byte_at(&state.journal_notes, cursor);
                state.journal_notes.replace_range(start..end, "");
                state.journal_notes_cursor -= 1;
            }
        }
        KeyCode::Delete => {
            if selection(state).is_some() {
                replace_selection(state, "");
            } else if cursor < len {
                let start = byte_at(&state.journal_notes, cursor);
                let end = byte_at(&state.journal_notes, cursor + 1);
                state.journal_notes.replace_range(start..end, "");
            }
        }
        _ => {}
    }
    if matches!(
        key.code,
        KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace | KeyCode::Delete
    ) {
        // Keep the sidecar current as the player types, so an application exit
        // cannot discard an edited note that has not yet left the tab.
        let _ = crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes);
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
    let _ = crate::styles::write_per_game_journal_notes(&state.game_dir, &state.journal_notes);
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
        "Ctrl+A all · Ctrl+C copy · Ctrl+Shift+V paste"
    } else {
        "Shift+Enter or click: edit notes"
    };
    crate::render::draw_str_clipped(buf, area.x, area.y, hint, muted, area);
    if state.journal_notes_editing {
        crate::render::draw_str_clipped(buf, area.x, area.y.saturating_add(1), "Enter newline · Ctrl+S save · Esc save/exit", muted, area);
    }
    let content_top = area.y.saturating_add(2);
    let available = area.bottom().saturating_sub(content_top);
    let selected = selection(state);
    let rows = wrapped_lines(&state.journal_notes, area.width);
    let cursor_row = rows
        .iter()
        .enumerate()
        .rfind(|(_, (start, end, _))| *start <= state.journal_notes_cursor && state.journal_notes_cursor <= *end)
        .map_or(0, |(row, _)| row);
    let first_row = if state.journal_notes_editing {
        cursor_row.saturating_add(1).saturating_sub(available as usize)
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
        let game_dir = std::env::temp_dir().join(format!(
            "lanthorn-journal-notes-input-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&game_dir).unwrap();
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
        assert_eq!(
            crate::styles::read_per_game_journal_notes(&game_dir),
            "noXes"
        );
        state.focus = crate::state::Focus::Map;
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.focus, crate::state::Focus::Game);
        assert!(!state.journal_notes_editing);
        let _ = std::fs::remove_dir_all(game_dir);
    }

}
