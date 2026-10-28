use crate::{
    api::Api,
    media::MediaInfo,
    storage,
    submission::{self, Metadata},
    upload::UploadReceipt,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}
struct Server {
    origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start(handler: impl Fn(&Request, &str) -> (u16, Value) + Send + 'static) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", server.server_addr());
        let origin_copy = origin.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let Some(mut req) = server.recv_timeout(Duration::from_millis(50)).unwrap() else {
                    continue;
                };
                let mut body = Vec::new();
                req.as_reader().read_to_end(&mut body).unwrap();
                let record = Request {
                    method: req.method().to_string(),
                    path: req.url().into(),
                    headers: req
                        .headers()
                        .iter()
                        .map(|h| (h.field.to_string().to_lowercase(), h.value.to_string()))
                        .collect(),
                    body,
                };
                let (status, body) = handler(&record, &origin_copy);
                records.lock().unwrap().push(record);
                req.respond(
                    tiny_http::Response::from_string(body.to_string())
                        .with_status_code(status)
                        .with_header(
                            tiny_http::Header::from_bytes("content-type", "application/json")
                                .unwrap(),
                        ),
                )
                .unwrap();
            }
        });
        Self {
            origin,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn snapshot(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn media(bytes: &[u8]) -> MediaInfo {
    MediaInfo {
        bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        duration_seconds: 1.0,
        width: 1920,
        height: 1080,
        video_codec: "h264".into(),
        audio_codec: Some("aac".into()),
    }
}
fn handler(req: &Request, origin: &str) -> (u16, Value) {
    if req.path.starts_with("/x/web-interface/nav") {
        return (
            200,
            json!({"code":0,"data":{"isLogin":true,"mid":123,"uname":"fixture"}}),
        );
    }
    if req.path.starts_with("/preupload") {
        return (
            200,
            json!({"OK":1,"chunk_size":4,"auth":"upload-secret","endpoint":origin,"biz_id":7,"upos_uri":"upos://bucket/object.mp4"}),
        );
    }
    if req.method == "POST" && req.path.contains("uploads") {
        return (200, json!({"upload_id":"upload-id"}));
    }
    if req.method == "PUT" {
        return (200, json!({"OK":1}));
    }
    if req.path.starts_with("/bucket/") {
        return (200, json!({"OK":1}));
    }
    panic!("Unexpected request to {}", req.path);
}

#[test]
fn upos_upload_uses_exact_ranges_scoped_auth_and_reuses_completed_receipt() {
    let server = Server::start(handler);
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let receipt = dir.path().join("upload.json");
    fs::write(&path, b"0123456789").unwrap();
    let completed = api
        .upload(&path, &media(b"0123456789"), &receipt, false)
        .unwrap();
    assert_eq!(completed.state, "complete");
    assert_eq!(completed.filename.as_deref(), Some("object"));
    assert_eq!(completed.cid, Some(7));
    let records = server.snapshot();
    let preupload = records
        .iter()
        .find(|r| r.path.starts_with("/preupload"))
        .unwrap();
    let preupload_url = url::Url::parse(&format!("{}{}", server.origin, preupload.path)).unwrap();
    let preupload_query: HashMap<_, _> = preupload_url.query_pairs().into_owned().collect();
    assert_eq!(
        preupload_query.get("upcdn").map(String::as_str),
        Some("bldsa")
    );
    assert_eq!(preupload_query.get("zone").map(String::as_str), Some("cs"));
    let puts: Vec<_> = records.iter().filter(|r| r.method == "PUT").collect();
    assert_eq!(
        puts.iter().map(|r| r.body.clone()).collect::<Vec<_>>(),
        vec![b"0123".to_vec(), b"4567".to_vec(), b"89".to_vec()]
    );
    for (i, req) in puts.iter().enumerate() {
        let url = url::Url::parse(&format!("{}{}", server.origin, req.path)).unwrap();
        let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["partNumber"], (i + 1).to_string());
        assert_eq!(q["start"], (i * 4).to_string());
        assert_eq!(q["end"], ((i * 4 + 4).min(10)).to_string());
        assert_eq!(q["total"], "10");
        assert_eq!(q["chunks"], "3");
    }
    for req in records.iter().filter(|r| r.path.starts_with("/bucket/")) {
        assert_eq!(req.headers["x-upos-auth"], "upload-secret");
        assert!(!req.headers.contains_key("cookie"));
    }
    let finalize = records
        .iter()
        .find(|r| {
            r.method == "POST" && r.path.starts_with("/bucket/") && !r.path.contains("uploads")
        })
        .unwrap();
    let body: Value = serde_json::from_slice(&finalize.body).unwrap();
    assert_eq!(body["parts"].as_array().unwrap().len(), 3);
    assert!(
        !fs::read_to_string(&receipt)
            .unwrap()
            .contains("upload-secret")
    );
    let before = server
        .snapshot()
        .iter()
        .filter(|r| r.path.starts_with("/preupload"))
        .count();
    api.upload(&path, &media(b"0123456789"), &receipt, false)
        .unwrap();
    assert_eq!(
        server
            .snapshot()
            .iter()
            .filter(|r| r.path.starts_with("/preupload"))
            .count(),
        before
    );
}

#[test]
fn upload_resumes_same_object_and_does_not_send_confirmed_parts_again() {
    let fail_once = Arc::new(AtomicBool::new(true));
    let flag = fail_once.clone();
    let server = Server::start(move |r, origin| {
        if r.method == "PUT"
            && r.path.contains("partNumber=2")
            && flag.swap(false, Ordering::SeqCst)
        {
            return (400, json!({"message":"upload-secret"}));
        }
        handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let receipt = dir.path().join("upload.json");
    fs::write(&path, b"0123456789").unwrap();
    let error = api
        .upload(&path, &media(b"0123456789"), &receipt, false)
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("upload-secret"));
    assert!(
        api.upload(&path, &media(b"0123456789"), &receipt, false)
            .is_err()
    );
    api.upload(&path, &media(b"0123456789"), &receipt, true)
        .unwrap();
    let records = server.snapshot();
    assert_eq!(
        records
            .iter()
            .filter(|r| r.path.starts_with("/preupload"))
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.method == "PUT" && r.path.contains("partNumber=1"))
            .count(),
        1
    );
}

#[test]
fn preview_is_offline_and_ambiguous_publish_is_never_retried() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x/vu/web/add/v3") {
            return (500, json!({"message":"secret-session"}));
        }
        handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let metadata: Metadata =
        serde_json::from_value(json!({"title":"Fixture","tid":201,"tags":["Rust"]})).unwrap();
    let receipt: UploadReceipt = serde_json::from_value(json!({"schema_version":1,"owner_mid":123,"media":media(b"abcd"),"state":"complete","filename":"object"})).unwrap();
    let prepared = metadata.prepare(&[receipt]).unwrap();
    let result = dir.path().join("publish.json");
    assert_eq!(
        submission::execute(&api, &prepared, &result, false).unwrap()["mode"],
        "preview"
    );
    assert!(server.snapshot().is_empty());
    assert!(!result.exists());
    let error = submission::execute(&api, &prepared, &result, true)
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("secret-session"));
    let result_value: Value = storage::read_json(&result).unwrap();
    assert_eq!(result_value["state"], "uncertain");
    assert!(submission::execute(&api, &prepared, &result, true).is_err());
    assert_eq!(
        server
            .snapshot()
            .iter()
            .filter(|r| r.path.starts_with("/x/vu/web/add/v3"))
            .count(),
        1
    );
}

