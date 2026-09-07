use std::io::{self, Read};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct Request {
    instructions: String,
    input: String,
    model: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut body = String::new();
    io::stdin()
        .read_to_string(&mut body)
        .context("could not read the local polishing request")?;
    let request: Request =
        serde_json::from_str(&body).context("could not parse the local polishing request")?;
    let polished = hear_local_polish::polish(
        &request.instructions,
        &request.input,
        request.model.as_deref(),
    )?;
    println!("{polished}");
    Ok(())
}
