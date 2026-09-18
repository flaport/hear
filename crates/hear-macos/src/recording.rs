pub use hear_core::dictation::Recorder;

pub fn start(config: &crate::config::HearConfig) -> anyhow::Result<Recorder> {
    Recorder::start(
        config,
        crate::transcriber::helper_path(),
        crate::credentials::stored_api_key,
    )
}