#[test]
fn malicious_upload_origin_is_rejected_before_any_credentials_leave_bilibili() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/preupload") {
            return (
                200,
                json!({"chunk_size":4,"auth":"upload-secret","endpoint":"https://evil.invalid","biz_id":7,"upos_uri":"upos://bucket/object.mp4"}),
            );
        }
        handler(r, origin)
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    fs::write(&path, b"abcd").unwrap();
    assert!(
        Api::test(&server.origin)
            .upload(
                &path,
                &media(b"abcd"),
                &dir.path().join("receipt.json"),
                false
            )
            .is_err()
    );
    assert_eq!(server.snapshot().len(), 2);
}

#[test]
fn accepted_submission_records_ids_and_sends_web_contract_once() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x/vu/web/add/v3") {
            return (
                200,
                json!({"code":0,"data":{"aid":170001,"bvid":"BV17x411w7KC"}}),
            );
        }
        handler(r, origin)
    });
    let metadata: Metadata = serde_json::from_value(json!({"title":"Fixture","tid":201,"tags":["Rust"],"copyright":2,"source":"https://example.org/original"})).unwrap();
    let upload: UploadReceipt = serde_json::from_value(json!({"schema_version":1,"owner_mid":123,"media":media(b"abcd"),"state":"complete","filename":"object"})).unwrap();
    let prepared = metadata.prepare(&[upload]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let result_path = dir.path().join("publish.json");
    let result =
        submission::execute(&Api::test(&server.origin), &prepared, &result_path, true).unwrap();
    assert_eq!(result["state"], "accepted");
    assert_eq!(result["aid"], 170001);
    assert_eq!(result["title"], "Fixture");
    let saved: Value = storage::read_json(&result_path).unwrap();
    assert_eq!(saved, result);
    let records = server.snapshot();
    let request = records
        .iter()
        .find(|r| r.path.starts_with("/x/vu/web/add/v3"))
        .unwrap();
    let url = url::Url::parse(&format!("{}{}", server.origin, request.path)).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["csrf"], "secret-csrf");
    assert!(request.headers["cookie"].contains("SESSDATA=secret-session"));
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["videos"][0]["filename"], "object");
    assert_eq!(body["copyright"], 2);
    assert!(!fs::read_to_string(result_path).unwrap().contains("secret-"));
}

