//! Small form primitives shared by TUI overlays.

/// A single-line editable string with a cursor tracked as a character index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn insert(&mut self, character: char) {
        let byte = byte_index(&self.value, self.cursor);
        self.value.insert(byte, character);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = byte_index(&self.value, self.cursor - 1);
        let end = byte_index(&self.value, self.cursor);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        let chars = self.value.chars().count();
        if self.cursor >= chars {
            return;
        }
        let start = byte_index(&self.value, self.cursor);
        let end = byte_index(&self.value, self.cursor + 1);
        self.value.replace_range(start..end, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    pub fn with_cursor(&self) -> String {
        let byte = byte_index(&self.value, self.cursor);
        format!("{}|{}", &self.value[..byte], &self.value[byte..])
    }
}

fn byte_index(value: &str, character_index: usize) -> usize {
    value
        .char_indices()
        .nth(character_index)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::TextInput;

    #[test]
    fn edits_at_character_boundaries() {
        let mut input = TextInput::new("Aldén");
        input.left();
        input.insert('e');
        assert_eq!(input.value, "Aldéen");
        input.backspace();
        assert_eq!(input.value, "Aldén");
        input.home();
        input.delete();
        assert_eq!(input.value, "ldén");
    }

    #[test]
    fn cursor_rendering_keeps_unicode_intact() {
        let mut input = TextInput::new("Möss");
        input.left();
        input.left();
        assert_eq!(input.with_cursor(), "Mö|ss");
    }
}
