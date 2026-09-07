use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use sha2::{Digest, Sha256};

const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve";

pub(super) struct Model {
    pub(super) name: &'static str,
    filename: &'static str,
    bytes: u64,
    sha256: &'static str,
    pub(super) multilingual: bool,
}

const MODELS: &[Model] = &[
    Model {
        name: "tiny.en",
        filename: "ggml-tiny.en.bin",
        bytes: 77_704_715,
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        multilingual: false,
    },
    Model {
        name: "base.en",
        filename: "ggml-base.en.bin",
        bytes: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        multilingual: false,
    },
    Model {
        name: "small.en",
        filename: "ggml-small.en.bin",
        bytes: 487_614_201,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        multilingual: false,
    },
    Model {
        name: "medium.en",
        filename: "ggml-medium.en.bin",
        bytes: 1_533_774_781,
        sha256: "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356",
        multilingual: false,
    },
    Model {
        name: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        bytes: 1_624_555_275,
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        multilingual: true,
    },
];

pub(super) fn find(name: &str) -> Result<&'static Model> {
    MODELS.iter().find(|model| model.name == name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown Whisper model '{name}'; choose tiny.en, base.en, small.en, medium.en, or large-v3-turbo"
        )
    })
}

pub(super) fn ensure(model: &Model) -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine the platform cache directory")?;
    let directory = base.cache_dir().join("hear").join("models");
    let destination = directory.join(model.filename);
    if destination.is_file() {
        verify_file(&destination, model).with_context(|| {
            format!(
                "cached Whisper model failed integrity verification: {}; remove it and try again",
                destination.display()
            )
        })?;
        return Ok(destination);
    }

    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create the Whisper model cache: {}",
            directory.display()
        )
    })?;
    let url = format!("{MODEL_BASE_URL}/{MODEL_REVISION}/{}", model.filename);
    eprintln!(
        "Downloading Whisper model {} to {}...",
        model.name,
        destination.display()
    );
    let client = reqwest::blocking::Client::builder()
        .build()
        .context("could not initialize the model download client")?;
    let mut response = client
        .get(url)
        .send()
        .context("could not download the Whisper model")?
        .error_for_status()
        .context("Whisper model download was rejected")?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .context("could not create a temporary model file")?;
    let (downloaded, checksum) = copy_and_hash(&mut response, &mut temporary)
        .context("could not save the downloaded Whisper model")?;
    temporary
        .flush()
        .context("could not flush the downloaded Whisper model")?;
    verify_metadata(downloaded, &checksum, model)?;
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("could not install Whisper model: {}", destination.display()))?;
    Ok(destination)
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
    fn maps_supported_model_names_and_capabilities() {
        assert_eq!(find("tiny.en").unwrap().filename, "ggml-tiny.en.bin");
        assert!(!find("tiny.en").unwrap().multilingual);
        assert!(find("large-v3-turbo").unwrap().multilingual);
        assert!(find("surprise").is_err());
    }

    #[test]
    fn computes_sha256_while_copying() {
        let input = b"hear model";
        let mut output = Vec::new();
        let (bytes, checksum) = copy_and_hash(&mut input.as_slice(), &mut output).unwrap();
        assert_eq!(bytes, input.len() as u64);
        assert_eq!(output, input);
        assert_eq!(
            checksum,
            "93ac1d2aa8c846219d68a880b7daaccea684f95f4ad617e8dd8212a4b3fe939e"
        );
    }
}
