use anyhow::{Result, bail};

use crate::FormatContext;

pub(super) struct PreparedTranscript<'a> {
    pub(super) context: FormatContext,
    pub(super) body: &'a str,
}

pub(super) fn prepare(
    transcript: &str,
    explicit_context: Option<FormatContext>,
) -> Result<PreparedTranscript<'_>> {
    let transcript = transcript.trim();
    if transcript.is_empty() {
        bail!("cannot polish an empty transcript");
    }
    if let Some(context) = explicit_context {
        return Ok(PreparedTranscript {
            context,
            body: transcript,
        });
    }

    let token_end = transcript
        .find(char::is_whitespace)
        .unwrap_or(transcript.len());
    let token = transcript[..token_end]
        .trim_matches(|character: char| matches!(character, ':' | ',' | '.' | ';'));
    if let Some(context) = directive_context(token) {
        let body = transcript[token_end..].trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | ',' | ';')
        });
        if body.is_empty() {
            bail!("spoken context directive '{token}' is not followed by a transcript");
        }
        Ok(PreparedTranscript { context, body })
    } else {
        Ok(PreparedTranscript {
            context: FormatContext::Auto,
            body: transcript,
        })
    }
}

fn directive_context(token: &str) -> Option<FormatContext> {
    if token.eq_ignore_ascii_case("email") {
        Some(FormatContext::Email)
    } else if token.eq_ignore_ascii_case("message") || token.eq_ignore_ascii_case("text") {
        Some(FormatContext::Message)
    } else if token.eq_ignore_ascii_case("todo") || token.eq_ignore_ascii_case("tasks") {
        Some(FormatContext::Todo)
    } else if token.eq_ignore_ascii_case("note") || token.eq_ignore_ascii_case("notes") {
        Some(FormatContext::Notes)
    } else if token.eq_ignore_ascii_case("plain") {
        Some(FormatContext::Plain)
    } else if token.eq_ignore_ascii_case("verbatim") {
        Some(FormatContext::Verbatim)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_and_removes_spoken_directive() {
        let prepared = prepare("Email: Hi Sam, here is the proposal.", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Email);
        assert_eq!(prepared.body, "Hi Sam, here is the proposal.");
    }

    #[test]
    fn recognizes_directive_alias_case_insensitively() {
        let prepared = prepare("TASKS buy milk and call Alex", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Todo);
        assert_eq!(prepared.body, "buy milk and call Alex");
    }

    #[test]
    fn explicit_context_preserves_a_directive_like_first_word() {
        let prepared = prepare("Message received yesterday.", Some(FormatContext::Plain)).unwrap();
        assert_eq!(prepared.context, FormatContext::Plain);
        assert_eq!(prepared.body, "Message received yesterday.");
    }

    #[test]
    fn explicit_verbatim_context_preserves_directive_like_text() {
        let prepared = prepare("Email Sam exactly this.", Some(FormatContext::Verbatim)).unwrap();
        assert_eq!(prepared.context, FormatContext::Verbatim);
        assert_eq!(prepared.body, "Email Sam exactly this.");
    }

    #[test]
    fn defaults_to_auto_context() {
        let prepared = prepare("First item, milk. Second item, tea.", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Auto);
        assert_eq!(prepared.body, "First item, milk. Second item, tea.");
    }
}
