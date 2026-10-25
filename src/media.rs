use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaInfo {
    pub bytes: u64,
    pub sha256: String,
    pub duration_seconds: f64,
    pub width: u64,
    pub height: u64,
    pub video_codec: String,
    pub audio_codec: Option<String>,
}

pub fn check_video(path: &Path) -> Result<MediaInfo> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mp4")),
        "Video must use the .mp4 extension"
    );
    let meta = fs::metadata(path).context("Cannot read video file")?;
    ensure!(
        meta.is_file() && meta.len() >= 12,
        "Video must be a nonempty regular MP4 file"
    );
    let mut file = File::open(path).context("Cannot open video file")?;
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    ensure!(
        &header[4..8] == b"ftyp" && &header[8..12] != b"qt  ",
        "File has no supported MP4 file-type header"
    );
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            anyhow::anyhow!(
                "ffprobe is required for MP4 validation; install FFmpeg and add ffprobe to PATH"
            )
        })?;
    ensure!(
        output.status.success(),
        "ffprobe rejected the video; repair or re-encode it before uploading"
    );
    let probe: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| anyhow::anyhow!("ffprobe returned invalid metadata JSON"))?;
    let sha256 = hash_file(path)?;
    parse_probe(&probe, meta.len(), sha256)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut reader = File::open(path).context("Cannot open file for fingerprinting")?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let len = reader.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        sha.update(&buffer[..len]);
    }
    Ok(hex::encode(sha.finalize()))
}

pub fn check_cover(path: &Path) -> Result<CoverInfo> {
    let meta = fs::metadata(path).context("Cannot read cover file")?;
    ensure!(
        meta.is_file() && meta.len() > 0 && meta.len() <= 10 * 1024 * 1024,
        "Cover must be a regular image file of at most 10 MiB"
    );
    let bytes = fs::read(path).context("Cannot open cover image")?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else {
        anyhow::bail!("Cover must contain JPEG or PNG image data");
    };
    let size = imagesize::blob_size(&bytes)
        .map_err(|_| anyhow::anyhow!("Cannot read cover dimensions"))?;
    ensure!(
        size.width > 0 && size.height > 0,
        "Cover has invalid dimensions"
    );
    Ok(CoverInfo {
        bytes,
        mime,
        width: size.width,
        height: size.height,
    })
}
pub struct CoverInfo {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: usize,
    pub height: usize,
}
fn parse_probe(value: &Value, bytes: u64, sha256: String) -> Result<MediaInfo> {
    let format = value["format"]["format_name"].as_str().unwrap_or_default();
    ensure!(
        format.split(',').any(|f| f == "mp4"),
        "ffprobe did not detect an MP4 container"
    );
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .context("Video has no valid duration")?;
    ensure!(
        duration.is_finite() && duration > 0.0,
        "Video has no positive finite duration"
    );
    let streams = value["streams"]
        .as_array()
        .context("Video has no streams")?;
    let video = streams
        .iter()
        .find(|s| {
            s["codec_type"] == "video"
                && s.pointer("/disposition/attached_pic") != Some(&serde_json::json!(1))
        })
        .context("MP4 has no video stream")?;
    let width = video["width"]
        .as_u64()
        .filter(|n| *n > 0)
        .context("Video has invalid width")?;
    let height = video["height"]
        .as_u64()
        .filter(|n| *n > 0)
        .context("Video has invalid height")?;
    let video_codec = video["codec_name"]
        .as_str()
        .filter(|s| *s != "unknown")
        .context("Video codec is unknown")?
        .into();
    let audio_codec = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .and_then(|s| s["codec_name"].as_str())
        .map(str::to_owned);
    Ok(MediaInfo {
        bytes,
        sha256,
        duration_seconds: duration,
        width,
        height,
        video_codec,
        audio_codec,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn probe_requires_real_video_and_finite_duration() {
        let valid = json!({"format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"3.5"},"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080},{"codec_type":"audio","codec_name":"aac"}]});
        let info = parse_probe(&valid, 100, "hash".into()).unwrap();
        assert_eq!(info.width, 1920);
        assert_eq!(info.duration_seconds, 3.5);
        assert_eq!(info.audio_codec.as_deref(), Some("aac"));
        let mut invalid = valid.clone();
        invalid["format"]["duration"] = json!("NaN");
        assert!(parse_probe(&invalid, 100, "hash".into()).is_err());
        invalid = valid.clone();
        invalid["streams"] = json!([{"codec_type":"audio","codec_name":"aac"}]);
        assert!(parse_probe(&invalid, 100, "hash".into()).is_err());
    }
    #[test]
    fn renamed_non_mp4_and_invalid_cover_fail_before_network() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.mp4");
        fs::write(&fake, b"this is not an MP4").unwrap();
        assert!(check_video(&fake).is_err());
        assert!(check_cover(&fake).is_err());
    }
    #[test]
    fn detects_png_cover_from_content() {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.bin");
        fs::write(&path, png).unwrap();
        let info = check_cover(&path).unwrap();
        assert_eq!(info.mime, "image/png");
        assert_eq!((info.width, info.height), (1, 1));
    }
}
