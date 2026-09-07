#[cfg(not(target_os = "linux"))]
compile_error!("hear-linux only supports Linux");

mod app;
mod credentials;
mod delivery;
mod recording;
mod transcriber;

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("install-api-key") => credentials::install_api_key(),
        Some("remove-api-key") => credentials::remove_api_key(),
        Some("help" | "--help" | "-h") => {
            println!(
                "hear-linux\n\nCommands:\n  install-api-key  Save an OpenAI API key in the system keyring\n  remove-api-key   Remove the stored OpenAI API key"
            );
            Ok(())
        }
        Some(command) => Err(anyhow::anyhow!("unknown command: {command}")),
        None => app::App::run(),
    };
    if let Err(error) = result {
        eprintln!("hear-linux failed: {error:#}");
        std::process::exit(1);
    }
}
