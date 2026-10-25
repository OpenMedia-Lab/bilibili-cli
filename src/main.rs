use anyhow::{Context, Result, ensure};
use bilibili_cli::{
    api::Api,
    auth::Session,
    media, storage,
    submission::{self, Metadata},
    upload::UploadReceipt,
};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "bilibili",
    version,
    about = "Bilibili creator CLI. Submission defaults to an offline preview.",
    long_about = "Native Rust Bilibili CLI. Uses an existing web session. Uploading media is separate from submission; only submit --publish submits to Bilibili for review/publication. A preview is local and is not a remote draft."
)]
struct Cli {
    /// Private session JSON path (default: OS config directory/bilibili-cli/session.json)
    #[arg(long, global = true)]
    session: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import, inspect, or remove a local web session
    Auth {
        #[command(subcommand)]
        command: Auth,
    },
    /// Check the current session without exposing cookie values
    Status,
    /// Read the authenticated user's profile
    Me,
    /// Read, validate, or upload video files
    Video {
        #[command(subcommand)]
        command: Video,
    },
    /// Validate or upload a JPEG/PNG cover
    Cover {
        #[command(subcommand)]
        command: Cover,
    },
    /// Read a bounded page of your creator-center archives
    Archives {
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long, default_value_t = 20)]
        page_size: u32,
        #[arg(long, default_value = "is_pubing,pubed,not_pubed")]
        status: String,
    },
    /// Read current creator-center category metadata (requires a session)
    Categories,
    /// Validate local media and metadata; no network requests or remote draft
    Plan {
        #[arg(long)]
        metadata: PathBuf,
        #[arg(long, required = true)]
        video: Vec<PathBuf>,
    },
    /// Build a local preview; --publish explicitly submits for review/publication
    Submit {
        #[arg(long)]
        metadata: PathBuf,
        /// Completed upload receipt; repeat for ordered multipart video
        #[arg(long, required = true)]
        upload: Vec<PathBuf>,
        /// Submit to Bilibili; without this flag, produce an offline preview only
        #[arg(long)]
        publish: bool,
        /// Submission receipt (default: OS data directory, keyed by payload hash)
        #[arg(long)]
        result: Option<PathBuf>,
    },
    /// Reconcile a submission receipt against your archives; never resubmits
    Readback {
        receipt: PathBuf,
        /// Maximum creator-center pages to inspect, 1..20
        #[arg(long, default_value_t = 5)]
        max_pages: u32,
        /// Also check whether the public video detail endpoint can see it
        #[arg(long)]
        public: bool,
    },
    /// Inspect a receipt safely, omitting partial-upload authorization
    Receipt { path: PathBuf },
}

#[derive(Subcommand)]
enum Auth {
    /// Import raw Cookie, Netscape, browser JSON, or biliup JSON (never an argv secret)
    Import {
        #[arg(long, required_unless_present = "stdin", conflicts_with = "stdin")]
        file: Option<PathBuf>,
        #[arg(long, required_unless_present = "file", conflicts_with = "file")]
        stdin: bool,
    },
    /// Check the local session and current server login state
    Status,
    /// Delete the local session; this does not revoke the browser session
    Logout,
}

#[derive(Subcommand)]
enum Video {
    /// Validate a local MP4 with ffprobe and compute its SHA-256
    Check { path: PathBuf },
    /// Read public video details by BVID or AID (no login required)
    Show { id: String },
    /// Upload media only; keep a private receipt for reuse/resume
    Upload {
        path: PathBuf,
        #[arg(long)]
        receipt: PathBuf,
        /// Resume an interrupted upload of the same file and UPOS object
        #[arg(long)]
        resume: bool,
    },
}

