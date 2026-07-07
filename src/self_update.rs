//! Self-update functionality for phpantom_lsp.
//!
//! Downloads the latest release from GitHub and replaces the current
//! binary.  Supports `.tar.gz` (Unix) and `.zip` (Windows) archives.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::time::Duration;

const REPO_OWNER: &str = "PHPantom-dev";
const REPO_NAME: &str = "phpantom_lsp";
const BIN_NAME: &str = "phpantom_lsp";

/// The current version, set by build.rs from `git describe` or
/// `CARGO_PKG_VERSION`.
const VERSION: &str = env!("PHPANTOM_GIT_VERSION");

// ─── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum UpdateError {
    /// HTTP or network error.
    Http(String),
    /// JSON parsing error.
    Json(String),
    /// No matching release asset for this platform.
    NoAsset(String),
    /// I/O error (download, extract, replace).
    Io(io::Error),
    /// User cancelled.
    Cancelled,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(msg) => write!(f, "HTTP error: {msg}"),
            Self::Json(msg) => write!(f, "JSON parse error: {msg}"),
            Self::NoAsset(msg) => write!(f, "{msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Cancelled => write!(f, "Update cancelled"),
        }
    }
}

impl From<io::Error> for UpdateError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Outcome of a successful update check or install.
#[derive(Debug)]
pub enum UpdateStatus {
    /// The current binary is already the latest release.
    UpToDate(String),
    /// An update is available but was not installed (`--check` mode).
    UpdateAvailable(String),
    /// The binary was replaced with the given release version.
    Updated(String),
}

/// Run the self-update flow.
///
/// - `check_only`: if true, only check for updates without installing.
/// - `no_confirm`: if true, skip the confirmation prompt.
pub fn run(check_only: bool, no_confirm: bool) -> Result<UpdateStatus, UpdateError> {
    let target = current_target()?;

    eprintln!("Current version: {VERSION}");
    eprintln!("Platform: {target}");
    eprintln!();

    // 1. Fetch latest release info from GitHub.
    eprintln!("Checking for updates...");
    let release = fetch_latest_release()?;

    eprintln!("Latest release: {} ({})", release.tag, release.version);

    // 2. Compare versions.
    if !is_newer(VERSION, &release.version) {
        return Ok(UpdateStatus::UpToDate(release.version));
    }

    eprintln!();
    eprintln!("Update available: {VERSION} -> {}", release.version);

    if check_only {
        return Ok(UpdateStatus::UpdateAvailable(release.version));
    }

    // 3. Find the matching asset.
    let archive_ext = if target.contains("windows") {
        ".zip"
    } else {
        ".tar.gz"
    };
    let asset_name = format!("{BIN_NAME}-{target}{archive_ext}");
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            let available: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
            UpdateError::NoAsset(format!(
                "No release asset for platform '{target}' (expected '{asset_name}').\n\
                 Available assets: {available:?}\n\
                 You may need to build from source: cargo install --path . --locked"
            ))
        })?;

    eprintln!("Asset: {}", asset.name);

    // 4. Confirm.
    if !no_confirm {
        eprint!("\nThe existing binary will be replaced. Continue? [Y/n] ");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if !input.is_empty() && input != "y" {
            return Err(UpdateError::Cancelled);
        }
        eprintln!();
    }

    // 5. Download to temp file.
    eprintln!("Downloading {}...", asset.name);
    let tmp_dir = tempfile::tempdir()?;
    let archive_path = tmp_dir.path().join(&asset.name);
    download_asset(&asset.download_url, &archive_path)?;

    // 6. Extract binary.
    eprintln!("Extracting...");
    let bin_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let extracted = if archive_ext == ".zip" {
        extract_from_zip(&archive_path, BIN_NAME, bin_suffix, tmp_dir.path())?
    } else {
        extract_from_tar_gz(&archive_path, BIN_NAME, bin_suffix, tmp_dir.path())?
    };

    // 7. Replace current binary.
    eprintln!("Replacing binary...");
    self_replace::self_replace(&extracted).map_err(|e| UpdateError::Io(io::Error::other(e)))?;

    eprintln!();
    eprintln!(
        "Successfully updated to {} ({})",
        release.tag, release.version
    );

    Ok(UpdateStatus::Updated(release.version))
}

// ─── Platform detection ─────────────────────────────────────────────────────

/// Map the current OS/ARCH to the Rust target triple used in release
/// asset names.
fn current_target() -> Result<String, UpdateError> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    let target = match (arch, os) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        _ => {
            return Err(UpdateError::NoAsset(format!(
                "Unsupported platform: {arch}-{os}. Build from source instead."
            )));
        }
    };

    Ok(target.to_string())
}

