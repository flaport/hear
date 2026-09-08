use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use sha2::{Digest, Sha256};

pub const DEFAULT_MODEL: &str = "qwen3.5-2b";

const MODEL_BASE_URL: &str = "https://huggingface.co";

struct Model {
    name: &'static str,
    repository: &'static str,
    revision: &'static str,
    filename: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const QWEN_3_5_2B: Model = Model {
    name: DEFAULT_MODEL,
    repository: "bartowski/Qwen_Qwen3.5-2B-GGUF",
    revision: "7d26695454df6de5fbcce2e58681e62dae06ce43",
    filename: "Qwen_Qwen3.5-2B-Q4_K_M.gguf",
    bytes: 1_396_198_496,
    sha256: "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
};

const QWEN_3_5_0_8B: Model = Model {
    name: "qwen3.5-0.8b",
    repository: "bartowski/Qwen_Qwen3.5-0.8B-GGUF",
    revision: "f36b1ea49a332ede8fe5f389bbf5b3575ef71f48",
    filename: "Qwen_Qwen3.5-0.8B-Q4_K_M.gguf",
    bytes: 579_615_840,
    sha256: "fb044e93939a70469c905781334f5de1e6c8b608ced6cbc8c9249bd4127d9526",
};

pub(crate) fn resolve(requested: Option<&str>) -> Result<PathBuf> {
    let requested = requested.map(str::trim).filter(|model| !model.is_empty());
    if let Some(model) = builtin_model(requested) {
        return ensure(model);
    }
    let requested = requested.context("local polishing model name is empty")?;
    let path = Path::new(requested);
    if path.is_file() {
        Ok(path.to_path_buf())
    } else if looks_like_path(path) {
        bail!("local polishing model does not exist: {}", path.display())
    } else {
        bail!(
            "unknown local polishing model '{requested}'; choose {DEFAULT_MODEL}, qwen3.5-0.8b, or provide a GGUF file path"
        )
    }
}

fn builtin_model(requested: Option<&str>) -> Option<&'static Model> {
    match requested {
        None | Some(DEFAULT_MODEL | "qwen3.5-2b-q4_k_m") => Some(&QWEN_3_5_2B),
        Some("qwen3.5-0.8b" | "qwen3.5-0.8b-q4_k_m") => Some(&QWEN_3_5_0_8B),
        Some(_) => None,
    }
}

fn looks_like_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().count() > 1
        || path
            .extension()
            .is_some_and(|extension| extension == "gguf")
}

fn ensure(model: &Model) -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine the platform cache directory")?;
    let directory = base.cache_dir().join("hear").join("models");
    let destination = directory.join(model.filename);

    if destination.is_file() {
        eprintln!(
            "Verifying cached local polishing model {}...",
            destination.display()
        );
        if let Err(error) = verify_file(&destination, model) {
            eprintln!("Cached model failed integrity verification ({error}); re-downloading...");
            let _ = fs::remove_file(&destination);
        } else {
            return Ok(destination);
        }
    }

    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create the local model cache: {}",
            directory.display()
        )
    })?;
    let url = format!(
        "{MODEL_BASE_URL}/{}/resolve/{}/{}?download=true",
        model.repository, model.revision, model.filename
    );
    eprintln!(
        "Downloading local polishing model {} ({})...",
        model.name,
        display_size(model.bytes)
    );
    eprintln!("  source: {} at {}", model.repository, model.revision);
    eprintln!("  destination: {}", destination.display());
    eprintln!("  expected SHA-256: {}", model.sha256);
    let client = reqwest::blocking::Client::builder()
        .build()
        .context("could not initialize the local model download client")?;
    let mut response = client
        .get(url)
        .send()
        .context("could not download the local polishing model")?
        .error_for_status()
        .context("local polishing model download was rejected")?;
    if let Some(bytes) = response.content_length() {
        eprintln!("  server content length: {}", display_size(bytes));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .context("could not create a temporary model file")?;
    let mut next_progress = 10_u64;
    let (downloaded, checksum) = copy_and_hash(&mut response, &mut temporary, |downloaded| {
        let percent = downloaded.saturating_mul(100) / model.bytes;
        if percent >= next_progress {
            eprintln!(
                "  downloaded {} / {} ({percent}%)",
                display_size(downloaded),
                display_size(model.bytes)
            );
            next_progress += 10;
        }
    })
    .context("could not save the downloaded local polishing model")?;
    temporary
        .flush()
        .context("could not flush the downloaded local polishing model")?;
    eprintln!("Verifying downloaded model size and SHA-256...");
    verify_metadata(downloaded, &checksum, model)?;
    eprintln!("Model verification passed.");
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("could not install local model: {}", destination.display()))?;
    eprintln!(
        "Installed local polishing model at {}.",
        destination.display()
    );
    Ok(destination)
}

