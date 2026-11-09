//! The text that a field holds while it has the keyboard, and the keys that change it. gpui 0.2 has no text box, so
//! the window draws its fields itself and routes the keys here, as the player's window does.

use gpui::Keystroke;

/// The character drawn at the cursor of the active field.
pub const CARET: char = '|';

/// What a key did to a field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The text changed.
    Changed,
    /// Only the cursor moved, or the key did nothing.
    Kept,
    /// Enter: the field is done.
    Submit,
    /// Escape: the field gives up the keyboard.
    Cancel,
}

/// A field's text and the cursor in it, as a byte index on a character boundary.
#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    pub text: String,
    cursor: usize,
}

impl LineEdit {
    /// A field that holds `text`, with the cursor at its end.
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }

    /// The text with the caret drawn at the cursor.
    pub fn shown(&self) -> String {
        let mut shown = self.text.clone();
        shown.insert(self.cursor, CARET);
        shown
    }

    /// Applies one key.
    pub fn key(&mut self, keystroke: &Keystroke, multiline: bool, pasted: Option<String>) -> Outcome {
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;

        match keystroke.key.as_str() {
            "escape" => Outcome::Cancel,
            "enter" if multiline && keystroke.modifiers.shift => self.insert("\n"),
            "enter" => Outcome::Submit,
            "backspace" => match self.previous() {
                Some(start) => {
                    self.text.replace_range(start..self.cursor, "");
                    self.cursor = start;
                    Outcome::Changed
                }
                None => Outcome::Kept,
            },
            "delete" => match self.next() {
                Some(end) => {
                    self.text.replace_range(self.cursor..end, "");
                    Outcome::Changed
                }
                None => Outcome::Kept,
            },
            "left" => {
                self.cursor = self.previous().unwrap_or(self.cursor);
                Outcome::Kept
            }
            "right" => {
                self.cursor = self.next().unwrap_or(self.cursor);
                Outcome::Kept
            }
            "home" => {
                self.cursor = 0;
                Outcome::Kept
            }
            "end" => {
                self.cursor = self.text.len();
                Outcome::Kept
            }
            "v" if command => match pasted {
                Some(text) if multiline => self.insert(&text),
                Some(text) => self.insert(text.lines().next().unwrap_or_default()),
                None => Outcome::Kept,
            },
            _ if command => Outcome::Kept,
            _ => match keystroke.key_char.as_deref().filter(|typed| !typed.chars().any(char::is_control)) {
                Some(typed) => self.insert(typed),
                None => Outcome::Kept,
            },
        }
    }

    fn insert(&mut self, typed: &str) -> Outcome {
        self.text.insert_str(self.cursor, typed);
        self.cursor += typed.len();
        Outcome::Changed
    }

    /// The start of the character before the cursor.
    fn previous(&self) -> Option<usize> {
        self.text[..self.cursor].char_indices().next_back().map(|(at, _)| at)
    }

    /// The end of the character after the cursor.
    fn next(&self) -> Option<usize> {
        self.text[self.cursor..].chars().next().map(|character| self.cursor + character.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(edit: &mut LineEdit, key: &str) -> Outcome {
        edit.key(&Keystroke::parse(key).expect("a valid keystroke"), false, None)
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_backspace_removes_one_character() {
        let mut edit = LineEdit::new("aé".into());

        press(&mut edit, "left");
        press(&mut edit, "backspace");
        assert_eq!(edit.text, "é");

        press(&mut edit, "end");
        press(&mut edit, "backspace");
        assert_eq!(edit.text, "");
        assert_eq!(press(&mut edit, "backspace"), Outcome::Kept);
    }

    #[test]
    fn the_caret_marks_the_cursor() {
        let mut edit = LineEdit::new("ab".into());

        press(&mut edit, "home");
        press(&mut edit, "right");
        assert_eq!(edit.shown(), "a|b");
    }

    #[test]
    fn enter_submits_and_escape_cancels() {
        let mut edit = LineEdit::default();

        assert_eq!(press(&mut edit, "enter"), Outcome::Submit);
        assert_eq!(press(&mut edit, "escape"), Outcome::Cancel);
    }
}