#[derive(Subcommand)]
enum Cover {
    /// Inspect JPEG/PNG signatures and dimensions locally
    Check { path: PathBuf },
    /// Upload a cover to Bilibili (does not submit a video)
    Upload {
        path: PathBuf,
        /// Save the returned cover URL to a private JSON file
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
}

fn authenticated(path: Option<&Path>) -> Result<Api> {
    let path = path
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(Session::default_path)?;
    Api::new(Some(Session::load(&path)?))
}

fn status(path: Option<&Path>) -> Result<Value> {
    let path = path
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(Session::default_path)?;
    if !path.exists() {
        return Ok(json!({"session_exists":false,"is_login":false,"session_file":path}));
    }
    let mut value = authenticated(Some(&path))?.status()?;
    value["session_exists"] = json!(true);
    value["session_file"] = json!(path);
    Ok(value)
}

fn run(cli: Cli) -> Result<Value> {
    let session_path = cli.session.as_deref();
    match cli.command {
        Command::Status
        | Command::Auth {
            command: Auth::Status,
        } => status(session_path),
        Command::Auth {
            command: Auth::Import { file, stdin: _ },
        } => {
            let bytes = if let Some(path) = file {
                ensure!(
                    fs::metadata(&path)
                        .context("Cannot open cookie import file")?
                        .len()
                        <= 1024 * 1024,
                    "Cookie import file exceeds 1 MiB"
                );
                fs::read(path).context("Cannot read cookie import file")?
            } else {
                let mut bytes = Vec::new();
                std::io::stdin()
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 1024 * 1024, "Cookie import exceeds 1 MiB");
                bytes
            };
            let session = Session::parse(
                std::str::from_utf8(&bytes).context("Cookie import must be UTF-8")?,
            )?;
            let path = cli.session.map(Ok).unwrap_or_else(Session::default_path)?;
            session.save(&path)?;
            Ok(json!({"imported":true,"session_file":path,"next":"bilibili status"}))
        }
        Command::Auth {
            command: Auth::Logout,
        } => {
            let path = cli.session.map(Ok).unwrap_or_else(Session::default_path)?;
            if path.exists() {
                ensure!(
                    !fs::symlink_metadata(&path)?.file_type().is_symlink(),
                    "Refusing to remove a symlink"
                );
                fs::remove_file(&path)?;
            }
            Ok(json!({"local_session_removed":true,"session_file":path}))
        }
        Command::Me => authenticated(session_path)?.me(),
        Command::Categories => authenticated(session_path)?.categories(),
        Command::Archives {
            page,
            page_size,
            status,
        } => authenticated(session_path)?.archives(page, page_size, &status),
        Command::Video {
            command: Video::Check { path },
        } => Ok(serde_json::to_value(media::check_video(&path)?)?),
        Command::Video {
            command: Video::Show { id },
        } => Api::new(None)?.video(&id),
        Command::Video {
            command:
                Video::Upload {
                    path,
                    receipt,
                    resume,
                },
        } => {
            let media = media::check_video(&path)?;
            let upload = authenticated(session_path)?.upload(&path, &media, &receipt, resume)?;
            let mut summary = upload.summary();
            summary["receipt_file"] = json!(receipt);
            Ok(summary)
        }
        Command::Cover {
            command: Cover::Check { path },
        } => {
            let cover = media::check_cover(&path)?;
            Ok(
                json!({"mime":cover.mime,"bytes":cover.bytes.len(),"width":cover.width,"height":cover.height}),
            )
        }
        Command::Cover {
            command: Cover::Upload { path, receipt },
        } => {
            if let Some(path) = &receipt {
                ensure!(
                    !path.exists(),
                    "Cover receipt exists; reuse its URL or choose a new receipt"
                );
            }
            let cover = media::check_cover(&path)?;
            let value = authenticated(session_path)?.upload_cover(&cover.bytes, cover.mime)?;
            if let Some(path) = receipt {
                storage::write_json(&path, &value, false)?;
            }
            Ok(value)
        }
        Command::Plan { metadata, video } => {
            let metadata = Metadata::load(&metadata)?;
            metadata.validate()?;
            ensure!(
                metadata.part_titles.is_empty() || metadata.part_titles.len() == video.len(),
                "part_titles must match local video count"
            );
            let media = video
                .iter()
                .map(|path| Ok(json!({"path":path,"media":media::check_video(path)?})))
                .collect::<Result<Vec<_>>>()?;
            Ok(
                json!({"mode":"local_plan","remote_draft":false,"metadata":metadata,"files":media,"next":"Upload cover/video explicitly, then submit without --publish to review the exact payload"}),
            )
        }
        Command::Submit {
            metadata,
            upload,
            publish,
            result,
        } => {
            let metadata = Metadata::load(&metadata)?;
            let receipts = upload
                .iter()
                .map(|path| UploadReceipt::load(path))
                .collect::<Result<Vec<_>>>()?;
            let prepared = metadata.prepare(&receipts)?;
            let result = result
                .map(Ok)
                .unwrap_or_else(|| submission::default_result(&prepared))?;
            let api = if publish {
                authenticated(session_path)?
            } else {
                Api::new(None)?
            };
            let mut value = submission::execute(&api, &prepared, &result, publish)
                .with_context(|| format!("Submission receipt: {}", result.display()))?;
            value["submission_receipt"] = json!(result);
            Ok(value)
        }
        Command::Readback {
            receipt,
            max_pages,
            public,
        } => readback(&authenticated(session_path)?, &receipt, max_pages, public),
        Command::Receipt { path } => {
            let value: Value = storage::read_json(&path)?;
            if value.get("media").is_some() {
                Ok(UploadReceipt::load(&path)?.summary())
            } else if value.get("url").is_some() {
                Ok(json!({"kind":"cover","url":value["url"]}))
            } else {
                Ok(
                    json!({"schema_version":value["schema_version"],"state":value["state"],"owner_mid":value["owner_mid"],"aid":value["aid"],"bvid":value["bvid"],"title":value["title"],"created_at":value["created_at"],"payload_sha256":value["payload_sha256"]}),
                )
            }
        }
    }
}