#[test]
fn cover_upload_checks_api_code_and_never_echoes_server_secrets() {
    let server = Server::start(|r, _| {
        assert!(r.path.starts_with("/x/vu/web/cover/up"));
        (
            200,
            json!({"code":-101,"message":"secret-session","data":{"url":"https://i0.hdslb.com/not-actually-success.png"}}),
        )
    });
    let error = Api::test(&server.origin)
        .upload_cover(b"fixture", "image/png")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("-101"));
    assert!(!error.contains("secret-session"));
    let requests = server.snapshot();
    let form: HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
        .into_owned()
        .collect();
    assert_eq!(form["csrf"], "secret-csrf");
    assert_eq!(form["cover"], "data:image/png;base64,Zml4dHVyZQ==");
}

#[test]
fn changing_source_bytes_during_upload_prevents_finalization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    fs::write(&path, b"0123456789").unwrap();
    let changing_path = path.clone();
    let server = Server::start(move |r, origin| {
        if r.method == "PUT" && r.path.contains("partNumber=1") {
            fs::write(&changing_path, b"0123abcdef").unwrap();
        }
        handler(r, origin)
    });
    let result = Api::test(&server.origin).upload(
        &path,
        &media(b"0123456789"),
        &dir.path().join("upload.json"),
        false,
    );
    assert!(result.err().unwrap().to_string().contains("changed"));
    assert!(
        !server
            .snapshot()
            .iter()
            .any(|r| r.method == "POST" && r.path.contains("uploadId="))
    );
    fs::write(&path, b"0123456789").unwrap();
    Api::test(&server.origin)
        .upload(
            &path,
            &media(b"0123456789"),
            &dir.path().join("upload.json"),
            true,
        )
        .unwrap();
    assert_eq!(last_uploaded_bytes(&server.snapshot()), b"0123456789");
}

fn last_uploaded_bytes(records: &[Request]) -> Vec<u8> {
    let mut parts = std::collections::BTreeMap::new();
    for record in records.iter().filter(|r| r.method == "PUT") {
        let url = url::Url::parse(&format!("http://localhost{}", record.path)).unwrap();
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        parts.insert(
            query["partNumber"].parse::<usize>().unwrap(),
            record.body.clone(),
        );
    }
    parts.into_values().flatten().collect()
}

#[test]
fn resume_repairs_changed_confirmed_parts_even_if_network_failed_before_hash_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let original = b"0123456789AB";
    fs::write(&path, original).unwrap();
    let changing_path = path.clone();
    let fail_once = AtomicBool::new(true);
    let server = Server::start(move |r, origin| {
        if r.method == "PUT" && r.path.contains("partNumber=1") {
            fs::write(&changing_path, b"0123abcd89AB").unwrap();
        }
        if r.method == "PUT"
            && r.path.contains("partNumber=3")
            && fail_once.swap(false, Ordering::SeqCst)
        {
            return (400, json!({"error":"fixture"}));
        }
        handler(r, origin)
    });
    let receipt = dir.path().join("upload.json");
    let api = Api::test(&server.origin);
    assert!(
        api.upload(&path, &media(original), &receipt, false)
            .is_err()
    );
    fs::write(&path, original).unwrap();
    api.upload(&path, &media(original), &receipt, true).unwrap();
    assert_eq!(last_uploaded_bytes(&server.snapshot()), original);
}

