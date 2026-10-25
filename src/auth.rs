use crate::storage;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub cookies: BTreeMap<String, String>,
}
impl Session {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let mut cookies = BTreeMap::new();
        if input.starts_with('{') || input.starts_with('[') {
            let value: Value =
                serde_json::from_str(input).map_err(|_| anyhow::anyhow!("Invalid cookie JSON"))?;
            let items = value
                .pointer("/cookie_info/cookies")
                .or_else(|| value.get("cookies"))
                .unwrap_or(&value);
            if let Some(items) = items.as_array() {
                for item in items {
                    if let Some(domain) = item.get("domain").and_then(Value::as_str)
                        && !bilibili_domain(domain)
                    {
                        continue;
                    }
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .context("Cookie entry has no name")?;
                    let value = item
                        .get("value")
                        .and_then(Value::as_str)
                        .context("Cookie entry has no value")?;
                    insert(&mut cookies, name, value)?;
                }
            } else if let Some(items) = items.as_object() {
                for (name, value) in items {
                    insert(
                        &mut cookies,
                        name,
                        value.as_str().context("Cookie values must be strings")?,
                    )?;
                }
            } else {
                bail!("Unsupported cookie JSON format");
            }
        } else if input.contains('\t') {
            for line in input.lines() {
                let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let fields: Vec<_> = line.split('\t').collect();
                ensure!(fields.len() == 7, "Invalid Netscape cookie row");
                if bilibili_domain(fields[0]) {
                    insert(&mut cookies, fields[5], fields[6])?;
                }
            }
        } else {
            let input = input.strip_prefix("Cookie:").unwrap_or(input).trim();
            ensure!(
                !input.contains(['\r', '\n']),
                "Cookie header must be a single line"
            );
            for pair in input.split(';').filter(|p| !p.trim().is_empty()) {
                let (name, value) = pair
                    .trim()
                    .split_once('=')
                    .context("Invalid cookie header")?;
                insert(&mut cookies, name.trim(), value.trim())?;
            }
        }
        for required in ["SESSDATA", "bili_jct"] {
            ensure!(
                cookies.get(required).is_some_and(|v| !v.is_empty()),
                "Session requires both SESSDATA and bili_jct cookies"
            );
        }
        Ok(Self { cookies })
    }
    pub fn header(&self) -> Result<reqwest::header::HeaderValue> {
        let joined = self
            .cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        let mut value = reqwest::header::HeaderValue::from_str(&joined)
            .map_err(|_| anyhow::anyhow!("Invalid session cookie header"))?;
        value.set_sensitive(true);
        Ok(value)
    }
    pub fn csrf(&self) -> Result<&str> {
        self.cookies
            .get("bili_jct")
            .map(String::as_str)
            .context("Session has no CSRF cookie")
    }
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = storage::read_private(path)?;
        Self::parse(std::str::from_utf8(&bytes).context("Session must be UTF-8")?)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        storage::write_private(path, &serde_json::to_vec_pretty(self)?, true)
    }
    pub fn default_path() -> Result<PathBuf> {
        Ok(dirs::config_dir()
            .context("No config directory; pass --session")?
            .join("bilibili-cli/session.json"))
    }
}

fn bilibili_domain(domain: &str) -> bool {
    let domain = domain.trim_start_matches('.');
    domain == "bilibili.com" || domain.ends_with(".bilibili.com")
}

fn insert(cookies: &mut BTreeMap<String, String>, name: &str, value: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "Invalid cookie name"
    );
    ensure!(
        value.bytes().all(|b| (0x21..=0x7e).contains(&b)
            && b != b';'
            && b != b','
            && b != b'"'
            && b != b'\\'),
        "Invalid cookie value"
    );
    if let Some(previous) = cookies.insert(name.to_owned(), value.to_owned()) {
        ensure!(
            previous == value,
            "Conflicting duplicate cookie entries; export only the active account"
        );
    }
    Ok(())
}