fn readback(api: &Api, path: &Path, max_pages: u32, public: bool) -> Result<Value> {
    ensure!((1..=20).contains(&max_pages), "max-pages must be in 1..20");
    let receipt: Value = storage::read_json(path)?;
    ensure!(
        receipt["schema_version"] == 1 && receipt.get("media").is_none(),
        "Expected a submission receipt"
    );
    let owner = api.account_id()?;
    ensure!(
        receipt["owner_mid"].as_u64() == Some(owner),
        "Receipt belongs to another account"
    );
    let aid = receipt["aid"].as_u64();
    let bvid = receipt["bvid"].as_str();
    ensure!(
        aid.is_some() || bvid.is_some(),
        "Receipt has no known video ID. Inspect bilibili archives and match the title/time manually; do not blindly submit again"
    );
    let mut found = None;
    let mut pages_checked = 0;
    for page in 1..=max_pages {
        let data = api.archives(page, 100, "is_pubing,pubed,not_pubed")?;
        pages_checked += 1;
        let audits = data
            .get("arc_audits")
            .or_else(|| data.get("archives"))
            .and_then(Value::as_array)
            .context("Archive response has no recognized item list")?;
        for item in audits {
            let archive = item
                .get("Archive")
                .or_else(|| item.get("archive"))
                .unwrap_or(item);
            let aid_matches = aid.is_some_and(|id| archive["aid"].as_u64() == Some(id));
            let bvid_matches = bvid.is_some_and(|id| archive["bvid"].as_str() == Some(id));
            if aid_matches || bvid_matches {
                found = Some(item.clone());
                break;
            }
        }
        if found.is_some() || audits.len() < 100 {
            break;
        }
    }
    let mut value = json!({"submission_state":receipt["state"],"aid":aid,"bvid":bvid,"found_in_creator_center":found.is_some(),"pages_checked":pages_checked,"archive":found,"note":"Acceptance, review and public visibility are different states. A missing result is not permission to submit again."});
    if public {
        let id = bvid
            .map(str::to_owned)
            .or_else(|| aid.map(|n| n.to_string()))
            .context("No video ID")?;
        value["public_view"] = match api.video(&id) {
            Ok(data) => json!({"available":true,"data":data}),
            Err(error) => json!({"available":false,"error":error.to_string()}),
        };
    }
    Ok(value)
}

fn main() {
    match run(Cli::parse()) {
        Ok(value) => println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("JSON serialization")
        ),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
    }
}
