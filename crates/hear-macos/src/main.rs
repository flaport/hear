#[cfg(not(target_os = "macos"))]
compile_error!("hear-macos only supports macOS");

mod app;
mod delivery;
mod recording;
mod transcriber;

fn main() {
    if let Err(error) = app::App::run() {
        eprintln!("hear-macos failed: {error:#}");
        std::process::exit(1);
    }
}
