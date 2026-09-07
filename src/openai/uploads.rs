use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use crate::ffmpeg::{require_ffmpeg, run_ffmpeg};

const MAX_UPLOAD_BYTES: u64 = 25_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialPlan {
    Direct,
    Convert,
    Split,
}

pub(super) struct PreparedUploads {
    paths: Vec<PathBuf>,
    _temporary_files: Option<TempDir>,
}

impl PreparedUploads {
    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

pub(super) fn prepare(input: &Path) -> Result<PreparedUploads> {
    let size = file_size(input)?;
    match initial_plan(is_supported(input), size) {
        InitialPlan::Direct => Ok(PreparedUploads {
            paths: vec![input.to_path_buf()],
            _temporary_files: None,
        }),
        InitialPlan::Convert => convert(input),
        InitialPlan::Split => split(input, true),
    }
}

fn convert(input: &Path) -> Result<PreparedUploads> {
    require_ffmpeg("converting this audio format for OpenAI")?;
    let directory = temporary_directory()?;
    let converted = directory.path().join("converted.mp3");
    eprintln!(
        "Converting unsupported input format to MP3 with FFmpeg: {}",
        input.display()
    );
    run_ffmpeg(
        input,
        &["-vn", "-ac", "1", "-ar", "16000", "-b:a", "64k"],
        &converted,
        "convert the audio to MP3",
    )?;

    if !exceeds_limit(file_size(&converted)?) {
        return Ok(PreparedUploads {
            paths: vec![converted],
            _temporary_files: Some(directory),
        });
    }

    eprintln!(
        "Warning: converting {} produced an upload larger than OpenAI's 25 MB limit; splitting it with FFmpeg.",
        input.display()
    );
    split_in_directory(&converted, directory)
}

fn split(input: &Path, warn_about_source: bool) -> Result<PreparedUploads> {
    require_ffmpeg("compressing or splitting an audio file larger than 25 MB")?;
    if warn_about_source {
        eprintln!(
            "Warning: {} is larger than OpenAI's 25 MB upload limit; compressing and splitting it with FFmpeg.",
            input.display()
        );
    }
    split_in_directory(input, temporary_directory()?)
}

fn split_in_directory(input: &Path, directory: TempDir) -> Result<PreparedUploads> {
    let pattern = directory.path().join("part-%04d.mp3");
    run_ffmpeg(
        input,
        &[
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-b:a",
            "32k",
            "-f",
            "segment",
            "-segment_time",
            "2700",
            "-reset_timestamps",
            "1",
        ],
        &pattern,
        "compress and split the audio",
    )?;
    let mut paths = fs::read_dir(directory.path())
        .context("could not read temporary audio chunks")?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "mp3"))
        .filter(|path| path.file_name().is_some_and(|name| name != "converted.mp3"))
        .collect::<Vec<_>>();
    paths.sort();
    validate_outputs(&paths)?;
    Ok(PreparedUploads {
        paths,
        _temporary_files: Some(directory),
    })
}

fn validate_outputs(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        bail!("FFmpeg produced no audio to upload");
    }
    for path in paths {
        let size = file_size(path)?;
        if exceeds_limit(size) {
            bail!(
                "FFmpeg output still exceeds OpenAI's 25 MB limit: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn temporary_directory() -> Result<TempDir> {
    tempfile::Builder::new()
        .prefix("hear-openai-")
        .tempdir()
        .context("could not create temporary audio directory")
}

fn file_size(path: &Path) -> Result<u64> {
    Ok(fs::metadata(path)
        .with_context(|| format!("could not inspect audio file: {}", path.display()))?
        .len())
}

fn initial_plan(supported: bool, size: u64) -> InitialPlan {
    if size > MAX_UPLOAD_BYTES {
        InitialPlan::Split
    } else if supported {
        InitialPlan::Direct
    } else {
        InitialPlan::Convert
    }
}

fn exceeds_limit(size: u64) -> bool {
    size > MAX_UPLOAD_BYTES
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "mp4" | "mpeg" | "mpga" | "m4a" | "wav" | "webm"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_openai_formats_case_insensitively() {
        assert!(is_supported(Path::new("message.M4A")));
        assert!(is_supported(Path::new("message.webm")));
        assert!(!is_supported(Path::new("message.flac")));
    }

    #[test]
    fn plans_direct_convert_and_split_uploads() {
        assert_eq!(initial_plan(true, 1_000), InitialPlan::Direct);
        assert_eq!(initial_plan(false, 1_000), InitialPlan::Convert);
        assert_eq!(initial_plan(true, MAX_UPLOAD_BYTES + 1), InitialPlan::Split);
        assert_eq!(
            initial_plan(false, MAX_UPLOAD_BYTES + 1),
            InitialPlan::Split
        );
    }

    #[test]
    fn converted_output_is_rechecked_against_limit() {
        assert!(!exceeds_limit(MAX_UPLOAD_BYTES));
        assert!(exceeds_limit(MAX_UPLOAD_BYTES + 1));
    }
}