#[test]
fn replacement_part_with_lost_ack_is_reuploaded_before_finalization() {
    use std::sync::atomic::AtomicUsize;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let original = b"0123456789AB";
    fs::write(&path, original).unwrap();
    let changing_path = path.clone();
    let nav_calls = AtomicUsize::new(0);
    let fail_initial = AtomicBool::new(true);
    let fail_repair = AtomicBool::new(true);
    let server = Server::start(move |r, origin| {
        if r.path.starts_with("/x/web-interface/nav")
            && nav_calls.fetch_add(1, Ordering::SeqCst) == 1
        {
            fs::write(&changing_path, b"0123abcd89AB").unwrap();
        }
        if r.method == "PUT"
            && r.path.contains("partNumber=3")
            && fail_initial.swap(false, Ordering::SeqCst)
        {
            return (400, json!({"error":"initial interruption"}));
        }
        // Emulate an ambiguous failure after the server has stored a replacement.
        if r.method == "PUT"
            && r.path.contains("partNumber=2")
            && nav_calls.load(Ordering::SeqCst) == 2
            && fail_repair.swap(false, Ordering::SeqCst)
        {
            return (400, json!({"error":"replacement acknowledgment lost"}));
        }
        handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let receipt = dir.path().join("upload.json");
    assert!(
        api.upload(&path, &media(original), &receipt, false)
            .is_err()
    );
    assert!(api.upload(&path, &media(original), &receipt, true).is_err());
    fs::write(&path, original).unwrap();
    api.upload(&path, &media(original), &receipt, true).unwrap();
    assert_eq!(last_uploaded_bytes(&server.snapshot()), original);
}

#[test]
fn ai_declaration_and_current_cover_host_survive_preparation() {
    let metadata: Metadata = serde_json::from_value(json!({"title":"Fixture","tid":231,"tags":["AI"],"cover":"https://archive.biliimg.com/bfs/archive/fixture.png","creation_statement_id":1})).unwrap();
    let upload: UploadReceipt = serde_json::from_value(json!({"schema_version":1,"owner_mid":123,"media":media(b"abcd"),"state":"complete","filename":"object"})).unwrap();
    let prepared = metadata.prepare(&[upload]).unwrap();
    assert_eq!(prepared.payload["creation_statement"]["id"], 1);
    assert_eq!(
        prepared.payload["cover"],
        "https://archive.biliimg.com/bfs/archive/fixture.png"
    );
}

fn edit_snapshot() -> Value {
    json!({"archive":{"aid":170001,"bvid":"BV17x411w7KC","mid":123,"state":0,"title":"Old","desc":"Old body","tag":"Rust","tid":231,"is_only_self":0,"no_public":0,"creation_statement":{"id":1}},"videos":[{"cid":10,"filename":"old-file","title":"Old"}],"replace_check":{"can_replace":true},"in_season":true})
}
fn replacement() -> UploadReceipt {
    serde_json::from_value(json!({"schema_version":1,"owner_mid":123,"state":"complete","filename":"new-file","cid":20,"media":media(b"replacement")})).unwrap()
}
fn new_metadata() -> Metadata {
    serde_json::from_value(json!({"title":"New","desc":"First paragraph\n\nSecond paragraph","tid":231,"tags":["Rust"]})).unwrap()
}
#[test]
fn edit_preview_is_offline_keeps_archive_identity_and_uses_new_cid() {
    let server = Server::start(|_, _| panic!("Preview must not call the network"));
    let dir = tempfile::tempdir().unwrap();
    let result = dir.path().join("edit.json");
    let preview = crate::revision::edit(
        &Api::test(&server.origin),
        &edit_snapshot(),
        &new_metadata(),
        &[replacement()],
        Some(&result),
        false,
    )
    .unwrap();
    let p = &preview["plan"]["payload"];
    assert_eq!(p["aid"], 170001);
    assert_eq!(p["videos"][0]["cid"], 20);
    assert_eq!(p["videos"][0]["filename"], "new-file");
    assert_eq!(p["new_web_edit"], 1);
    assert_eq!(p["creation_statement"]["id"], 1);
    assert_eq!(p["desc"], "First paragraph\n\nSecond paragraph");
    assert!(!result.exists());
    assert!(server.snapshot().is_empty());
}
#[test]
fn edit_uses_original_archive_endpoint_and_accepts_empty_success_data() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x/vupre/web/archive/view") {
            return (200, json!({"code":0,"data":edit_snapshot()}));
        }
        if r.path.starts_with("/x/vu/web/edit?") {
            let p: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(p["aid"], 170001);
            assert_eq!(p["videos"][0]["cid"], 20);
            assert!(r.path.contains("csrf=secret-csrf"));
            return (200, json!({"code":0}));
        }
        handler(r, origin)
    });
    let dir = tempfile::tempdir().unwrap();
    let result = dir.path().join("edit.json");
    let receipt = crate::revision::edit(
        &Api::test(&server.origin),
        &edit_snapshot(),
        &new_metadata(),
        &[replacement()],
        Some(&result),
        true,
    )
    .unwrap();
    assert_eq!(receipt["state"], "accepted");
    assert_eq!(receipt["aid"], 170001);
    assert!(!server.snapshot().iter().any(|r| r.path.contains("/add/")));
    assert!(
        crate::revision::edit(
            &Api::test(&server.origin),
            &edit_snapshot(),
            &new_metadata(),
            &[replacement()],
            Some(&result),
            true
        )
        .is_err()
    );
    assert_eq!(
        server
            .snapshot()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}
