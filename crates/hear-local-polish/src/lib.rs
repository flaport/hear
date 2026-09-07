//! In-process local transcript polishing for hear.

mod model;

use std::num::NonZeroU32;

use anyhow::{Context, Result, bail};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{LogOptions, send_logs_to_tracing};
use serde::Deserialize;

pub use model::DEFAULT_MODEL;

const CONTEXT_TOKENS: u32 = 8_192;
const MAX_OUTPUT_TOKENS: usize = 4_096;
const OUTPUT_GRAMMAR: &str = r#"
root ::= "{" ws "\"kind\"" ws ":" ws kind "," ws "\"text\"" ws ":" ws string ws "}"
kind ::= "\"email\"" | "\"message\"" | "\"todo\"" | "\"notes\"" | "\"plain\""
string ::= "\"" ([^"\\] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F]))* "\""
ws ::= [ \t\n\r]*
"#;

#[derive(Debug, Deserialize)]
struct Polished {
    #[allow(dead_code)]
    kind: String,
    text: String,
}

/// Polish text locally with a GGUF model.
///
/// When `requested_model` is absent, the recommended Qwen model is downloaded
/// to the platform cache on first use. A path to another GGUF model is also
/// accepted.
pub fn polish(instructions: &str, input: &str, requested_model: Option<&str>) -> Result<String> {
    let model_path = model::resolve(requested_model)?;
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
    let backend = LlamaBackend::init().context("could not initialize local model inference")?;
    let model_params = local_model_params();
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .with_context(|| format!("could not load local model {}", model_path.display()))?;
    let template = model
        .chat_template(None)
        .context("local model does not contain a chat template")?;
    let messages = [
        LlamaChatMessage::new("system".to_owned(), instructions.to_owned())
            .context("formatting instructions contain a null byte")?,
        LlamaChatMessage::new("user".to_owned(), input.to_owned())
            .context("transcript contains a null byte")?,
    ];
    let prompt = model
        .apply_chat_template(&template, &messages, true)
        .context("could not apply the local model chat template")?;
    let output = generate(&backend, &model, &prompt)?;
    parse_output(&output)
}

fn local_model_params() -> LlamaModelParams {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        LlamaModelParams::default().with_n_gpu_layers(u32::MAX)
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        LlamaModelParams::default()
    }
}

fn generate(backend: &LlamaBackend, model: &LlamaModel, prompt: &str) -> Result<String> {
    let tokens = model
        .str_to_token(prompt, AddBos::Never)
        .context("could not tokenize the local polishing prompt")?;
    if tokens.is_empty() {
        bail!("local polishing prompt produced no tokens");
    }
    if tokens.len() + 2 > CONTEXT_TOKENS as usize {
        bail!(
            "transcript is too long for local polishing ({} prompt tokens; maximum is {})",
            tokens.len(),
            CONTEXT_TOKENS - 2
        );
    }

    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(CONTEXT_TOKENS))
        .with_n_batch(CONTEXT_TOKENS);
    let mut context = model
        .new_context(backend, context_params)
        .context("could not create a local model context")?;
    let mut batch = LlamaBatch::new(tokens.len().max(1), 1);
    let last = i32::try_from(tokens.len() - 1).context("local polishing prompt is too long")?;
    for (position, token) in (0_i32..).zip(tokens) {
        batch
            .add(token, position, &[0], position == last)
            .context("could not add the local polishing prompt to the inference batch")?;
    }
    context
        .decode(&mut batch)
        .context("could not evaluate the local polishing prompt")?;

    let mut grammar = LlamaSampler::grammar(model, OUTPUT_GRAMMAR, "root")
        .context("could not initialize structured local output")?;
    let mut selector = LlamaSampler::greedy();
    let max_output = MAX_OUTPUT_TOKENS.min(CONTEXT_TOKENS as usize - batch.n_tokens() as usize);
    let mut position = batch.n_tokens();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();

    for _ in 0..max_output {
        let mut candidates = context.token_data_array_ith(batch.n_tokens() - 1);
        grammar.apply(&mut candidates);
        selector.apply(&mut candidates);
        let token = candidates
            .selected_token()
            .context("local model sampling did not select a token")?;
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .context("could not decode local model output")?;
        grammar
            .try_accept(token)
            .context("local output violated its structured grammar")?;
        selector.accept(token);
        output.push_str(&piece);
        if serde_json::from_str::<Polished>(output.trim()).is_ok() {
            break;
        }
        batch.clear();
        batch
            .add(token, position, &[0], true)
            .context("could not add a generated token to the inference batch")?;
        position += 1;
        context
            .decode(&mut batch)
            .context("could not evaluate a generated local model token")?;
    }

    if output.trim().is_empty() {
        bail!("local polishing model returned an empty response");
    }
    Ok(output)
}

fn parse_output(output: &str) -> Result<String> {
    let polished: Polished = serde_json::from_str(output.trim())
        .context("local polishing model returned invalid structured output")?;
    if polished.text.trim().is_empty() {
        bail!("local polishing model returned an empty transcript");
    }
    Ok(polished.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_output() {
        assert_eq!(
            parse_output(r#"{"kind":"plain","text":"Hello, world."}"#).unwrap(),
            "Hello, world."
        );
    }

    #[test]
    fn rejects_empty_or_invalid_output() {
        assert!(parse_output("not JSON").is_err());
        assert!(parse_output(r#"{"kind":"plain","text":""}"#).is_err());
    }

    #[test]
    #[ignore = "downloads and runs the local polishing model"]
    fn live_local_polishing() {
        let formatted = polish(
            "Correct punctuation. Return JSON with kind and text.",
            "hello world this is a test",
            None,
        )
        .unwrap();
        assert!(!formatted.is_empty());
    }
}
