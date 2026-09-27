//! Plain stderr previews: append new words, start a new line when text is revised.
use hear::streaming::TranscriptUpdate;
use std::io::{self, Write};

#[derive(Default)]
pub struct Display {
    segment: String,
    text: String,
    open: bool,
}

impl Display {
    pub fn finish(&mut self, output: &mut impl Write) -> io::Result<()> {
        if self.open {
            writeln!(output)?;
            output.flush()?;
            self.open = false;
        }
        Ok(())
    }

    pub fn update(&mut self, output: &mut impl Write, update: &TranscriptUpdate) -> io::Result<()> {
        // Keep each preview on a plain line, including when stderr is redirected.
        let text = update.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() && !self.open {
            return Ok(());
        }
        if self.open && self.segment == update.segment_id && text.starts_with(&self.text) {
            write!(output, "{}", &text[self.text.len()..])?;
        } else {
            if self.open {
                writeln!(output)?;
            }
            write!(output, "[live] {text}")?;
        }
        self.segment.clone_from(&update.segment_id);
        self.text = text;
        self.open = !update.committed;
        if update.committed {
            writeln!(output)?;
        }
        output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_words_but_displays_revisions_and_new_segments_on_new_lines() {
        let mut display = Display::default();
        let mut output = Vec::new();
        for (id, text, committed) in [
            ("a", "hello", false),
            ("a", "hello", false),
            ("a", "hello world", false),
            ("a", "Hello world!", true),
            ("b", "next\nline\u{001b}", true),
        ] {
            display
                .update(
                    &mut output,
                    &TranscriptUpdate {
                        segment_id: id.into(),
                        text: text.into(),
                        committed,
                    },
                )
                .unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[live] hello world\n[live] Hello world!\n[live] next line\n"
        );
    }
}
