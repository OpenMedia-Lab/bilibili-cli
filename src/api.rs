use crate::auth::Session;
use anyhow::{Context, Result, ensure};
use reqwest::{
    Method,
    blocking::{Client, RequestBuilder},
    header::{COOKIE, HeaderValue},
};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

pub struct Api {
    pub(crate) http: reqwest::blocking::Client,
    pub(crate) session: Option<Session>,
    pub(crate) api_origin: String,
    pub(crate) member_origin: String,
    pub(crate) testing: bool,
}
impl Api {
    pub fn new(session: Option<Session>) -> Result<Self> {
        let http = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; OpenMedia-BilibiliCLI/0.1; +https://github.com/OpenMedia-Lab/bilibili-cli)")
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build().context("Cannot initialize HTTP client")?;
        Ok(Self {
            http,
            session,
            api_origin: "https://api.bilibili.com".into(),
            member_origin: "https://member.bilibili.com".into(),
            testing: false,
        })
    }
    #[cfg(test)]
    pub(crate) fn test(origin: &str) -> Self {
        let mut api = Self::new(Some(
            Session::parse("SESSDATA=secret-session; bili_jct=secret-csrf").unwrap(),
        ))
        .unwrap();
        api.api_origin = origin.into();
        api.member_origin = origin.into();
        api.testing = true;
        api
    }
    pub(crate) fn request(
        &self,
        method: Method,
        member: bool,
        path: &str,
        auth: bool,
    ) -> Result<RequestBuilder> {
        let origin = if member {
            &self.member_origin
        } else {
            &self.api_origin
        };
        let mut request = self
            .http
            .request(method, format!("{origin}{path}"))
            .header("Referer", "https://member.bilibili.com/")
            .header("Origin", "https://member.bilibili.com");
        if auth {
            request = request.header(
                COOKIE,
                self.session
                    .as_ref()
                    .context("No session; run bilibili auth import")?
                    .header()?,
            );
        }
        Ok(request)
    }
    pub(crate) fn csrf(&self) -> Result<&str> {
        self.session
            .as_ref()
            .context("No session; run bilibili auth import")?
            .csrf()
    }
    pub fn status(&self) -> Result<Value> {
        let data = envelope(
            self.request(Method::GET, false, "/x/web-interface/nav", true)?,
            "session status",
        )?;
        Ok(
            json!({"is_login": data["isLogin"].as_bool().unwrap_or(false), "mid": data["mid"], "name":data["uname"]}),
        )
    }
    pub fn account_id(&self) -> Result<u64> {
        let status = self.status()?;
        ensure!(
            status["is_login"] == true,
            "Session has expired; import fresh cookies"
        );
        status["mid"]
            .as_u64()
            .filter(|m| *m > 0)
            .context("Status response is missing account ID")
    }
    pub fn me(&self) -> Result<Value> {
        envelope(
            self.request(Method::GET, false, "/x/space/myinfo", true)?,
            "profile",
        )
    }
    pub fn video(&self, id: &str) -> Result<Value> {
        let (key, value) = video_id(id)?;
        envelope(
            self.request(Method::GET, false, "/x/web-interface/view", false)?
                .query(&[(key, value)]),
            "video detail",
        )
    }
    pub fn archives(&self, page: u32, page_size: u32, status: &str) -> Result<Value> {
        ensure!(
            page > 0 && (1..=100).contains(&page_size),
            "Page must be positive and page size 1..100"
        );
        envelope(
            self.request(Method::GET, true, "/x/web/archives", true)?
                .query(&[
                    ("pn", page.to_string()),
                    ("ps", page_size.to_string()),
                    ("status", status.to_string()),
                ]),
            "own archives",
        )
    }
    pub fn categories(&self) -> Result<Value> {
        let data = envelope(
            self.request(Method::GET, true, "/x/vupre/web/archive/pre", true)?,
            "category metadata",
        )?;
        let mut found = serde_json::Map::new();
        for name in ["typelist", "type_tree", "type_list"] {
            if let Some(value) = data.get(name) {
                found.insert(name.into(), value.clone());
            }
        }
        ensure!(
            !found.is_empty(),
            "Creator category response has changed; no recognized category list"
        );
        Ok(Value::Object(found))
    }
    pub fn upload_cover(&self, bytes: &[u8], mime: &str) -> Result<Value> {
        use base64::Engine;
        let data_url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let data = envelope(
            self.request(Method::POST, true, "/x/vu/web/cover/up", true)?
                .form(&[("cover", data_url.as_str()), ("csrf", self.csrf()?)]),
            "cover upload",
        )?;
        let url = data["url"].as_str().context("Cover response has no URL")?;
        let parsed = url::Url::parse(url).context("Cover response contains an invalid URL")?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https")
                && parsed
                    .host_str()
                    .is_some_and(|h| h == "hdslb.com" || h.ends_with(".hdslb.com")),
            "Cover response contains an untrusted URL"
        );
        Ok(json!({"url":url}))
    }
    pub(crate) fn publish(&self, payload: &Value) -> Result<Value> {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
            .to_string();
        let request = self
            .request(Method::POST, true, "/x/vu/web/add/v3", true)?
            .query(&[("t", millis.as_str()), ("csrf", self.csrf()?)])
            .json(payload);
        envelope(request, "submission")
    }
    pub(crate) fn upos_auth(&self, auth: &str) -> Result<HeaderValue> {
        let mut header = HeaderValue::from_str(auth)
            .map_err(|_| anyhow::anyhow!("Invalid scoped upload authorization"))?;
        header.set_sensitive(true);
        Ok(header)
    }
}

pub(crate) fn response(request: RequestBuilder, operation: &str) -> Result<Value> {
    let response = request.send().map_err(|error| {
        if error.is_timeout() {
            anyhow::anyhow!("{operation}: network timeout; mutation outcome may be unknown")
        } else {
            anyhow::anyhow!("{operation}: network failure; mutation outcome may be unknown")
        }
    })?;
    ensure!(
        response.status().is_success(),
        "{operation}: HTTP {}; no automatic mutation retry",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    response
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            anyhow::anyhow!("{operation}: failed to read response; outcome may be unknown")
        })?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "{operation}: response is too large"
    );
    serde_json::from_slice(&bytes).map_err(|_| {
        anyhow::anyhow!("{operation}: response was not valid JSON; outcome may be unknown")
    })
}

pub(crate) fn envelope(request: RequestBuilder, operation: &str) -> Result<Value> {
    let value = response(request, operation)?;
    let code = value["code"]
        .as_i64()
        .context("Bilibili response is missing a status code")?;
    ensure!(
        code == 0,
        "{operation}: Bilibili API code {code} (session, permissions, rate limits or validation); server body omitted"
    );
    value
        .get("data")
        .filter(|v| !v.is_null())
        .cloned()
        .context("Successful response is missing data")
}

pub fn video_id(id: &str) -> Result<(&str, &str)> {
    if id.starts_with("BV") {
        ensure!(
            id.len() == 12 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
            "BVID must be BV followed by ten letters/digits"
        );
        Ok(("bvid", id))
    } else {
        let id = id.strip_prefix("av").unwrap_or(id);
        ensure!(
            id.parse::<u64>().is_ok_and(|n| n > 0),
            "Video ID must be a positive AID/av number or a BVID"
        );
        Ok(("aid", id))
    }
}
