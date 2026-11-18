//! A one-line text editor.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub struct Input {
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
}

impl Input {
    pub fn new(text: &str) -> Self {
        Input { text: text.into(), cursor: text.chars().count() }
    }

    pub fn set(&mut self, text: &str) {
        *self = Input::new(text);
    }

    fn byte(&self, char_idx: usize) -> usize {
        self.text.char_indices().nth(char_idx).map_or(self.text.len(), |(i, _)| i)
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn at_end(&self) -> bool {
        self.cursor == self.len()
    }

    /// Text before the cursor, for placing the terminal cursor.
    pub fn before_cursor(&self) -> &str {
        &self.text[..self.byte(self.cursor)]
    }

    /// Returns true when the key was an editing key.
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.text.drain(..self.byte(self.cursor));
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.text.chars().collect();
                let mut start = self.cursor;
                while start > 0 && chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && chars[start - 1] != ' ' && chars[start - 1] != '/' {
                    start -= 1;
                }
                let (a, b) = (self.byte(start), self.byte(self.cursor));
                self.text.drain(a..b);
                self.cursor = start;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.len(),
            KeyCode::Char(c) if !ctrl => {
                let at = self.byte(self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let at = self.byte(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Delete if !self.at_end() => {
                let at = self.byte(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            KeyCode::Backspace | KeyCode::Delete => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn edits_unicode_text() {
        let mut i = Input::new("hå");
        i.handle(key(KeyCode::Left));
        i.handle(key(KeyCode::Char('ö')));
        assert_eq!(i.text, "höå");
        i.handle(key(KeyCode::Backspace));
        i.handle(key(KeyCode::Backspace));
        assert_eq!((i.text.as_str(), i.cursor), ("å", 0));
        let mut w = Input::new("~/repos/foo");
        w.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(w.text, "~/repos/");
    }
}
