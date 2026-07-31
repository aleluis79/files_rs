use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

pub struct ViewerState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub scroll_x: usize,
    pub editing: bool,
    pub cursor: (usize, usize),
    pub selection_anchor: Option<(usize, usize)>,
    pub clipboard: Option<String>,
    pub original_lines: Option<Vec<String>>,
    undo_stack: Vec<(Vec<String>, (usize, usize), Option<(usize, usize)>)>,
}

impl ViewerState {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("No se pudo leer {}", path.display()))?;
        Self::from_bytes(path.to_path_buf(), bytes)
    }

    pub fn from_bytes(path: PathBuf, bytes: Vec<u8>) -> Result<Self> {
        if bytes.iter().take(4096).any(|byte| *byte == 0) {
            bail!("El archivo no parece ser texto legible");
        }

        let content = String::from_utf8_lossy(&bytes);
        let lines = content
            .lines()
            .map(|line| line.replace('\t', "    "))
            .collect::<Vec<_>>();

        Ok(Self {
            path,
            lines: if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            },
            scroll: 0,
            scroll_x: 0,
            editing: false,
            cursor: (0, 0),
            selection_anchor: None,
            clipboard: None,
            original_lines: None,
            undo_stack: Vec::new(),
        })
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn enter_edit_mode(&mut self) {
        self.editing = true;
        self.original_lines = Some(self.lines.clone());
        self.cursor = (0, 0);
        self.scroll = 0;
        self.scroll_x = 0;
        self.selection_anchor = None;
        self.undo_stack.clear();
    }

    fn line_len_chars(&self, line_idx: usize) -> usize {
        self.lines
            .get(line_idx)
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn clamp_cursor_column(&self, line_idx: usize, col_idx: usize) -> usize {
        self.line_len_chars(line_idx).min(col_idx)
    }

    fn byte_index_for_column(line: &str, col_idx: usize) -> usize {
        line.chars().take(col_idx).map(char::len_utf8).sum()
    }

    fn substring_by_columns(line: &str, start_col: usize, end_col: usize) -> String {
        let chars = line.chars().collect::<Vec<_>>();
        chars
            .iter()
            .skip(start_col)
            .take(end_col.saturating_sub(start_col))
            .collect()
    }

    fn selection_range(&self) -> Option<(usize, usize, usize, usize)> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor;
        let (start, end) = if (anchor.0, anchor.1) <= (cursor.0, cursor.1) {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };

        if start == end {
            None
        } else {
            Some((start.0, start.1, end.0, end.1))
        }
    }

    pub fn start_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn copy_selection(&self) -> Option<String> {
        let Some((start_line, start_col, end_line, end_col)) = self.selection_range() else {
            return None;
        };

        if start_line == end_line {
            return Some(Self::substring_by_columns(
                &self.lines[start_line],
                start_col,
                end_col,
            ));
        }

        let mut parts = vec![Self::substring_by_columns(
            &self.lines[start_line],
            start_col,
            self.line_len_chars(start_line),
        )];
        for line_idx in start_line + 1..end_line {
            parts.push(self.lines[line_idx].clone());
        }
        parts.push(Self::substring_by_columns(
            &self.lines[end_line],
            0,
            end_col,
        ));

        Some(parts.join("\n"))
    }

    pub fn copy_selection_to_clipboard(&mut self) -> Option<String> {
        let copied = self.copy_selection();
        if let Some(text) = copied.clone() {
            self.clipboard = Some(text);
        }
        copied
    }

    pub fn paste_from_clipboard(&mut self) {
        if !self.editing {
            return;
        }

        if let Some(text) = self.clipboard.clone() {
            self.push_undo_state();
            self.insert_text(&text);
        }
    }

    fn push_undo_state(&mut self) {
        self.undo_stack.push((self.lines.clone(), self.cursor, self.selection_anchor));
    }

    pub fn undo(&mut self) {
        if !self.editing {
            return;
        }

        if let Some((lines, cursor, selection_anchor)) = self.undo_stack.pop() {
            self.lines = lines;
            self.cursor = cursor;
            self.selection_anchor = selection_anchor;
        }
    }

    pub fn delete_selection(&mut self) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        let Some((start_line, start_col, end_line, end_col)) = self.selection_range() else {
            return;
        };

        if start_line == end_line {
            let mut fallback = String::new();
            let line = self.lines.get_mut(start_line).unwrap_or(&mut fallback);
            let start_byte = Self::byte_index_for_column(line, start_col.min(line.chars().count()));
            let end_byte = Self::byte_index_for_column(line, end_col.min(line.chars().count()));
            line.replace_range(start_byte..end_byte, "");
            self.cursor = (start_line, start_col);
            self.selection_anchor = None;
            return;
        }

        let prefix = Self::substring_by_columns(&self.lines[start_line], 0, start_col);
        let suffix = Self::substring_by_columns(&self.lines[end_line], end_col, self.line_len_chars(end_line));
        self.lines[start_line] = prefix + &suffix;
        self.lines.drain(start_line + 1..=end_line);
        self.cursor = (start_line, start_col);
        self.selection_anchor = None;
    }

    pub fn move_cursor_left(&mut self) {
        if !self.editing {
            return;
        }

        let (line_idx, col_idx) = self.cursor;
        if col_idx > 0 {
            self.cursor.1 = col_idx.saturating_sub(1);
            return;
        }

        if line_idx > 0 {
            let previous_line = line_idx.saturating_sub(1);
            self.cursor = (previous_line, self.line_len_chars(previous_line));
        }
    }

    pub fn move_cursor_right(&mut self) {
        if !self.editing {
            return;
        }

        let (line_idx, col_idx) = self.cursor;
        let line_len = self.line_len_chars(line_idx);
        if col_idx < line_len {
            self.cursor.1 = col_idx.saturating_add(1);
            return;
        }

        if line_idx + 1 < self.lines.len() {
            self.cursor = (line_idx + 1, 0);
        }
    }

    pub fn move_cursor_up(&mut self) {
        if !self.editing {
            return;
        }

        let (line_idx, col_idx) = self.cursor;
        if line_idx > 0 {
            let previous_line = line_idx.saturating_sub(1);
            let column = self.clamp_cursor_column(previous_line, col_idx);
            self.cursor = (previous_line, column);
        }
    }

    pub fn move_cursor_down(&mut self) {
        if !self.editing {
            return;
        }

        let (line_idx, col_idx) = self.cursor;
        if line_idx + 1 < self.lines.len() {
            let next_line = line_idx + 1;
            let column = self.clamp_cursor_column(next_line, col_idx);
            self.cursor = (next_line, column);
        }
    }

    pub fn move_cursor_home(&mut self) {
        if !self.editing {
            return;
        }
        self.cursor.1 = 0;
    }

    pub fn move_cursor_end(&mut self) {
        if !self.editing {
            return;
        }
        let line_idx = self.cursor.0;
        self.cursor.1 = self.line_len_chars(line_idx);
    }

    pub fn ensure_cursor_visible(&mut self, viewport_height: usize, viewport_width: usize) {
        if !self.editing {
            return;
        }

        let viewport_height = viewport_height.max(1);
        let top = self.scroll;
        let bottom = self.scroll.saturating_add(viewport_height.saturating_sub(1));

        if self.cursor.0 < top {
            self.scroll = self.cursor.0;
        } else if self.cursor.0 > bottom {
            self.scroll = self.cursor.0.saturating_sub(viewport_height.saturating_sub(1));
        }

        let viewport_width = viewport_width.max(1);
        let margin = 3usize;
        let left = self.scroll_x.saturating_add(margin);
        let right = self.scroll_x.saturating_add(viewport_width.saturating_sub(1).saturating_sub(margin));

        if self.cursor.1 < left {
            self.scroll_x = self.cursor.1.saturating_sub(margin).max(0);
        } else if self.cursor.1 > right {
            self.scroll_x = self.cursor.1.saturating_sub(viewport_width.saturating_sub(1).saturating_sub(margin));
        }
    }

    fn insert_char_internal(&mut self, ch: char) {
        let (line_idx, col_idx) = self.cursor;
        let mut fallback = String::new();
        let line = self.lines.get_mut(line_idx).unwrap_or(&mut fallback);
        let line_len_chars = line.chars().count();
        let byte_idx = Self::byte_index_for_column(line, col_idx.min(line_len_chars));
        line.insert(byte_idx, ch);
        self.cursor.1 = self.cursor.1.saturating_add(1);
    }

    pub fn insert_char(&mut self, ch: char) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        self.insert_char_internal(ch);
    }

    pub fn insert_text(&mut self, text: &str) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for ch in normalized.chars() {
            if ch == '\n' {
                self.insert_new_line_internal();
            } else {
                self.insert_char_internal(ch);
            }
        }
    }

    pub fn delete_char(&mut self) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        let (line_idx, col_idx) = self.cursor;
        if col_idx > 0 {
            let mut fallback = String::new();
            let line = self.lines.get_mut(line_idx).unwrap_or(&mut fallback);
            let byte_idx = Self::byte_index_for_column(line, col_idx.saturating_sub(1));
            line.remove(byte_idx);
            self.cursor.1 = self.cursor.1.saturating_sub(1);
            return;
        }

        if line_idx > 0 {
            let previous = self.lines.remove(line_idx);
            let mut fallback = String::new();
            let target = self.lines.get_mut(line_idx.saturating_sub(1)).unwrap_or(&mut fallback);
            target.push_str(&previous);
            self.cursor = (line_idx.saturating_sub(1), target.chars().count());
        }
    }

    pub fn delete_char_forward(&mut self) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        let (line_idx, col_idx) = self.cursor;
        let line_len = self.line_len_chars(line_idx);
        if col_idx < line_len {
            let mut fallback = String::new();
            let line = self.lines.get_mut(line_idx).unwrap_or(&mut fallback);
            let byte_idx = Self::byte_index_for_column(line, col_idx);
            line.remove(byte_idx);
            return;
        }

        if line_idx + 1 < self.lines.len() {
            let next = self.lines.remove(line_idx + 1);
            let mut fallback = String::new();
            let target = self.lines.get_mut(line_idx).unwrap_or(&mut fallback);
            target.push_str(&next);
        }
    }

    fn insert_new_line_internal(&mut self) {
        let (line_idx, col_idx) = self.cursor;
        if let Some(line) = self.lines.get_mut(line_idx) {
            let column = col_idx.min(line.chars().count());
            let byte_idx = Self::byte_index_for_column(line, column);
            let remainder = line[byte_idx..].to_string();
            line.truncate(byte_idx);
            self.lines.insert(line_idx + 1, remainder);
            self.cursor = (line_idx + 1, 0);
        }
    }

    pub fn insert_new_line(&mut self) {
        if !self.editing {
            return;
        }

        self.push_undo_state();
        self.insert_new_line_internal();
    }

    pub fn save_edit(&mut self) -> Result<()> {
        self.original_lines = Some(self.lines.clone());
        self.editing = false;
        let content = self.lines.join("\n");
        fs::write(&self.path, format!("{content}\n")).with_context(|| {
            format!("No se pudo guardar {}", self.path.display())
        })?;
        Ok(())
    }

    pub fn discard_edit(&mut self) {
        if let Some(original) = self.original_lines.clone() {
            self.lines = original;
        }
        self.editing = false;
        self.cursor = (0, 0);
        self.scroll = 0;
        self.scroll_x = 0;
        self.selection_anchor = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.original_lines.as_ref().is_some_and(|original| *original != self.lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    fn make_temp_file(content: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("files-rs-viewer-test-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("sample.txt");
        fs::write(&path, content).expect("write temp file");
        path
    }

    #[test]
    fn saving_editor_changes_updates_the_file_and_viewer_lines() {
        let path = make_temp_file("first line\nsecond line\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["updated line".to_string(), "second line".to_string()];
        viewer.save_edit().expect("save edit");

        assert_eq!(viewer.lines, vec!["updated line", "second line"]);
        let saved = fs::read_to_string(&path).expect("read saved file");
        assert_eq!(saved, "updated line\nsecond line\n");
    }

    #[test]
    fn discarding_editor_changes_keeps_the_original_file_unchanged() {
        let path = make_temp_file("keep me\nunchanged\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["new content".to_string()];
        viewer.discard_edit();

        assert_eq!(viewer.lines, vec!["keep me", "unchanged"]);
        let discarded = fs::read_to_string(&path).expect("read discarded file");
        assert_eq!(discarded, "keep me\nunchanged\n");
    }

    #[test]
    fn cursor_navigation_moves_between_lines_without_modifying_text() {
        let path = make_temp_file("hello\nworld\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["hello".to_string(), "world".to_string()];
        viewer.move_cursor_right();
        viewer.move_cursor_right();
        viewer.move_cursor_right();
        viewer.move_cursor_down();

        assert_eq!(viewer.cursor, (1, 3));
        assert_eq!(viewer.lines, vec!["hello", "world"]);
    }

    #[test]
    fn inserting_multibyte_characters_keeps_the_cursor_in_a_valid_position() {
        let path = make_temp_file("hola\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["h".to_string()];
        viewer.insert_char('ñ');
        viewer.insert_char('ñ');

        assert_eq!(viewer.lines, vec!["ññh"]);
        assert_eq!(viewer.cursor, (0, 2));
    }

    #[test]
    fn cursor_movement_updates_the_scroll_offset_when_the_cursor_leaves_the_visible_window() {
        let path = make_temp_file("line 1\nline 2\nline 3\nline 4\nline 5\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
            "line 4".to_string(),
            "line 5".to_string(),
        ];

        viewer.move_cursor_down();
        viewer.move_cursor_down();
        viewer.move_cursor_down();
        viewer.ensure_cursor_visible(3, 3);

        assert_eq!(viewer.cursor, (3, 0));
        assert_eq!(viewer.scroll, 1);
    }

    #[test]
    fn cursor_movement_updates_the_horizontal_scroll_offset_when_the_cursor_leaves_the_visible_window() {
        let path = make_temp_file("line 1\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["abcdefghijklmnop".to_string()];
        viewer.cursor = (0, 12);

        viewer.ensure_cursor_visible(6, 6);

        assert_eq!(viewer.cursor, (0, 12));
        assert_eq!(viewer.scroll_x, 10);
    }

    #[test]
    fn pasting_from_the_internal_clipboard_inserts_the_copied_text() {
        let path = make_temp_file("hola\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["hola".to_string()];
        viewer.cursor = (0, 0);
        viewer.clipboard = Some("abc".to_string());

        viewer.paste_from_clipboard();

        assert_eq!(viewer.lines, vec!["abchola"]);
        assert_eq!(viewer.cursor, (0, 3));
    }

    #[test]
    fn undo_restores_the_previous_text_state() {
        let path = make_temp_file("hola\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["hola".to_string()];
        viewer.cursor = (0, 0);

        viewer.insert_char('x');
        viewer.undo();

        assert_eq!(viewer.lines, vec!["hola"]);
        assert_eq!(viewer.cursor, (0, 0));
    }

    #[test]
    fn inserting_pasted_text_inserts_multiple_lines_and_characters() {
        let path = make_temp_file("hola\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["hola".to_string()];
        viewer.cursor = (0, 0);

        viewer.insert_text("ab\ncd");

        assert_eq!(viewer.lines, vec!["ab", "cdhola"]);
        assert_eq!(viewer.cursor, (1, 2));
    }

    #[test]
    fn deleting_a_selection_removes_the_selected_range() {
        let path = make_temp_file("abcde\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["abcde".to_string()];
        viewer.cursor = (0, 1);
        viewer.selection_anchor = Some((0, 1));
        viewer.cursor = (0, 4);

        viewer.delete_selection();

        assert_eq!(viewer.lines, vec!["ae"]);
        assert_eq!(viewer.cursor, (0, 1));
    }

    #[test]
    fn copying_a_selection_stores_the_selected_text() {
        let path = make_temp_file("abcde\n");
        let mut viewer = ViewerState::open(&path).expect("open viewer file");

        viewer.enter_edit_mode();
        viewer.lines = vec!["abcde".to_string()];
        viewer.cursor = (0, 1);
        viewer.selection_anchor = Some((0, 1));
        viewer.cursor = (0, 4);

        let copied = viewer.copy_selection();

        assert_eq!(copied, Some("bcd".to_string()));
    }
}
