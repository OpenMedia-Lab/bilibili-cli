use bilibili_cli::{auth::Session, storage::write_private};
use std::fs;

#[test]
fn imports_common_cookie_exports_without_ever_printing_values() {
    let inputs = [
        "SESSDATA=secret-session; bili_jct=secret-csrf; DedeUserID=123",
        r#"[{"name":"SESSDATA","value":"secret-session","domain":".bilibili.com"},{"name":"bili_jct","value":"secret-csrf","domain":".bilibili.com"}]"#,
        r#"{"cookie_info":{"cookies":[{"name":"SESSDATA","value":"secret-session"},{"name":"bili_jct","value":"secret-csrf"}]}}"#,
        "# Netscape HTTP Cookie File\n#HttpOnly_.bilibili.com\tTRUE\t/\tTRUE\t0\tSESSDATA\tsecret-session\n.bilibili.com\tTRUE\t/\tTRUE\t0\tbili_jct\tsecret-csrf\n",
    ];
    for input in inputs {
        let session = Session::parse(input).unwrap();
        assert_eq!(session.cookies["SESSDATA"], "secret-session");
        assert!(session.header().unwrap().is_sensitive());
    }
}

#[test]
fn refuses_foreign_cookie_domains_and_header_injection_without_echoing_secrets() {
    for input in [
        "SESSDATA=secret\r\nX-Stolen: yes; bili_jct=token",
        r#"[{"name":"SESSDATA","value":"secret","domain":"evil.example"},{"name":"bili_jct","value":"token","domain":"evil.example"}]"#,
        "SESSDATA=secret",
    ] {
        let message = Session::parse(input).err().unwrap().to_string();
        assert!(!message.contains("secret"));
        assert!(!message.contains("token"));
    }
}

#[test]
fn private_writes_do_not_clobber_receipts_or_follow_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("receipt.json");
    write_private(&target, b"first", false).unwrap();
    assert!(write_private(&target, b"second", false).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"first");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(write_private(&link, b"stolen", true).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"first");
    }
}
