//! Preview-first revisions of an existing single-part archive and collection membership.
use crate::{
    api::Api,
    storage::{self, Lock},
    submission::Metadata,
    upload::UploadReceipt,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn hash(value: &Value) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}
fn positive(value: &Value, field: &str) -> Result<u64> {
    value[field]
        .as_u64()
        .filter(|n| *n > 0)
        .with_context(|| format!("Missing positive {field}"))
}
pub fn archive_fingerprint(snapshot: &Value) -> Result<String> {
    let a = &snapshot["archive"];
    let mut stable = serde_json::Map::new();
    for k in [
        "aid",
        "bvid",
        "mid",
        "title",
        "desc",
        "tag",
        "tid",
        "cover",
        "copyright",
        "source",
        "no_reprint",
        "dynamic",
        "creation_statement",
        "is_only_self",
        "no_public",
        "is_space_hidden",
        "is_dolby",
        "lossless_music",
        "recreate",
        "state",
        "staffs",
        "charging_pay",
        "ugcpay",
        "interactive",
    ] {
        stable.insert(k.into(), a[k].clone());
    }
    let parts = snapshot["videos"]
        .as_array()
        .context("Snapshot has no videos")?
        .iter()
        .map(|v| json!({"cid":v["cid"],"filename":v["filename"],"title":v["title"]}))
        .collect::<Vec<_>>();
    hash(&json!({"archive":stable,"videos":parts,"in_season":snapshot["in_season"]}))
}
pub fn prepare_edit(
    snapshot: &Value,
    metadata: &Metadata,
    uploads: &[UploadReceipt],
) -> Result<Value> {
    let a = &snapshot["archive"];
    let aid = positive(a, "aid")?;
    let mid = positive(a, "mid")?;
    let old = snapshot["videos"]
        .as_array()
        .context("Snapshot has no videos")?;
    ensure!(
        old.len() == 1 && uploads.len() == 1,
        "This edit command replaces exactly one existing part; multipart changes are not supported"
    );
    ensure!(
        snapshot["replace_check"]["can_replace"] == true,
        "Creator detail does not allow video replacement"
    );
    ensure!(
        a["state"].as_i64() == Some(0),
        "Wait for the archive to finish processing/review before replacing media"
    );
    for key in [
        "charging_pay",
        "ugcpay",
        "interactive",
        "is_staff",
        "premiere",
    ] {
        ensure!(
            a[key].is_null() || a[key] == 0 || a[key] == false,
            "Editing {key} archives is not supported"
        );
    }
    ensure!(
        a["staffs"].is_null() || a["staffs"].as_array().is_some_and(|s| s.is_empty()),
        "Editing collaborative archives is not supported"
    );
    if let Some(id) = metadata.creation_statement_id {
        ensure!(
            a["creation_statement"]["id"].as_i64() == Some(id),
            "Bilibili locks the creation declaration after publication; preserve the original declaration when editing"
        );
    }
    let prepared = metadata.prepare(uploads)?;
    ensure!(
        prepared.owner_mid == mid,
        "Replacement upload belongs to another account"
    );
    let cid = uploads[0]
        .cid
        .filter(|n| *n > 0)
        .context("Replacement receipt has no CID; upload new media with this CLI version")?;
    ensure!(
        old[0]["cid"].as_u64() != Some(cid),
        "Replacement must be new uploaded media"
    );
    let mut payload = prepared.payload;
    payload["aid"] = json!(aid);
    payload["new_web_edit"] = json!(1);
    payload["videos"][0]["cid"] = json!(cid);
    // Preserve explicit creator settings; metadata contains the complete new copy.
    for key in [
        "is_only_self",
        "no_public",
        "is_space_hidden",
        "is_dolby",
        "lossless_music",
        "recreate",
    ] {
        if !a[key].is_null() {
            payload[key] = a[key].clone();
        }
    }
    if !a["creation_statement"].is_null() {
        payload["creation_statement"] = a["creation_statement"].clone();
    }
    Ok(
        json!({"operation":"edit","owner_mid":mid,"aid":aid,"bvid":a["bvid"],"before_sha256":archive_fingerprint(snapshot)?,"original_cid":old[0]["cid"],"replacement_cid":cid,"in_season":snapshot["in_season"],"payload":payload}),
    )
}
fn result_path(plan: &Value, provided: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = provided {
        return Ok(p.to_owned());
    }
    Ok(dirs::data_local_dir()
        .context("No local data directory; pass --result")?
        .join("bilibili-cli/revisions")
        .join(format!("{}.json", hash(plan)?)))
}
fn mutate(
    api: &Api,
    plan: &Value,
    path: &str,
    result: Option<&Path>,
    publish: bool,
) -> Result<Value> {
    let output = result_path(plan, result)?;
    if !publish {
        return Ok(json!({"mode":"preview","remote_draft":false,"plan":plan,"result_file":output}));
    }
    let _lock = Lock::acquire(&output)?;
    ensure!(
        !output.exists(),
        "Revision receipt exists; reconcile it before a new attempt"
    );
    let mut receipt = json!({"schema_version":1,"state":"attempting","created_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),"plan":plan,"plan_sha256":hash(plan)?,"aid":plan["aid"],"bvid":plan["bvid"],"owner_mid":plan["owner_mid"]});
    storage::write_json(&output, &receipt, false)?;
    match api.mutate_creator(path, &plan["payload"]) {
        Ok(response) => {
            receipt["state"] = json!("accepted");
            receipt["response"] = response;
        }
        Err(error) => {
            receipt["state"] = json!("uncertain");
            storage::write_json(&output, &receipt, true)?;
            return Err(error.context(format!(
                "Attempt recorded at {}; use creator/collection reads before retrying",
                output.display()
            )));
        }
    }
    storage::write_json(&output, &receipt, true)?;
    receipt["result_file"] = json!(output);
    Ok(receipt)
}
pub fn edit(
    api: &Api,
    snapshot: &Value,
    metadata: &Metadata,
    uploads: &[UploadReceipt],
    result: Option<&Path>,
    publish: bool,
) -> Result<Value> {
    let plan = prepare_edit(snapshot, metadata, uploads)?;
    if publish {
        ensure!(
            api.account_id()? == positive(&plan, "owner_mid")?,
            "Archive/upload account differs from current session"
        );
        let fresh = api.archive(&positive(&plan, "aid")?.to_string())?;
        ensure!(
            archive_fingerprint(&fresh)? == plan["before_sha256"],
            "Archive changed since snapshot; fetch a fresh snapshot and review again"
        );
        ensure!(
            fresh["replace_check"]["can_replace"] == true,
            "Replacement permission changed"
        );
    }
    mutate(api, &plan, "/x/vu/web/edit", result, publish)
}
fn owned_section(api: &Api, season: u64, section: u64) -> Result<(u64, Value)> {
    let mid = api.account_id()?;
    let collection = api.collection(season)?;
    ensure!(
        collection["season"]["mid"].as_u64() == Some(mid),
        "Collection belongs to another account"
    );
    ensure!(
        collection["season"]["is_pay"].as_i64().unwrap_or(0) == 0,
        "Paid collections are not supported"
    );
    let data = api.section(section)?;
    ensure!(
        data["section"]["seasonId"].as_u64() == Some(season),
        "Section belongs to a different collection"
    );
    ensure!(
        data["section"]["id"].as_u64() == Some(section),
        "Section response ID mismatch"
    );
    let episodes = data["episodes"]
        .as_array()
        .context("Missing section episodes")?;
    ensure!(
        data["section"]["epCount"].as_u64() == Some(episodes.len() as u64),
        "Incomplete section response; refusing a partial membership/order plan"
    );
    Ok((mid, data))
}
pub fn add(
    api: &Api,
    season: u64,
    section: u64,
    video: &str,
    result: Option<&Path>,
    publish: bool,
) -> Result<Value> {
    let (mid, section_data) = owned_section(api, season, section)?;
    let archive = api.archive(video)?;
    let a = &archive["archive"];
    let aid = positive(a, "aid")?;
    ensure!(
        positive(a, "mid")? == mid,
        "Video belongs to another account"
    );
    if section_data["episodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["aid"] == aid)
    {
        return Ok(
            json!({"state":"already_member","mutated":false,"season_id":season,"section_id":section,"aid":aid}),
        );
    }
    ensure!(
        archive["in_season"] != true,
        "Video already belongs to another collection/section; moving it requires a separate operation"
    );
    ensure!(
        a["state"].as_i64() == Some(0),
        "Wait for processing/review before adding the video"
    );
    let videos = archive["videos"]
        .as_array()
        .context("Archive has no videos")?;
    ensure!(
        videos.len() == 1,
        "Collection add currently supports single-part videos"
    );
    for key in ["charging_pay", "member_first", "limited_free", "ugcpay"] {
        ensure!(
            a[key].is_null() || a[key] == 0 || a[key] == false,
            "Collection add does not support {key} videos"
        );
    }
    let cid = positive(&videos[0], "cid")?;
    let payload = json!({"sectionId":section,"episodes":[{"aid":aid,"cid":cid,"title":a["title"],"charging_pay":0,"member_first":0,"limited_free":false}]});
    let plan = json!({"operation":"collection_add","owner_mid":mid,"season_id":season,"section_id":section,"aid":aid,"bvid":a["bvid"],"payload":payload});
    mutate(
        api,
        &plan,
        "/x2/creative/web/season/section/episodes/add",
        result,
        publish,
    )
}
pub fn prepare_order(section: &Value, episode_ids: &[u64]) -> Result<Value> {
    let existing = section["episodes"].as_array().context("Missing episodes")?;
    let actual = existing
        .iter()
        .map(|e| positive(e, "id"))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    let proposed: std::collections::BTreeSet<_> = episode_ids.iter().copied().collect();
    ensure!(
        !episode_ids.is_empty() && actual == proposed && proposed.len() == episode_ids.len(),
        "Order must contain every current episode ID exactly once"
    );
    let s = &section["section"];
    Ok(
        json!({"section":{"id":s["id"],"type":s["type"],"seasonId":s["seasonId"],"title":s["title"]},"sorts":episode_ids.iter().enumerate().map(|(i,id)|json!({"id":id,"sort":i+1})).collect::<Vec<_>>()}),
    )
}
pub fn order(
    api: &Api,
    season: u64,
    section: u64,
    episode_ids: &[u64],
    result: Option<&Path>,
    publish: bool,
) -> Result<Value> {
    let (mid, data) = owned_section(api, season, section)?;
    let payload = prepare_order(&data, episode_ids)?;
    let plan = json!({"operation":"collection_order","owner_mid":mid,"season_id":season,"section_id":section,"payload":payload});
    mutate(
        api,
        &plan,
        "/x2/creative/web/season/section/edit",
        result,
        publish,
    )
}
