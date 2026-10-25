use crate::{
    api::Api,
    storage::{self, Lock},
    upload::UploadReceipt,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub title: String,
    pub tid: u64,
    pub tags: Vec<String>,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub dynamic: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default = "original")]
    pub copyright: u8,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub no_reprint: bool,
    #[serde(default)]
    pub part_titles: Vec<String>,
    #[serde(default)]
    pub tid_v2: Option<u64>,
}
fn original() -> u8 {
    1
}
pub struct Prepared {
    pub owner_mid: u64,
    pub payload: Value,
    pub sha256: String,
}
impl Metadata {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.title.trim().is_empty() && self.title.chars().count() <= 80,
            "Title must contain 1..80 characters"
        );
        ensure!(
            self.tid > 0,
            "A category tid is required; use bilibili categories"
        );
        ensure!((1..=12).contains(&self.tags.len()), "Provide 1..12 tags");
        ensure!(
            self.tags.iter().all(|s| !s.trim().is_empty()
                && s.chars().count() <= 20
                && !s.contains([',', '\n', '\r'])),
            "Tags must contain 1..20 characters, without commas or newlines"
        );
        ensure!(
            self.desc.chars().count() <= 2000,
            "Description exceeds 2000 characters"
        );
        ensure!(
            self.dynamic.chars().count() <= 233,
            "Dynamic text exceeds 233 characters"
        );
        ensure!(
            self.copyright == 1 || self.copyright == 2,
            "Copyright must be 1 (original) or 2 (repost)"
        );
        ensure!(
            self.copyright != 2 || !self.source.trim().is_empty(),
            "Reposts require a source"
        );
        if !self.cover.is_empty() {
            let cover = url::Url::parse(&self.cover)
                .context("Cover must be a URL returned by bilibili cover upload")?;
            ensure!(
                matches!(cover.scheme(), "http" | "https")
                    && cover
                        .host_str()
                        .is_some_and(|h| h == "hdslb.com" || h.ends_with(".hdslb.com"))
                    && cover.username().is_empty()
                    && cover.password().is_none(),
                "Untrusted cover URL"
            );
        }
        Ok(())
    }
    pub fn prepare(&self, receipts: &[UploadReceipt]) -> Result<Prepared> {
        self.validate()?;
        ensure!(
            !receipts.is_empty() && receipts.len() <= 100,
            "Provide 1..100 completed upload receipts (account limits may be lower)"
        );
        ensure!(
            self.part_titles.is_empty() || self.part_titles.len() == receipts.len(),
            "part_titles must match the number of uploads"
        );
        let owner_mid = receipts[0].owner_mid;
        ensure!(owner_mid > 0, "Upload receipt is missing an owner");
        let mut seen = std::collections::HashSet::new();
        let mut videos = Vec::new();
        for (index, receipt) in receipts.iter().enumerate() {
            ensure!(
                receipt.schema_version == 1 && receipt.state == "complete",
                "Every upload receipt must be complete and use schema version 1"
            );
            ensure!(
                receipt.owner_mid == owner_mid,
                "Upload receipts belong to different accounts"
            );
            ensure!(
                seen.insert(receipt.media.sha256.as_str()),
                "Duplicate video content in upload receipts"
            );
            let filename = receipt
                .filename
                .as_deref()
                .filter(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                })
                .context("Receipt has an invalid uploaded filename")?;
            let part_title = self
                .part_titles
                .get(index)
                .map(String::as_str)
                .unwrap_or(self.title.as_str());
            ensure!(
                !part_title.trim().is_empty() && part_title.chars().count() <= 80,
                "Part titles must contain 1..80 characters"
            );
            videos.push(json!({"filename":filename,"title":part_title,"desc":""}));
        }
        let mut payload = json!({"copyright":self.copyright,"source":self.source,"tid":self.tid,"cover":self.cover,"title":self.title,"desc_format_id":0,"desc":self.desc,"dynamic":self.dynamic,"tag":self.tags.join(","),"videos":videos,"subtitle":{"open":0,"lan":""},"no_reprint":u8::from(self.no_reprint)});
        if let Some(tid_v2) = self.tid_v2 {
            ensure!(tid_v2 > 0, "tid_v2 must be positive");
            payload["tid_v2"] = json!(tid_v2);
        }
        let sha256 = hex::encode(Sha256::digest(serde_json::to_vec(
            &json!({"owner_mid":owner_mid,"payload":payload}),
        )?));
        Ok(Prepared {
            owner_mid,
            payload,
            sha256,
        })
    }
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).context("Cannot read submission metadata")?;
        ensure!(
            meta.len() <= 1024 * 1024,
            "Submission metadata exceeds 1 MiB"
        );
        serde_json::from_slice(&std::fs::read(path)?).map_err(|_| {
            anyhow::anyhow!(
                "Invalid metadata JSON or unrecognized fields; see examples/submission.json"
            )
        })
    }
}

pub fn execute(api: &Api, prepared: &Prepared, result: &Path, publish: bool) -> Result<Value> {
    if !publish {
        return Ok(
            json!({"mode":"preview","remote_draft":false,"owner_mid":prepared.owner_mid,"payload_sha256":prepared.sha256,"payload":prepared.payload}),
        );
    }
    let _lock = Lock::acquire(result)?;
    ensure!(
        !result.exists(),
        "Submission receipt already exists; refusing a duplicate attempt. Use readback and own archives to reconcile it"
    );
    ensure!(
        api.account_id()? == prepared.owner_mid,
        "Submission uploads belong to another account"
    );
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut receipt = json!({"schema_version":1,"state":"attempting","created_at":created_at,"owner_mid":prepared.owner_mid,"payload_sha256":prepared.sha256,"title":prepared.payload["title"]});
    storage::write_json(result, &receipt, false)?;
    let data = match api.publish(&prepared.payload) {
        Ok(data) => data,
        Err(error) => {
            receipt["state"] = json!("uncertain");
            storage::write_json(result, &receipt, true)?;
            return Err(error.context(
                "Submission attempt recorded; reconcile with own archives before any new attempt",
            ));
        }
    };
    let aid = data["aid"].as_u64().filter(|n| *n > 0);
    let bvid = data["bvid"].as_str().filter(|s| s.starts_with("BV"));
    if aid.is_none() && bvid.is_none() {
        receipt["state"] = json!("uncertain");
        storage::write_json(result, &receipt, true)?;
        anyhow::bail!(
            "Submission returned success without an ID; reconcile with own archives, do not submit again"
        );
    }
    receipt["state"] = json!("accepted");
    receipt["aid"] = json!(aid);
    receipt["bvid"] = json!(bvid);
    storage::write_json(result, &receipt, true)?;
    Ok(receipt)
}

pub fn default_result(prepared: &Prepared) -> Result<std::path::PathBuf> {
    Ok(dirs::data_local_dir()
        .context("No local data directory; pass --result")?
        .join("bilibili-cli/submissions")
        .join(format!("{}.receipt.json", prepared.sha256)))
}