fn display_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

fn verify_file(path: &Path, model: &Model) -> Result<()> {
    let mut file = File::open(path)?;
    let (bytes, checksum) = copy_and_hash(&mut file, &mut std::io::sink(), |_| {})?;
    verify_metadata(bytes, &checksum, model)
}

fn verify_metadata(bytes: u64, checksum: &str, model: &Model) -> Result<()> {
    if bytes != model.bytes {
        bail!("expected {} bytes but found {bytes} bytes", model.bytes);
    }
    if checksum != model.sha256 {
        bail!("SHA-256 checksum does not match the model catalog");
    }
    Ok(())
}

fn copy_and_hash(
    reader: &mut impl Read,
    writer: &mut impl Write,
    mut progress: impl FnMut(u64),
) -> Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        bytes += read as u64;
        progress(bytes);
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_model_verification_rejects_same_size_corruption_and_truncation() {
        let model = Model {
            name: "test",
            repository: "test",
            revision: "test",
            filename: "test.gguf",
            bytes: 16,
            sha256: "43101460d399c8f9746f23b9a6029f0c43cdcfe8008cbd6c2b31b9d9620479f0",
        };
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), b"hear local model").unwrap();
        verify_file(file.path(), &model).unwrap();
        fs::write(file.path(), b"evil local model").unwrap();
        assert!(verify_file(file.path(), &model).is_err());
        fs::write(file.path(), b"x").unwrap();
        assert!(verify_file(file.path(), &model).is_err());
    }

    #[test]
    fn accepts_a_custom_gguf_path() {
        let file = tempfile::Builder::new().suffix(".gguf").tempfile().unwrap();
        assert_eq!(resolve(file.path().to_str()).unwrap(), file.path());
    }

    #[test]
    fn rejects_unknown_models_and_missing_paths() {
        assert!(resolve(Some("surprise-model")).is_err());
        assert!(resolve(Some("/missing/model.gguf")).is_err());
    }

    #[test]
    fn maps_builtin_names_and_quantization_aliases() {
        assert_eq!(builtin_model(None).unwrap().name, DEFAULT_MODEL);
        assert_eq!(
            builtin_model(Some("qwen3.5-2b-q4_k_m")).unwrap().name,
            DEFAULT_MODEL
        );
        assert_eq!(
            builtin_model(Some("qwen3.5-0.8b")).unwrap().name,
            "qwen3.5-0.8b"
        );
        assert_eq!(
            builtin_model(Some("qwen3.5-0.8b-q4_k_m")).unwrap().name,
            "qwen3.5-0.8b"
        );
    }

    #[test]
    fn formats_catalog_sizes_for_download_progress() {
        assert_eq!(display_size(QWEN_3_5_0_8B.bytes), "579.6 MB");
        assert_eq!(display_size(QWEN_3_5_2B.bytes), "1.40 GB");
    }

    #[test]
    fn computes_sha256_while_copying() {
        let input = b"hear local model";
        let mut output = Vec::new();
        let mut progress = Vec::new();
        let (bytes, checksum) = copy_and_hash(&mut input.as_slice(), &mut output, |bytes| {
            progress.push(bytes)
        })
        .unwrap();
        assert_eq!(bytes, input.len() as u64);
        assert_eq!(output, input);
        assert_eq!(progress, [input.len() as u64]);
        assert_eq!(
            checksum,
            "43101460d399c8f9746f23b9a6029f0c43cdcfe8008cbd6c2b31b9d9620479f0"
        );
    }
}
