//! Official passport QR login; session replacement happens only after /nav confirms it.
use crate::{api::Api, auth::Session, storage};
use anyhow::{Context, Result, bail, ensure};
use qrcode::{
    QrCode,
    render::{svg, unicode},
};
use reqwest::{blocking::Client, header::SET_COOKIE};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

fn data(response: reqwest::blocking::Response) -> Result<(Value, BTreeMap<String, String>)> {
    ensure!(
        response.status().is_success(),
        "QR login HTTP request rejected"
    );
    let mut cookies = BTreeMap::new();
    for h in response.headers().get_all(SET_COOKIE) {
        let pair = h
            .to_str()
            .context("Invalid login cookie header")?
            .split(';')
            .next()
            .unwrap_or("");
        if let Some((name, value)) = pair.split_once('=') {
            cookies.insert(name.trim().to_owned(), value.to_owned());
        }
    }
    let value: Value = response
        .json()
        .map_err(|_| anyhow::anyhow!("Invalid QR login response"))?;
    ensure!(
        value["code"].as_i64() == Some(0),
        "QR login service rejected request"
    );
    Ok((value["data"].clone(), cookies))
}
fn challenge(value: &Value) -> Result<(&str, &str)> {
    let key = value["qrcode_key"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("QR key missing")?;
    let url = value["url"].as_str().context("QR URL missing")?;
    let parsed = url::Url::parse(url).context("Invalid QR URL")?;
    ensure!(
        parsed.scheme() == "https"
            && matches!(
                parsed.host_str(),
                Some("passport.bilibili.com" | "account.bilibili.com")
            )
            && parsed.username().is_empty()
            && parsed.password().is_none(),
        "Unexpected QR login origin"
    );
    Ok((key, url))
}
fn state(value: &Value) -> Result<i64> {
    let code = value["code"].as_i64().context("QR status missing")?;
    match code {
        0 | 86101 | 86090 => Ok(code),
        86038 => bail!("QR code expired; run auth login again"),
        _ => bail!("QR login was rejected or changed; no session saved"),
    }
}
pub fn render(url: &str, file: Option<&Path>) -> Result<()> {
    let qr = QrCode::new(url.as_bytes()).context("Cannot encode QR")?;
    eprintln!(
        "Scan with the Bilibili app and confirm login:\n{}",
        qr.render::<unicode::Dense1x2>().build()
    );
    if let Some(file) = file {
        storage::write_private(
            file,
            qr.render::<svg::Color>()
                .min_dimensions(512, 512)
                .build()
                .as_bytes(),
            false,
        )?;
        eprintln!("QR SVG: {}", file.display());
    }
    Ok(())
}
pub fn login(session_path: &Path, qr_file: Option<&Path>, timeout: u64) -> Result<Value> {
    ensure!(
        (1..=600).contains(&timeout),
        "Timeout must be 1..600 seconds"
    );
    let http = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?;
    let (generated, mut cookies) = data(
        http.get("https://passport.bilibili.com/x/passport-login/web/qrcode/generate")
            .send()
            .map_err(|_| anyhow::anyhow!("Cannot request Bilibili login QR"))?,
    )?;
    let (key, url) = challenge(&generated)?;
    render(url, qr_file)?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut scanned = false;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(1));
        let (value, received) = data(
            http.get("https://passport.bilibili.com/x/passport-login/web/qrcode/poll")
                .query(&[("qrcode_key", key)])
                .send()
                .map_err(|_| anyhow::anyhow!("QR status request failed; no session saved"))?,
        )?;
        cookies.extend(received);
        match state(&value)? {
            86090 if !scanned => {
                eprintln!("Scanned; waiting for phone confirmation");
                scanned = true;
            }
            0 => {
                // Only authentication cookies are needed; preference values may contain Unicode.
                cookies.retain(|k, _| {
                    matches!(
                        k.as_str(),
                        "SESSDATA"
                            | "bili_jct"
                            | "DedeUserID"
                            | "DedeUserID__ckMd5"
                            | "buvid3"
                            | "buvid4"
                    )
                });
                let candidate =
                    Session::parse(&serde_json::to_string(&json!({"cookies":cookies}))?)?;
                let check = Api::new(Some(Session::parse(&serde_json::to_string(&candidate)?)?))?
                    .status()?;
                return save_verified(candidate, session_path, check);
            }
            _ => {}
        }
    }
    bail!("QR login timed out; existing session kept")
}
fn save_verified(candidate: Session, path: &Path, check: Value) -> Result<Value> {
    ensure!(
        check["is_login"] == true && check["mid"].as_u64().is_some_and(|m| m > 0),
        "QR confirmed but authenticated profile check failed; existing session kept"
    );
    candidate.save(path)?;
    Ok(json!({"is_login":true,"mid":check["mid"],"name":check["name"],"session_file":path}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qr_export_never_overwrites_an_existing_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        Session::parse("SESSDATA=fixture; bili_jct=fixture")
            .unwrap()
            .save(&path)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(
            render(
                "https://account.bilibili.com/h5/account-h5/auth/scan-web",
                Some(&path)
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn failed_profile_keeps_original_session_and_verified_success_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let old = Session::parse("SESSDATA=old-fixture; bili_jct=old-csrf").unwrap();
        old.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        for check in [
            json!({"is_login":false,"mid":3}),
            json!({"is_login":true,"mid":null}),
        ] {
            let candidate = Session::parse("SESSDATA=new-fixture; bili_jct=new-csrf").unwrap();
            assert!(save_verified(candidate, &path, check).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let candidate = Session::parse("SESSDATA=new-fixture; bili_jct=new-csrf").unwrap();
        assert!(
            save_verified(
                candidate,
                &path,
                json!({"is_login":true,"mid":123,"name":"Fixture"})
            )
            .is_ok()
        );
        assert_eq!(
            Session::load(&path).unwrap().cookies["SESSDATA"],
            "new-fixture"
        );
    }
    #[test]
    fn validates_actual_passport_challenge() {
        assert!(challenge(&json!({"qrcode_key":"fixture","url":"https://passport.bilibili.com/h5-app/passport/login/scan?test=1"})).is_ok());
        assert!(challenge(&json!({"qrcode_key":"fixture","url":"https://account.bilibili.com/h5/account-h5/auth/scan-web"})).is_ok());
        for url in [
            "https://passport.bilibili.com.evil.invalid/",
            "http://passport.bilibili.com/",
            "https://user@passport.bilibili.com/",
        ] {
            assert!(challenge(&json!({"qrcode_key":"fixture","url":url})).is_err());
        }
    }
    #[test]
    fn separates_pending_confirmed_expired_and_unknown() {
        for code in [0, 86101, 86090] {
            assert_eq!(state(&json!({"code":code})).unwrap(), code);
        }
        for code in [86038, 123] {
            assert!(state(&json!({"code":code})).is_err());
        }
    }
}
