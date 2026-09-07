use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use sha2::{Digest, Sha256};

pub const DEFAULT_MODEL: &str = "qwen3.5-2b";

const MODEL_REVISION: &str = "7d26695454df6de5fbcce2e58681e62dae06ce43";
const MODEL_BASE_URL: &str = "https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/resolve";

struct Model {
    name: &'static str,
    filename: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const QWEN_3_5_2B: Model = Model {
    name: DEFAULT_MODEL,
    filename: "Qwen_Qwen3.5-2B-Q4_K_M.gguf",
    bytes: 1_396_198_496,
    sha256: "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
};

pub(crate) fn resolve(requested: Option<&str>) -> Result<PathBuf> {
    let requested = requested.map(str::trim).filter(|model| !model.is_empty());
    match requested {
        None | Some(DEFAULT_MODEL | "qwen3.5-2b-q4_k_m") => ensure(&QWEN_3_5_2B),
        Some(requested) => {
            let path = Path::new(requested);
            if path.is_file() {
                Ok(path.to_path_buf())
            } else if looks_like_path(path) {
                bail!("local polishing model does not exist: {}", path.display())
            } else {
                bail!(
                    "unknown local polishing model '{requested}'; choose {DEFAULT_MODEL} or provide a GGUF file path"
                )
            }
        }
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
    let sidecar = directory.join(format!("{}.sha256", model.filename));
    if cached_metadata_matches(&destination, &sidecar, model) {
        return Ok(destination);
    }

    if destination.is_file() {
        eprintln!(
            "Verifying cached local polishing model {}...",
            destination.display()
        );
        if let Err(error) = verify_file(&destination, model) {
            eprintln!("Cached model failed integrity verification ({error}); re-downloading...");
            let _ = fs::remove_file(&destination);
            let _ = fs::remove_file(&sidecar);
        } else {
            write_sidecar(&sidecar, model);
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
        "{MODEL_BASE_URL}/{MODEL_REVISION}/{}?download=true",
        model.filename
    );
    eprintln!(
        "Downloading local polishing model {} ({:.1} GB) to {}...",
        model.name,
        model.bytes as f64 / 1_000_000_000.0,
        destination.display()
    );
    let client = reqwest::blocking::Client::builder()
        .build()
        .context("could not initialize the local model download client")?;
    let mut response = client
        .get(url)
        .send()
        .context("could not download the local polishing model")?
        .error_for_status()
        .context("local polishing model download was rejected")?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .context("could not create a temporary model file")?;
    let (downloaded, checksum) = copy_and_hash(&mut response, &mut temporary)
        .context("could not save the downloaded local polishing model")?;
    temporary
        .flush()
        .context("could not flush the downloaded local polishing model")?;
    verify_metadata(downloaded, &checksum, model)?;
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("could not install local model: {}", destination.display()))?;
    write_sidecar(&sidecar, model);
    Ok(destination)
}

fn cached_metadata_matches(destination: &Path, sidecar: &Path, model: &Model) -> bool {
    fs::metadata(destination).is_ok_and(|metadata| metadata.len() == model.bytes)
        && fs::read_to_string(sidecar)
            .map(|content| content.trim() == model.sha256)
            .unwrap_or(false)
}

fn write_sidecar(sidecar: &Path, model: &Model) {
    let _ = fs::write(sidecar, format!("{}\n", model.sha256));
}

fn verify_file(path: &Path, model: &Model) -> Result<()> {
    let mut file = File::open(path)?;
    let (bytes, checksum) = hash_reader(&mut file)?;
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

fn copy_and_hash(reader: &mut impl Read, writer: &mut impl Write) -> Result<(u64, String)> {
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
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

fn hash_reader(reader: &mut impl Read) -> Result<(u64, String)> {
    copy_and_hash(reader, &mut std::io::sink())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn computes_sha256_while_copying() {
        let input = b"hear local model";
        let mut output = Vec::new();
        let (bytes, checksum) = copy_and_hash(&mut input.as_slice(), &mut output).unwrap();
        assert_eq!(bytes, input.len() as u64);
        assert_eq!(output, input);
        assert_eq!(
            checksum,
            "43101460d399c8f9746f23b9a6029f0c43cdcfe8008cbd6c2b31b9d9620479f0"
        );
    }
}
