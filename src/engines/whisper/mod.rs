mod audio;
mod model;

use std::path::Path;

use anyhow::{Context, Result, bail};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub fn transcribe(
    input: &Path,
    model_name: &str,
    language: &str,
    vocabulary: &[String],
) -> Result<String> {
    let model = model::find(model_name)?;
    let language = resolve_language(model, language)?;
    let model_path = model::ensure(model)?;
    let samples = audio::load(input)?;
    eprintln!("Running whisper.cpp model {}...", model.name);

    let model_path = model_path
        .to_str()
        .context("Whisper model path is not valid UTF-8")?;
    let context = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .context("could not load the whisper.cpp model")?;
    let mut state = context
        .create_state()
        .context("could not initialize whisper.cpp")?;
    let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    parameters.set_language(language);
    parameters.set_translate(false);
    parameters.set_n_threads(
        std::thread::available_parallelism()
            .map(|threads| threads.get().min(8) as i32)
            .unwrap_or(4),
    );
    parameters.set_print_special(false);
    parameters.set_print_progress(false);
    parameters.set_print_realtime(false);
    parameters.set_print_timestamps(false);
    if !vocabulary.is_empty() {
        parameters.set_initial_prompt(&format!("Preferred spellings: {}.", vocabulary.join(", ")));
    }

    state
        .full(parameters, &samples)
        .context("whisper.cpp could not transcribe the audio")?;
    let transcript = state
        .as_iter()
        .map(|segment| segment.to_string())
        .collect::<String>();
    Ok(transcript.trim().to_owned())
}

fn resolve_language<'a>(model: &model::Model, language: &'a str) -> Result<Option<&'a str>> {
    let language = language.trim();
    if language.is_empty() {
        bail!("Whisper language cannot be empty");
    }
    if language.eq_ignore_ascii_case("auto") {
        return if model.multilingual {
            Ok(None)
        } else {
            Ok(Some("en"))
        };
    }
    if !model.multilingual && !language.eq_ignore_ascii_case("en") {
        bail!(
            "Whisper model '{}' only supports English; choose large-v3-turbo for --language {language}",
            model.name
        );
    }
    Ok(Some(language))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_language_from_model_capabilities() {
        let english = model::find("tiny.en").unwrap();
        assert_eq!(resolve_language(english, "auto").unwrap(), Some("en"));
        assert!(resolve_language(english, "de").is_err());

        let multilingual = model::find("large-v3-turbo").unwrap();
        assert_eq!(resolve_language(multilingual, "auto").unwrap(), None);
        assert_eq!(resolve_language(multilingual, "de").unwrap(), Some("de"));
    }
}
