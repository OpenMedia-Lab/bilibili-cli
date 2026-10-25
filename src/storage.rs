use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_private(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(parent)
            .context("Cannot create private storage directory")?;
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        ensure!(
            !meta.file_type().is_symlink(),
            "Refusing to write through a symlink"
        );
        ensure!(replace, "File already exists; refusing to overwrite");
    }
    let target = if replace {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        parent.join(format!(".bilibili-{}-{stamp}.tmp", std::process::id()))
    } else {
        path.to_path_buf()
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&target)
        .context("Cannot create private file (it may already exist)")?;
    let result = (|| {
        file.write_all(bytes).context("Cannot write private file")?;
        file.sync_all().context("Cannot sync private file")?;
        if replace {
            fs::rename(&target, path).context("Cannot replace private file")?;
        }
        // Persist the rename before a network mutation can begin.
        #[cfg(unix)]
        {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&target);
    }
    result
}

pub fn read_private(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).context("Cannot read private file")?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Private storage must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            meta.permissions().mode() & 0o077 == 0,
            "Private file is accessible to other users; run chmod 600 on it"
        );
    }
    ensure!(
        meta.len() <= 8 * 1024 * 1024,
        "Private JSON file is too large"
    );
    fs::read(path).context("Cannot read private file")
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_private(path)?)
        .map_err(|_| anyhow::anyhow!("Invalid private JSON file"))
}

pub fn write_json<T: Serialize>(path: &Path, value: &T, replace: bool) -> Result<()> {
    write_private(path, &serde_json::to_vec_pretty(value)?, replace)
}

pub struct Lock {
    path: PathBuf,
}
impl Lock {
    pub fn acquire(receipt: &Path) -> Result<Self> {
        let mut path = receipt.as_os_str().to_owned();
        path.push(".lock");
        let path = PathBuf::from(path);
        write_private(&path, std::process::id().to_string().as_bytes(), false).context("Receipt is locked; another operation may be active. After a crash, reconcile the receipt before removing its .lock file")?;
        Ok(Self { path })
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