#[test]
fn edit_rejects_stale_archive_and_missing_replacement_cid() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x/vupre/web/archive/view") {
            let mut s = edit_snapshot();
            s["archive"]["title"] = json!("Concurrent edit");
            return (200, json!({"code":0,"data":s}));
        }
        handler(r, origin)
    });
    let dir = tempfile::tempdir().unwrap();
    assert!(
        crate::revision::edit(
            &Api::test(&server.origin),
            &edit_snapshot(),
            &new_metadata(),
            &[replacement()],
            Some(&dir.path().join("edit.json")),
            true
        )
        .unwrap_err()
        .to_string()
        .contains("changed")
    );
    assert!(!server.snapshot().iter().any(|r| r.method == "POST"));
    let mut upload = replacement();
    upload.cid = None;
    assert!(crate::revision::prepare_edit(&edit_snapshot(), &new_metadata(), &[upload]).is_err());
    let mut collaborative = edit_snapshot();
    collaborative["archive"]["staffs"] = json!([{"mid":456}]);
    assert!(
        crate::revision::prepare_edit(&collaborative, &new_metadata(), &[replacement()]).is_err()
    );
}

#[test]
fn edit_preserves_the_platform_locked_creation_declaration() {
    let mut snapshot = edit_snapshot();
    snapshot["archive"]["creation_statement"] = json!({"id":-1,"content":"内容无需标注"});
    let mut metadata = new_metadata();
    let plan = crate::revision::prepare_edit(&snapshot, &metadata, &[replacement()]).unwrap();
    assert_eq!(plan["payload"]["creation_statement"]["id"], -1);
    metadata.creation_statement_id = Some(-1);
    assert!(crate::revision::prepare_edit(&snapshot, &metadata, &[replacement()]).is_ok());
    metadata.creation_statement_id = Some(1);
    assert!(
        crate::revision::prepare_edit(&snapshot, &metadata, &[replacement()])
            .unwrap_err()
            .to_string()
            .contains("locks")
    );
}
fn section_fixture() -> Value {
    json!({"section":{"id":22,"seasonId":11,"type":1,"title":"Main","epCount":2},"episodes":[{"id":101,"aid":1,"order":1},{"id":102,"aid":2,"order":2}]})
}
fn collection_handler(r: &Request, origin: &str) -> (u16, Value) {
    if r.path.starts_with("/x2/creative/web/season/section?") {
        return (200, json!({"code":0,"data":section_fixture()}));
    }
    if r.path.starts_with("/x2/creative/web/season?") {
        return (
            200,
            json!({"code":0,"data":{"season":{"id":11,"mid":123,"is_pay":0}}}),
        );
    }
    if r.path.starts_with("/x/vupre/web/archive/view") {
        let mut s = edit_snapshot();
        s["in_season"] = json!(false);
        return (200, json!({"code":0,"data":s}));
    }
    handler(r, origin)
}
#[test]
fn collection_add_preview_reads_state_without_writing_and_publish_has_exact_payload() {
    let server = Server::start(|r, origin| {
        if r.path
            .starts_with("/x2/creative/web/season/section/episodes/add?")
        {
            let p: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(p["sectionId"], 22);
            assert_eq!(p["episodes"][0]["aid"], 170001);
            assert_eq!(p["episodes"][0]["cid"], 10);
            return (200, json!({"code":0,"data":null}));
        }
        collection_handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("add.json");
    let preview = crate::revision::add(&api, 11, 22, "170001", Some(&path), false).unwrap();
    assert_eq!(preview["mode"], "preview");
    assert!(!server.snapshot().iter().any(|r| r.method == "POST"));
    assert!(!path.exists());
    let result = crate::revision::add(&api, 11, 22, "170001", Some(&path), true).unwrap();
    assert_eq!(result["state"], "accepted");
}
#[test]
fn collection_order_requires_complete_permutation_and_uses_sort_not_order() {
    assert!(crate::revision::prepare_order(&section_fixture(), &[101]).is_err());
    assert!(crate::revision::prepare_order(&section_fixture(), &[101, 101]).is_err());
    let p = crate::revision::prepare_order(&section_fixture(), &[102, 101]).unwrap();
    assert_eq!(p["sorts"], json!([{"id":102,"sort":1},{"id":101,"sort":2}]));
    assert_eq!(p["section"]["seasonId"], 11);
}
#[test]
fn ambiguous_edit_records_attempt_and_does_not_retry() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x/vupre/web/archive/view") {
            return (200, json!({"code":0,"data":edit_snapshot()}));
        }
        if r.path.starts_with("/x/vu/web/edit?") {
            return (500, json!({"secret":"secret-session"}));
        }
        handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.json");
    for _ in 0..2 {
        assert!(
            crate::revision::edit(
                &api,
                &edit_snapshot(),
                &new_metadata(),
                &[replacement()],
                Some(&path),
                true
            )
            .is_err()
        );
    }
    assert_eq!(
        storage::read_json::<Value>(&path).unwrap()["state"],
        "uncertain"
    );
    assert_eq!(
        server
            .snapshot()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}

#[test]
fn collection_order_posts_only_the_complete_order_and_blocks_repeat_attempts() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x2/creative/web/season/section/edit?") {
            assert_eq!(r.method, "POST");
            let p: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(p["sorts"], json!([{"id":102,"sort":1},{"id":101,"sort":2}]));
            assert_eq!(p["section"]["id"], 22);
            return (200, json!({"code":0}));
        }
        collection_handler(r, origin)
    });
    let api = Api::test(&server.origin);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.json");
    crate::revision::order(&api, 11, 22, &[102, 101], Some(&path), false).unwrap();
    assert!(!server.snapshot().iter().any(|r| r.method == "POST"));
    crate::revision::order(&api, 11, 22, &[102, 101], Some(&path), true).unwrap();
    assert!(crate::revision::order(&api, 11, 22, &[102, 101], Some(&path), true).is_err());
    assert_eq!(
        server
            .snapshot()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}

