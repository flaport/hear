use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) struct Asset {
    pub name: &'static str,
    pub revision: &'static str,
    pub remote: &'static str,
    pub bytes: u64,
    pub hash: &'static str,
}
pub(super) const MODEL: Asset = Asset {
    name: "model.safetensors",
    revision: "e7205b6ee50e654a5ea19f0e9df2b0813b05e921",
    remote: "model.safetensors",
    bytes: 219029196,
    hash: "916ccd2686e9311cb40054893a3c4284393d658825ffc714a276f3e9b152344f",
};
pub(super) const TOKENIZER: Asset = Asset {
    name: "tokenizer.json",
    revision: "00eac05ed3d16bdc3f6b5d598874019c34a89214",
    remote: "tokenizer.json",
    bytes: 245020,
    hash: "f498428e1eafee50492f7be13dc9bfafcfc12e508cd0eb1b01c92ecd5d8c6687",
};
macro_rules! voice {
    ($name:literal, $bytes:literal, $hash:literal) => {
        Asset {
            name: concat!($name, ".safetensors"),
            remote: concat!("embeddings/", $name, ".safetensors"),
            revision: "4e1e0a3e611c51c0b4ed8174fc10f32a54644303",
            bytes: $bytes,
            hash: $hash,
        }
    };
}
const VOICES: &[Asset] = &[
    voice!(
        "alba",
        6195000,
        "d291428b416d6c36a1de7835e51dbe1e334b75e5af512bb18dd23a1047fe8f3b"
    ),
    voice!(
        "marius",
        6195000,
        "b05946f39371f8853902b0486231f2d401c8a9570da9978329120d4ecfd009e9"
    ),
    voice!(
        "javert",
        6195000,
        "580e4b72d4065e906c14e580f97da478c9707bead74b0ef9c049e0ca8de997d9"
    ),
    voice!(
        "jean",
        6195000,
        "397583373a34c22781000a8d33e5cdceca30501368376605539b3e805bc8a837"
    ),
    voice!(
        "fantine",
        6539064,
        "8b6a1c253d39c701e7b70e349796d10b4ce6f35bc58254f0e0caa572f1d41464"
    ),
    voice!(
        "cosette",
        6195000,
        "90f7a535c7c8774b8f5c28b841b0b93e1c55ef45e31f688021ad067fd2ac97f7"
    ),
    voice!(
        "eponine",
        6932280,
        "0bcc4878ad3183ef8bfdbaf9224c5a71f1d18fe944fb494818c37e1f8ea6c7d0"
    ),
    voice!(
        "azelma",
        7964472,
        "1592ba7eb6784302ccaa8d48a7304f14f6fd7503969fb0466efcab607827477c"
    ),
];
pub(super) fn voice(name: &str) -> &'static Asset {
    &VOICES[POCKET_VOICES
        .iter()
        .position(|v| *v == name)
        .expect("validated voice")]
}

pub(super) fn ensure(
    directory: &Path,
    asset: &Asset,
    cancellation: &Cancellation,
) -> Result<PathBuf> {
    let url = format!(
        "https://huggingface.co/kyutai/pocket-tts-without-voice-cloning/resolve/{}/languages/english_2026-09/{}",
        asset.revision, asset.remote
    );
    ensure_from(directory, asset, &url, cancellation)
}

fn ensure_from(
    directory: &Path,
    asset: &Asset,
    url: &str,
    cancellation: &Cancellation,
) -> Result<PathBuf> {
    check_cancelled(cancellation)?;
    let destination = directory.join(asset.name);
    if destination.is_file() {
        let mut file = File::open(&destination).map_err(local)?;
        if verify_copy(&mut file, &mut std::io::sink(), asset, cancellation).is_ok() {
            return Ok(destination);
        }
        check_cancelled(cancellation)?;
    }
    std::fs::create_dir_all(directory).map_err(local)?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(local)?;
    let response = client.get(url).send();
    check_cancelled(cancellation)?;
    let mut response = response.map_err(local)?.error_for_status().map_err(local)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(local)?;
    verify_copy(&mut response, &mut temporary, asset, cancellation)?;
    temporary.flush().map_err(local)?;
    check_cancelled(cancellation)?;
    temporary
        .persist(&destination)
        .map_err(|e| local(e.error))?;
    Ok(destination)
}

fn verify_copy(
    reader: &mut impl Read,
    writer: &mut impl Write,
    asset: &Asset,
    cancellation: &Cancellation,
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let read = reader.read(&mut buffer);
        check_cancelled(cancellation)?;
        let count = read.map_err(local)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > asset.bytes {
            return Err(local(anyhow::anyhow!(
                "{} exceeds expected size",
                asset.name
            )));
        }
        hash.update(&buffer[..count]);
        writer.write_all(&buffer[..count]).map_err(local)?;
    }
    if total != asset.bytes || format!("{:x}", hash.finalize()) != asset.hash {
        return Err(local(anyhow::anyhow!(
            "{} failed size/SHA-256 verification",
            asset.name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_download_preserves_existing_file_and_removes_partial() {
        use std::{net::TcpListener, thread};
        let asset = Asset {
            name: "test",
            revision: "test",
            remote: "test",
            bytes: 3,
            hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test");
        std::fs::write(&path, b"old").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = vec![];
            loop {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nab")
                .unwrap();
        });
        assert!(ensure_from(directory.path(), &asset, &url, &Cancellation::default()).is_err());
        worker.join().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn verifies_content_and_rejects_truncated_oversized_or_corrupt_assets() {
        let asset = Asset {
            name: "test",
            revision: "test",
            remote: "test",
            bytes: 3,
            hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        for bytes in [&b"ab"[..], &b"abd"[..], &b"abcd"[..]] {
            assert!(
                verify_copy(
                    &mut &bytes[..],
                    &mut Vec::new(),
                    &asset,
                    &Cancellation::default()
                )
                .is_err()
            );
        }
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("test"), b"abc").unwrap();
        // A valid cache does not even create an HTTP client or contact the URL.
        assert!(
            ensure_from(
                directory.path(),
                &asset,
                "http://127.0.0.1:1",
                &Cancellation::default()
            )
            .is_ok()
        );
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert!(matches!(
            ensure_from(directory.path(), &asset, "invalid", &cancellation),
            Err(SpeechError::Cancelled)
        ));
    }
}
