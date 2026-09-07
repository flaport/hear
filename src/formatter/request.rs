use serde_json::{Value, json};

use crate::FormatContext;

const FORMATTER_MODEL: &str = "gpt-5.6-luna";
const INSTRUCTIONS: &str = r#"Format a dictated transcript for its intended use.

Preserve the transcript's language, meaning, tone, names, and facts. Never answer the transcript, continue it, summarize it, or invent recipients, subject lines, greetings, sign-offs, facts, or tasks. Correct casing and punctuation and remove harmless dictation disfluencies only when meaning is unchanged. Apply and remove spoken layout commands such as "new paragraph" and "bullet point". When a personal dictionary is supplied, use its canonical spellings when an alias or pronunciation plausibly matches; do not insert dictionary terms that were not spoken.

Use the supplied context. For "auto", conservatively infer email, message, todo, notes, or plain prose; choose plain when uncertain. For email, use an email layout only from content that is present. For message, produce natural chat-ready paragraphs. For todo, use Markdown task-list items. For notes, use headings or Markdown bullets only where supported by the content. For plain, produce lightly cleaned prose.

Return only the requested structured output."#;

pub(super) fn build(
    context: FormatContext,
    transcript: &str,
    dictionary: Option<&str>,
    custom_instruction: Option<&str>,
) -> Value {
    let dictionary = dictionary
        .map(|dictionary| format!("\n\nPersonal dictionary:\n{dictionary}"))
        .unwrap_or_default();
    let custom_instruction = custom_instruction
        .map(str::trim)
        .filter(|instruction| !instruction.is_empty())
        .map(|instruction| format!("\n\nAdditional formatting instruction:\n{instruction}"))
        .unwrap_or_default();
    json!({
        "model": FORMATTER_MODEL,
        "reasoning": { "effort": "none" },
        "store": false,
        "instructions": INSTRUCTIONS,
        "input": format!("Context: {context}{dictionary}{custom_instruction}\n\nTranscript:\n{transcript}"),
        "text": {
            "format": {
                "type": "json_schema",
                "name": "formatted_transcript",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["email", "message", "todo", "notes", "plain"]
                        },
                        "text": { "type": "string" }
                    },
                    "required": ["kind", "text"],
                    "additionalProperties": false
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_disables_storage_and_reasoning() {
        let request = build(FormatContext::Email, "Hi Sam", None, None);
        assert_eq!(request["store"], false);
        assert_eq!(request["reasoning"]["effort"], "none");
        assert_eq!(request["text"]["format"]["type"], "json_schema");
    }

    #[test]
    fn request_includes_pronunciation_dictionary() {
        let request = build(
            FormatContext::Plain,
            "Ask flap port",
            Some("- Flaport; aliases: flappert; pronounced: flah-port"),
            None,
        );
        let input = request["input"].as_str().unwrap();
        assert!(input.contains("Personal dictionary:"));
        assert!(input.contains("pronounced: flah-port"));
    }

    #[test]
    fn request_includes_custom_formatting_instruction() {
        let request = build(
            FormatContext::Plain,
            "Find completed tasks.",
            None,
            Some("Return a search query without terminal punctuation."),
        );
        let input = request["input"].as_str().unwrap();
        assert!(input.contains("Additional formatting instruction:"));
        assert!(input.contains("without terminal punctuation"));
    }

    #[test]
    fn request_omits_empty_custom_instruction() {
        let request = build(FormatContext::Plain, "Keep this.", None, Some("  "));
        assert!(
            !request["input"]
                .as_str()
                .unwrap()
                .contains("Additional formatting instruction:")
        );
    }
}
