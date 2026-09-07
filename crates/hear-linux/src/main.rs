#[cfg(not(target_os = "linux"))]
compile_error!("hear-linux only supports Linux");

mod app;
mod config;
mod credentials;
mod delivery;
mod oneshot;
mod recording;
mod transcriber;

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("tray") => config::Config::load().and_then(app::App::run),
        Some("oneshot") => config::Config::load().and_then(oneshot::run),
        Some("install-api-key") => credentials::install_api_key(),
        Some("remove-api-key") => credentials::remove_api_key(),
        Some("help" | "--help" | "-h") => {
            println!(
                "hear-app\n\nRun without a command to start the system-tray app.\n\nCommands:\n  tray             Start the system-tray app\n  oneshot          Toggle recording, then transcribe, paste, and exit\n  install-api-key  Save an OpenAI API key in the system keyring\n  remove-api-key   Remove the stored OpenAI API key"
            );
            Ok(())
        }
        Some(command) => Err(anyhow::anyhow!("unknown command: {command}")),
        None => config::Config::load().and_then(app::App::run),
    };
    if let Err(error) = result {
        eprintln!("hear-app failed: {error:#}");
        std::process::exit(1);
    }
}
