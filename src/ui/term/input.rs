
use crate::ui::term::{Window, text::force_width};

/// UI for input/editing of a single line of text on the terminal.
pub struct InputLine {
    // We could have used a more clever data structure, but as best I could tell from a cursory
    // attempt to look through tinyfugue's source they're not doing anything more clever than
    // shuffling memory around, either.  Maybe we'll need/want to upgrade, but we can start simple
    // and see if it performs unacceptably for the kind of editing we need to do.
    //
    // TODO: Should this be Vec<String> later because of unicode?  char is probably faster for now
    // though.
    buffer: Vec<char>,
    // The cursor is 0-indexed... but keep in mind that we usually think of a cursor as BETWEEN two
    // characters.
    cursor: usize,
    target_width: usize,
}

impl Window for InputLine {
    fn render(&self) -> Vec<String> {
        // Split the buffer up into chunks of size `target_width`, turn them into strings and
        // force_width() them.
        let chunks: Vec<String> = self.buffer.chunks(self.target_width).map(|chunk| {
            let chunk: String = chunk.iter().collect();
            force_width(chunk, self.target_width)
        }).collect();

        // It's possible for there to be no results if the buffer is completely empty, which
        // happens when someone erases everything in the line or it's been cleared.  In that case,
        // we want to still return a single line of spaces so the screen clears.
        if chunks.len() > 0 {
            chunks
        } else {
            vec![" ".repeat(self.target_width)]
        }
    }

    fn get_size(&self) -> (usize, usize) {
        // This is probably stupid, but casting to a float and using ceil seemed even more stupid.
        let mut lines: usize = self.buffer.len() / self.target_width;
        let remainder: usize = self.buffer.len() % self.target_width;
        if remainder > 0 || lines == 0 {
            lines += 1;
        }
        (self.target_width, lines)
    }

    fn get_cursor_pos(&self) -> (usize, usize) {
        let x: usize = self.cursor % self.target_width;
        let y: usize = self.cursor / self.target_width;

        (x, y)
    }

    fn set_width(&mut self, new_w: usize) {
        self.target_width = new_w;
    }

    fn set_height(&mut self, _new_h: usize) {
        panic!("Can't set the height of an InputLine: It's derived dynamically.");
    }
}

impl InputLine {
    pub fn new(width: usize, _height: usize) -> InputLine {
        InputLine {
            buffer: vec![],
            cursor: 0,
            target_width: width,
        }
    }

    /// Insert a single character at the current cursor position.
    pub fn insert_char(&mut self, what: char) {
        // The cursor is considered to be between two characters.  So, taken as an array index, it
        // will point to the character directly after itself, unless it's at the end, in which case
        // using it like an index will probably cause a panic.
        if self.cursor >= self.buffer.len() {
            self.buffer.push(what);
            self.cursor = self.buffer.len();
        } else {
            self.buffer.insert(self.cursor, what);
            self.cursor += 1;
        }
    }

    /// Delete n chars ahead of the cursor (positive input) or behind it (negative input), moving
    /// it backward if appropriate.
    pub fn delete_chars(&mut self, n: isize) {
        if n.is_negative() {
            // What we do is split the Vec in half, truncate the first half (e.g. what's before the
            // cursor) by however much we need to, and then glue the two halves back together.  The
            // cursor has to be moved back in this situation, as well.

            let to_del = n.abs() as usize;

            let mut remainder = if self.cursor < self.buffer.len() {
                // Note that split_off returns the 'rest of' the array, e.g., everything from its
                // argument's index to array end inclusive.
                self.buffer.split_off(self.cursor)
            } else {
                vec![]
            };

            if n.abs() as usize >= self.buffer.len() {
                // Kill everything
                self.buffer = vec![];
            } else {
                // Kill n characters
                self.buffer.truncate(self.buffer.len() - to_del);
            }

            self.buffer.append(&mut remainder);
            self.cursor = if to_del < self.cursor {
                self.cursor - to_del
            } else {
                0
            };
        } else {
            // What we'll do here is split the vector in half again, but we're going to split it at
            // (cursor + n chars) -- after that we basically do the same thing and truncate those n
            // chars.  We don't have to move the cursor since we're only deleting things to the
            // right.

            let splitpoint = self.cursor + n as usize;
            let mut remainder = if splitpoint < self.buffer.len() {
                self.buffer.split_off(splitpoint)
            } else {
                vec![]
            };

            if n as usize >= self.buffer.len() {
                self.buffer = vec![];
            } else {
                self.buffer.truncate(self.buffer.len() - n as usize);
            }

            self.buffer.append(&mut remainder);
        }
    }

    /// Set the contents of the input to some String.
    pub fn set_string(&mut self, what: String) {
        self.buffer = what.chars().collect();
        // We have to reset the cursor to somewhere anyway.
        self.cursor = 0;
    }

    /// Move the cursor `offset` chars to the left or right in the buffer, not allowing it to go
    /// out-of-bounds.
    pub fn move_cursor(&mut self, offset: isize) {
        if offset.is_negative() {
            let backwards = offset.abs() as usize;
            self.cursor = if backwards > self.cursor {
                0
            } else {
                self.cursor - backwards
            };
        } else {
            self.cursor += offset as usize;
            if self.cursor > self.buffer.len() {
                self.cursor = self.buffer.len();
            }
        }
    }

    pub fn as_text(&self) -> String {
        let result: String = self.buffer.iter().collect();
        result
    }
}