/// Check whether `release_version` is strictly newer than
/// `current_version`.  Compares only the `major.minor.patch`
/// components, ignoring git suffixes like `-72-g49514663-dirty`.  A dev
/// build whose base version matches the release is considered
/// up-to-date.
fn is_newer(current_version: &str, release_version: &str) -> bool {
    let current = parse_semver(current_version.strip_prefix('v').unwrap_or(current_version));
    let release = parse_semver(release_version.strip_prefix('v').unwrap_or(release_version));
    release > current
}

/// Parse a version string into `(major, minor, patch)`, stripping any
/// pre-release or git-describe suffix (everything after the first `-`
/// that follows a digit).
fn parse_semver(version: &str) -> (u64, u64, u64) {
    // Strip suffixes: "0.8.0-72-gabcdef-dirty" → "0.8.0"
    let core = version
        .find(['-', '+'])
        .map(|i| &version[..i])
        .unwrap_or(version);

    let mut parts = core.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

// ─── GitHub API ─────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Release {
    tag: String,
    version: String,
    assets: Vec<Asset>,
}

#[derive(Debug)]
struct Asset {
    name: String,
    download_url: String,
}

fn fetch_latest_release() -> Result<Release, UpdateError> {
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .new_agent();

    let response = agent
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", &format!("{BIN_NAME}/{VERSION}"))
        .call()
        .map_err(|e| UpdateError::Http(e.to_string()))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| UpdateError::Http(e.to_string()))?;

    parse_release_json(&body)
}

fn parse_release_json(json: &str) -> Result<Release, UpdateError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| UpdateError::Json(e.to_string()))?;

    let tag = value["tag_name"]
        .as_str()
        .ok_or_else(|| UpdateError::Json("missing 'tag_name'".into()))?
        .to_string();

    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();

    let assets = value["assets"]
        .as_array()
        .ok_or_else(|| UpdateError::Json("missing 'assets'".into()))?
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            let download_url = a["browser_download_url"].as_str()?.to_string();
            Some(Asset { name, download_url })
        })
        .collect();

    Ok(Release {
        tag,
        version,
        assets,
    })
}

// ─── Download ───────────────────────────────────────────────────────────────

fn download_asset(url: &str, dest: &std::path::Path) -> Result<(), UpdateError> {
    // No global timeout: the binary download may legitimately take a
    // while on slow connections. Connect and response-header timeouts
    // still prevent an indefinite hang on an unresponsive server.
    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .new_agent();

    let response = agent
        .get(url)
        .header("User-Agent", &format!("{BIN_NAME}/{VERSION}"))
        .call()
        .map_err(|e| UpdateError::Http(e.to_string()))?;

    let mut file = fs::File::create(dest)?;
    let mut reader = response.into_body().into_reader();
    io::copy(&mut reader, &mut file)?;
    file.flush()?;

    Ok(())
}

// ─── Archive extraction ─────────────────────────────────────────────────────

/// Extract the binary from a `.tar.gz` archive.
///
/// Looks for any entry whose file name matches `{bin_name}{suffix}`
/// (e.g. `phpantom_lsp`), regardless of directory nesting.
fn extract_from_tar_gz(
    archive_path: &std::path::Path,
    bin_name: &str,
    suffix: &str,
    out_dir: &std::path::Path,
) -> Result<std::path::PathBuf, UpdateError> {
    let file = fs::File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let expected_name = format!("{bin_name}{suffix}");

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if file_name == expected_name {
            let dest = out_dir.join(&expected_name);
            let mut out_file = fs::File::create(&dest)?;
            io::copy(&mut entry, &mut out_file)?;

            // Make executable on Unix.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
            }

            return Ok(dest);
        }
    }

    Err(UpdateError::NoAsset(format!(
        "Binary '{expected_name}' not found in archive"
    )))
}

/// Extract the binary from a `.zip` archive.
fn extract_from_zip(
    archive_path: &std::path::Path,
    bin_name: &str,
    suffix: &str,
    out_dir: &std::path::Path,
) -> Result<std::path::PathBuf, UpdateError> {
    let file = fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| UpdateError::Io(io::Error::other(e)))?;

    let expected_name = format!("{bin_name}{suffix}");

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| UpdateError::Io(io::Error::other(e)))?;

        let file_name = match entry.enclosed_name().and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        }) {
            Some(n) => n,
            None => continue,
        };

        if file_name == expected_name {
            let dest = out_dir.join(&expected_name);
            let mut out_file = fs::File::create(&dest)?;
            io::copy(&mut entry, &mut out_file)?;
            return Ok(dest);
        }
    }

    Err(UpdateError::NoAsset(format!(
        "Binary '{expected_name}' not found in archive"
    )))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "self_update_tests.rs"]
mod tests;
