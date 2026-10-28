use bilibili_cli::storage;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bilibili"))
}

#[test]
fn help_documents_preview_and_explicit_publish() {
    let output = cli().args(["submit", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--publish"));
    assert!(help.contains("preview"));
}

#[test]
fn submit_defaults_to_offline_preview_without_loading_any_session() {
    let dir = tempfile::tempdir().unwrap();
    let metadata = dir.path().join("metadata.json");
    fs::write(
        &metadata,
        r#"{"title":"Transformer / MoE","tid":201,"tags":["人工智能"]}"#,
    )
    .unwrap();
    let upload = dir.path().join("upload.json");
    storage::write_json(&upload,&json!({"schema_version":1,"owner_mid":123,"state":"complete","filename":"uploaded-key","media":{"bytes":100,"sha256":"fixture","duration_seconds":2.0,"width":1920,"height":1080,"video_codec":"h264","audio_codec":"aac"}}),false).unwrap();
    let output = cli()
        .args([
            "--session",
            dir.path().join("does-not-exist").to_str().unwrap(),
            "submit",
            "--metadata",
            metadata.to_str().unwrap(),
            "--upload",
            upload.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["mode"], "preview");
    assert_eq!(value["remote_draft"], false);
    assert_eq!(value["payload"]["videos"][0]["filename"], "uploaded-key");
}

#[test]
fn stdin_import_does_not_echo_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let mut child = cli()
        .args([
            "--session",
            session.to_str().unwrap(),
            "auth",
            "import",
            "--stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"SESSDATA=test-only-secret; bili_jct=test-only-token")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("test-only"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("test-only"));
    assert!(session.exists());
}

#[test]
fn incomplete_receipt_cannot_be_previewed_as_a_ready_submission() {
    let dir = tempfile::tempdir().unwrap();
    let metadata = dir.path().join("metadata.json");
    let upload = dir.path().join("upload.json");
    fs::write(&metadata, r#"{"title":"Test","tid":201,"tags":["Rust"]}"#).unwrap();
    storage::write_json(&upload,&json!({"schema_version":1,"owner_mid":123,"state":"uploading","filename":null,"media":{"bytes":100,"sha256":"fixture","duration_seconds":2.0,"width":1920,"height":1080,"video_codec":"h264","audio_codec":null}}),false).unwrap();
    let output = cli()
        .args([
            "submit",
            "--metadata",
            metadata.to_str().unwrap(),
            "--upload",
            upload.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("complete"));
}

#[test]
fn cover_receipt_inspection_preserves_the_returned_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cover.json");
    storage::write_json(
        &path,
        &json!({"url":"https://i0.hdslb.com/fixture.png"}),
        false,
    )
    .unwrap();
    let output = cli()
        .args(["receipt", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["url"], "https://i0.hdslb.com/fixture.png");
}

#[test]
fn edit_defaults_to_offline_preview_without_loading_a_session() {
    let dir = tempfile::tempdir().unwrap();
    let metadata = dir.path().join("metadata.json");
    let snapshot = dir.path().join("snapshot.json");
    let upload = dir.path().join("upload.json");
    fs::write(
        &metadata,
        r#"{"title":"Updated","tid":201,"tags":["Rust"]}"#,
    )
    .unwrap();
    storage::write_json(&snapshot, &json!({"archive":{"aid":170001,"bvid":"BV17x411w7KC","mid":123,"state":0},"videos":[{"cid":10,"filename":"old-file"}],"replace_check":{"can_replace":true}}), false).unwrap();
    storage::write_json(&upload,&json!({"schema_version":1,"owner_mid":123,"state":"complete","filename":"new-file","cid":20,"media":{"bytes":100,"sha256":"fixture","duration_seconds":2.0,"width":1920,"height":1080,"video_codec":"h264","audio_codec":"aac"}}),false).unwrap();
    let output = cli()
        .args([
            "--session",
            dir.path().join("missing.json").to_str().unwrap(),
            "edit",
            "--snapshot",
            snapshot.to_str().unwrap(),
            "--metadata",
            metadata.to_str().unwrap(),
            "--upload",
            upload.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["mode"], "preview");
    assert_eq!(value["plan"]["aid"], 170001);
    assert_eq!(value["plan"]["replacement_cid"], 20);
    assert!(!std::path::Path::new(value["result_file"].as_str().unwrap()).exists());
}