#[test]
fn collection_changes_reject_incomplete_sections_and_wrong_owners() {
    for wrong_owner in [false, true] {
        let server = Server::start(move |r, origin| {
            if wrong_owner && r.path.starts_with("/x2/creative/web/season?") {
                return (
                    200,
                    json!({"code":0,"data":{"season":{"mid":999,"is_pay":0}}}),
                );
            }
            if !wrong_owner && r.path.starts_with("/x2/creative/web/season/section?") {
                let mut s = section_fixture();
                s["section"]["epCount"] = json!(3);
                return (200, json!({"code":0,"data":s}));
            }
            collection_handler(r, origin)
        });
        assert!(
            crate::revision::add(&Api::test(&server.origin), 11, 22, "170001", None, true).is_err()
        );
        assert!(!server.snapshot().iter().any(|r| r.method == "POST"));
    }
}

#[test]
fn collection_add_is_a_noop_for_an_existing_member() {
    let server = Server::start(|r, origin| {
        if r.path.starts_with("/x2/creative/web/season/section?") {
            let mut s = section_fixture();
            s["episodes"][0]["aid"] = json!(170001);
            return (200, json!({"code":0,"data":s}));
        }
        collection_handler(r, origin)
    });
    let result =
        crate::revision::add(&Api::test(&server.origin), 11, 22, "170001", None, true).unwrap();
    assert_eq!(result["state"], "already_member");
    assert!(!server.snapshot().iter().any(|r| r.method == "POST"));
}
