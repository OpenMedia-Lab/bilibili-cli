use crate::{
    api::{self, Api},
    media::MediaInfo,
    storage::{self, Lock},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
pub struct UploadReceipt {
    pub schema_version: u32,
    pub owner_mid: u64,
    pub media: MediaInfo,
    pub state: String,
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cid: Option<u64>,
    #[serde(default)]
    pub source_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bucket: Option<Bucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upload_id: Option<String>,
    #[serde(default)]
    confirmed_parts: u64,
    #[serde(default)]
    confirmed_sha256: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Bucket {
    chunk_size: u64,
    auth: String,
    endpoint: String,
    biz_id: u64,
    upos_uri: String,
}

impl UploadReceipt {
    pub fn load(path: &Path) -> Result<Self> {
        let receipt: Self = storage::read_json(path)?;
        ensure!(
            receipt.schema_version == 1,
            "Unsupported upload receipt version"
        );
        Ok(receipt)
    }
    pub fn summary(&self) -> Value {
        json!({"state":self.state,"filename":self.filename,"cid":self.cid,"owner_mid":self.owner_mid,"sha256":self.media.sha256,"bytes":self.media.bytes,"confirmed_parts":self.confirmed_parts})
    }
}

impl Api {
    pub fn upload(
        &self,
        path: &Path,
        media: &MediaInfo,
        receipt_path: &Path,
        resume: bool,
    ) -> Result<UploadReceipt> {
        let _lock = Lock::acquire(receipt_path)?;
        let owner_mid = self.account_id()?;
        let mut receipt = if receipt_path.exists() {
            let receipt = UploadReceipt::load(receipt_path)?;
            ensure!(
                receipt.owner_mid == owner_mid,
                "Upload receipt belongs to another account"
            );
            ensure!(
                receipt.media.sha256 == media.sha256 && receipt.media.bytes == media.bytes,
                "Upload receipt does not match this file; choose a new receipt for new content"
            );
            if receipt.state == "complete" {
                return Ok(receipt);
            }
            ensure!(
                resume,
                "Incomplete upload receipt exists; use --resume to continue the same object"
            );
            ensure!(
                ["preparing", "uploading", "finalizing"].contains(&receipt.state.as_str()),
                "Unknown upload state"
            );
            receipt
        } else {
            ensure!(!resume, "No upload receipt exists to resume");
            let source_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Video filename must be UTF-8")?
                .to_owned();
            let receipt = UploadReceipt {
                schema_version: 1,
                owner_mid,
                media: media.clone(),
                state: "preparing".into(),
                filename: None,
                cid: None,
                source_name,
                bucket: None,
                upload_id: None,
                confirmed_parts: 0,
                confirmed_sha256: Vec::new(),
            };
            storage::write_json(receipt_path, &receipt, false)?;
            receipt
        };
        ensure!(media.bytes > 0, "Cannot upload an empty file");
        if receipt.bucket.is_none() {
            let value = api::response(
                self.request(reqwest::Method::GET, true, "/preupload", true)?
                    .query(&[
                        ("name", receipt.source_name.clone()),
                        ("size", media.bytes.to_string()),
                        ("r", "upos".into()),
                        ("profile", "ugcupos/bup".into()),
                        ("ssl", "0".into()),
                        ("version", "2.14.0".into()),
                        ("build", "2140000".into()),
                        ("zone", "cs".into()),
                        ("upcdn", "bldsa".into()),
                        ("probe_version", "20221109".into()),
                    ]),
                "preupload",
            )?;
            let bucket: Bucket = serde_json::from_value(value).map_err(|_| anyhow::anyhow!("Preupload response has no supported UPOS bucket; session/permissions/API may have changed"))?;
            self.upload_url(&bucket)?;
            ensure!(
                (1..=64 * 1024 * 1024).contains(&bucket.chunk_size),
                "UPOS chunk size is outside the supported 1..64 MiB range"
            );
            receipt.bucket = Some(bucket);
            storage::write_json(receipt_path, &receipt, true)?;
        }
        let bucket = receipt.bucket.clone().context("Missing upload bucket")?;
        let url = self.upload_url(&bucket)?;
        ensure!(
            (1..=64 * 1024 * 1024).contains(&bucket.chunk_size),
            "Invalid stored chunk size"
        );
        let auth = self.upos_auth(&bucket.auth)?;
        if receipt.upload_id.is_none() {
            let response = api::response(
                self.http
                    .post(url.clone())
                    .query(&[("uploads", ""), ("output", "json")])
                    .header("X-Upos-Auth", auth.clone()),
                "initiate upload",
            )?;
            receipt.upload_id = Some(
                response["upload_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("UPOS response has no upload ID")?
                    .into(),
            );
            receipt.state = "uploading".into();
            storage::write_json(receipt_path, &receipt, true)?;
        }
        let upload_id = receipt.upload_id.clone().context("Missing upload ID")?;
        let chunks = media.bytes.div_ceil(bucket.chunk_size);
        ensure!(
            receipt.confirmed_parts <= chunks
                && receipt.confirmed_sha256.len() as u64 <= receipt.confirmed_parts,
            "Invalid uploaded part count in receipt"
        );
        let mut file = File::open(path).context("Cannot open video")?;
        let mut hasher = Sha256::new();
        for chunk in 0..chunks {
            let start = chunk * bucket.chunk_size;
            let size = bucket.chunk_size.min(media.bytes - start);
            let mut bytes = vec![0; size as usize];
            file.read_exact(&mut bytes)
                .context("Video changed or became unreadable during upload")?;
            hasher.update(&bytes);
            let part_sha256 = hex::encode(Sha256::digest(&bytes));
            // A count alone cannot prove which bytes were confirmed before a crash
            // or a source-file mutation. Re-PUT the same part if its bytes differ.
            if chunk < receipt.confirmed_parts
                && receipt.confirmed_sha256.get(chunk as usize) == Some(&part_sha256)
            {
                continue;
            }
            // Persist uncertainty before replacing an acknowledged remote part.
            // If its response is lost, the old hash must never authorize a skip.
            if let Some(previous) = receipt.confirmed_sha256.get_mut(chunk as usize) {
                previous.clear();
                storage::write_json(receipt_path, &receipt, true)?;
            }
            let params = [
                ("uploadId", upload_id.clone()),
                ("chunks", chunks.to_string()),
                ("total", media.bytes.to_string()),
                ("chunk", chunk.to_string()),
                ("size", size.to_string()),
                ("partNumber", (chunk + 1).to_string()),
                ("start", start.to_string()),
                ("end", (start + size).to_string()),
            ];
            let mut success = false;
            for attempt in 0..3 {
                let response = self
                    .http
                    .put(url.clone())
                    .query(&params)
                    .header("X-Upos-Auth", auth.clone())
                    .timeout(Duration::from_secs(240))
                    .body(bytes.clone())
                    .send();
                match response {
                    Ok(response) if response.status().is_success() => {
                        success = true;
                        break;
                    }
                    Ok(response)
                        if !(response.status().is_server_error()
                            || response.status().as_u16() == 429) =>
                    {
                        anyhow::bail!(
                            "Chunk {} upload: HTTP {}; receipt kept for --resume",
                            chunk + 1,
                            response.status().as_u16()
                        );
                    }
                    _ => {
                        if attempt < 2 {
                            std::thread::sleep(Duration::from_millis(250 * (attempt + 1)));
                        }
                    }
                }
            }
            ensure!(
                success,
                "Chunk {} upload failed after three attempts; receipt kept for --resume",
                chunk + 1
            );
            if let Some(previous) = receipt.confirmed_sha256.get_mut(chunk as usize) {
                *previous = part_sha256;
            } else {
                receipt.confirmed_sha256.push(part_sha256);
            }
            receipt.confirmed_parts = receipt.confirmed_parts.max(chunk + 1);
            storage::write_json(receipt_path, &receipt, true)?;
            eprintln!(
                "Uploaded part {}/{} ({} / {} bytes)",
                chunk + 1,
                chunks,
                start + size,
                media.bytes
            );
        }
        let mut extra = [0; 1];
        ensure!(
            file.read(&mut extra)? == 0 && hex::encode(hasher.finalize()) == media.sha256,
            "Video content changed during upload; refusing to finalize"
        );
        receipt.state = "finalizing".into();
        storage::write_json(receipt_path, &receipt, true)?;
        let parts: Vec<_> = (1..=chunks)
            .map(|part| json!({"partNumber":part,"eTag":"etag"}))
            .collect();
        let finished = api::response(
            self.http
                .post(url)
                .header("X-Upos-Auth", auth)
                .query(&[
                    ("name", receipt.source_name.as_str()),
                    ("uploadId", upload_id.as_str()),
                    ("biz_id", &bucket.biz_id.to_string()),
                    ("output", "json"),
                    ("profile", "ugcupos/bup"),
                ])
                .json(&json!({"parts":parts})),
            "complete upload",
        )?;
        ensure!(
            finished["OK"] == 1,
            "UPOS did not confirm completion; receipt kept for explicit --resume"
        );
        receipt.filename = Some(object_filename(&bucket.upos_uri)?);
        receipt.state = "complete".into();
        receipt.cid = Some(bucket.biz_id);
        receipt.bucket = None;
        receipt.upload_id = None;
        storage::write_json(receipt_path, &receipt, true)?;
        Ok(receipt)
    }

    fn upload_url(&self, bucket: &Bucket) -> Result<url::Url> {
        let endpoint = if bucket.endpoint.starts_with("//") {
            format!("https:{}", bucket.endpoint)
        } else {
            bucket.endpoint.clone()
        };
        let mut url =
            url::Url::parse(&endpoint).map_err(|_| anyhow::anyhow!("Invalid UPOS endpoint"))?;
        let host = url.host_str().context("UPOS endpoint has no host")?;
        let trusted = host.ends_with(".bilivideo.com")
            || host.ends_with(".bilivideo.cn")
            || host.ends_with(".bilibili.com");
        let testing_origin = self.testing && endpoint == self.api_origin;
        ensure!(
            testing_origin
                || (url.scheme() == "https" && trusted && url.port_or_known_default() == Some(443)),
            "Refusing untrusted UPOS endpoint"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "Invalid UPOS endpoint structure"
        );
        let key = bucket
            .upos_uri
            .strip_prefix("upos://")
            .context("Invalid UPOS object URI")?;
        ensure!(
            !key.is_empty()
                && key.split('/').all(|segment| !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
            "Invalid UPOS object path"
        );
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid UPOS endpoint"))?
            .clear()
            .extend(key.split('/'));
        Ok(url)
    }
}

fn object_filename(uri: &str) -> Result<String> {
    let name = uri.rsplit('/').next().context("No UPOS filename")?;
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    ensure!(!stem.is_empty(), "No UPOS filename");
    Ok(stem.into())
}
