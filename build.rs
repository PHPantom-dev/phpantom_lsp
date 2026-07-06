//! Build script for PHPantom.
//!
//! Parses `stubs/jetbrains/phpstorm-stubs/PhpStormStubsMap.php` and generates
//! a Rust source file (`stub_map_generated.rs`) that:
//!
//!   1. Embeds every referenced PHP stub file via `include_str!`.
//!   2. Provides static arrays mapping class names and function names to
//!      indices into the embedded file array.
//!
//! The generated file is consumed by `src/stubs.rs` at compile time.
//!
//! ## Automatic stub fetching
//!
//! If the stubs directory doesn't exist, the build script will automatically
//! fetch phpstorm-stubs from GitHub. This allows `cargo install` to work
//! without any additional setup.
//!
//! ## Pinned version with integrity verification
//!
//! The file `stubs.lock` (checked into version control) pins the exact
//! commit SHA and records the SHA-256 hash of the corresponding GitHub
//! tarball.  The build script downloads that specific commit and verifies
//! the hash before extracting.  This ensures reproducible, tamper-evident
//! builds.
//!
//! To update the pinned version run `scripts/update-stubs.sh`.
//!
//! During `cargo publish`, stubs are downloaded into `OUT_DIR` so the source
//! directory is not modified (Cargo forbids build scripts from modifying
//! anything outside `OUT_DIR` during publish/verify).
//!
//! ## Re-run strategy
//!
//! The `stubs/` directory is gitignored, so Cargo's default "re-run when
//! any package file changes" behaviour does not notice when stubs are
//! downloaded.  Explicit `rerun-if-changed` on paths inside `stubs/` also
//! fails when the directory doesn't exist yet.
//!
//! Instead we watch the project root directory (`.`).  Its mtime changes
//! whenever a direct child like `stubs/` is created or removed.  We also
//! watch `build.rs` and `stubs.lock` for targeted rebuilds.
//!
//! To avoid unnecessary recompilation of the main crate we compare the
//! newly generated content against the existing output file and only write
//! when something actually changed.  This way `rustc` sees a stable mtime
//! on `stub_map_generated.rs` and skips recompilation when the stubs
//! haven't changed.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

/// Relative path (from the stubs root) to the stubs map file.
const MAP_FILE_REL: &str = "jetbrains/phpstorm-stubs/PhpStormStubsMap.php";

/// Relative path (from the stubs root) to the actual stub PHP files.
const STUBS_DIR_REL: &str = "jetbrains/phpstorm-stubs";

/// Contents of `stubs.lock`.
struct StubsLock {
    /// GitHub repository in `owner/repo` format (e.g. `JetBrains/phpstorm-stubs`).
    repo: String,
    commit: String,
    sha256: String,
}

/// Locate the stubs root directory.
///
/// 1. If `CARGO_MANIFEST_DIR/stubs` exists (local dev), use that.
/// 2. Otherwise, download into `OUT_DIR/stubs` (safe during `cargo publish`).
fn resolve_stubs_root() -> (PathBuf, bool) {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let local_stubs = Path::new(&manifest_dir).join("stubs");

    if local_stubs.join(MAP_FILE_REL).exists() {
        // Stubs already present in source tree (normal local development).
        // Still need to check staleness via .commit marker.
        return (local_stubs, false);
    }

    // Stubs not in source tree — use OUT_DIR so we never modify the source
    // directory. This is required for `cargo publish` verification.
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_stubs = Path::new(&out_dir).join("stubs");
    let needs_fetch = !out_stubs.join(MAP_FILE_REL).exists();
    (out_stubs, needs_fetch)
}

fn main() {
    // Watch the project root directory so that creating/removing `stubs/`
    // (which is gitignored) is detected via the directory mtime change.
    // Without this, Cargo's default "any package file" check ignores
    // gitignored paths, and explicit watches on non-existent paths don't
    // reliably trigger when they first appear.
    println!("cargo:rerun-if-changed=.");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=stubs.lock");
    // Re-run when HEAD moves (commit, checkout, tag) so the embedded
    // version string stays current.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");

    // ── Git version string ──────────────────────────────────────────
    // Explicit override:  release CI sets PHPANTOM_GIT_VERSION to the
    //                     release tag, since the tag does not exist yet
    //                     when building a draft release (and shallow
    //                     checkouts have no tags for `git describe`).
    // On a release tag:  "0.6.0"
    // Between tags:      "0.6.0-186-g37a901ed"  (commits ahead + hash)
    // No tags at all:    "g37a901ed"             (just the hash)
    // Dirty worktree:    any of the above + "-dirty"
    // No git at all:     falls back to CARGO_PKG_VERSION (crates.io builds)
    println!("cargo:rerun-if-env-changed=PHPANTOM_GIT_VERSION");
    let git_version = env::var("PHPANTOM_GIT_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["describe", "--tags", "--always", "--dirty"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=PHPANTOM_GIT_VERSION={git_version}");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let lock = read_stubs_lock(&manifest_dir);

    let (stubs_root, mut needs_fetch) = resolve_stubs_root();

    // Check whether the stubs need to be (re-)fetched.  A `.commit`
    // marker file inside the stubs directory records which commit was
    // last downloaded.  When `stubs.lock` pins a different commit the
    // stubs are stale and must be replaced.
    if !needs_fetch {
        let stubs_path = stubs_root.join(STUBS_DIR_REL);
        let commit_marker = stubs_path.join(".commit");
        if let Ok(marker) = fs::read_to_string(&commit_marker) {
            if marker.trim() != lock.commit {
                needs_fetch = true;
            }
        } else {
            // Stubs exist but no marker — written before this check was
            // added.  Treat as stale so we re-fetch and create the marker.
            needs_fetch = true;
        }
    }

    if needs_fetch {
        let stubs_path = stubs_root.join(STUBS_DIR_REL);
        if stubs_path.exists() {
            eprintln!(
                "cargo:warning=Stubs are stale (expected commit {}), re-fetching...",
                &lock.commit[..lock.commit.len().min(10)]
            );
            // Remove the old stubs so fetch_stubs writes a clean tree.
            let _ = fs::remove_dir_all(&stubs_path);
        } else {
            eprintln!("cargo:warning=Stubs not found, fetching from GitHub...");
        }
        if let Err(e) = fetch_stubs(&stubs_root, &lock) {
            eprintln!("cargo:warning=Failed to fetch stubs from GitHub: {}", e);
            eprintln!("cargo:warning=Building without stubs (network may be unavailable).");
            println!("cargo:rustc-env=PHPANTOM_STUBS_VERSION=none");
            write_empty_stubs();
            return;
        }
    }

    let stubs_path = stubs_root.join(STUBS_DIR_REL);
    let map_path = stubs_root.join(MAP_FILE_REL);

    // Emit the stubs version so the binary can log it at runtime.
    let short = &lock.commit[..lock.commit.len().min(10)];
    println!("cargo:rustc-env=PHPANTOM_STUBS_VERSION=master@{}", short);

    let map_content = match fs::read_to_string(&map_path) {
        Ok(c) => c,
        Err(e) => {
            // If stubs aren't installed yet, generate an empty map so the
            // build still succeeds (just without built-in stubs).
            eprintln!(
                "cargo:warning=Could not read PhpStormStubsMap.php ({}); generating empty stub index",
                e
            );
            write_empty_stubs();
            return;
        }
    };

    // ── Parse the three sections ────────────────────────────────────────

    let class_map = parse_section(&map_content, "CLASSES");
    let function_map = parse_section(&map_content, "FUNCTIONS");
    let constant_map = parse_section(&map_content, "CONSTANTS");

    // ── Collect unique file paths ───────────────────────────────────────

    let mut unique_files = BTreeSet::new();
    for path in class_map.values() {
        unique_files.insert(path.as_str());
    }
    for path in function_map.values() {
        unique_files.insert(path.as_str());
    }
    for path in constant_map.values() {
        unique_files.insert(path.as_str());
    }

    // Only keep files that actually exist on disk.
    let existing_files: Vec<&str> = unique_files
        .iter()
        .copied()
        .filter(|rel| stubs_path.join(rel).is_file())
        .collect();

    // Build a path → index mapping.
    let file_index: BTreeMap<&str, usize> = existing_files
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, i))
        .collect();

    // ── Generate Rust source ────────────────────────────────────────────

    let mut out = String::with_capacity(512 * 1024);

    // 1. The embedded file array.
    out.push_str("/// Embedded PHP stub file contents.\n");
    out.push_str("///\n");
    out.push_str("/// Each entry corresponds to one PHP file from phpstorm-stubs,\n");
    out.push_str("/// embedded at compile time via `include_str!`.\n");
    out.push_str(&format!(
        "pub(crate) static STUB_FILES: [&str; {}] = [\n",
        existing_files.len()
    ));
    for rel_path in &existing_files {
        // Build the include_str! path relative to the generated file's
        // location ($OUT_DIR).  We use an absolute path rooted at the stubs
        // directory to avoid fragile relative path arithmetic.
        let abs = stubs_path.join(rel_path);
        let abs_str = abs.to_string_lossy().replace('\\', "/");
        out.push_str(&format!("    include_str!(\"{}\")", abs_str));
        out.push_str(",\n");
    }
    out.push_str("];\n\n");

    // 2. Class name → file index mapping.
    let class_entries: Vec<(&str, usize)> = class_map
        .iter()
        .filter_map(|(name, path)| {
            file_index
                .get(path.as_str())
                .map(|&idx| (name.as_str(), idx))
        })
        .collect();

    out.push_str("/// Maps PHP class/interface/trait short names to an index into\n");
    out.push_str("/// [`STUB_FILES`].\n");
    out.push_str(&format!(
        "pub(crate) static STUB_CLASS_MAP: [(&str, usize); {}] = [\n",
        class_entries.len()
    ));
    for (name, idx) in &class_entries {
        out.push_str(&format!("    (\"{}\", {}),\n", escape(name), idx));
    }
    out.push_str("];\n\n");

    // 3. Function name → file index mapping.
    let function_entries: Vec<(&str, usize)> = function_map
        .iter()
        .filter_map(|(name, path)| {
            file_index
                .get(path.as_str())
                .map(|&idx| (name.as_str(), idx))
        })
        .collect();

    out.push_str("/// Maps PHP function names (including namespaced ones) to an index\n");
    out.push_str("/// into [`STUB_FILES`].\n");
    out.push_str(&format!(
        "pub(crate) static STUB_FUNCTION_MAP: [(&str, usize); {}] = [\n",
        function_entries.len()
    ));
    for (name, idx) in &function_entries {
        out.push_str(&format!("    (\"{}\", {}),\n", escape(name), idx));
    }
    out.push_str("];\n\n");

    // 4. Constant name → file index mapping.
    let constant_entries: Vec<(&str, usize)> = constant_map
        .iter()
        .filter_map(|(name, path)| {
            file_index
                .get(path.as_str())
                .map(|&idx| (name.as_str(), idx))
        })
        .collect();

    out.push_str("/// Maps PHP constant names (including namespaced ones) to an index\n");
    out.push_str("/// into [`STUB_FILES`].\n");
    out.push_str(&format!(
        "pub(crate) static STUB_CONSTANT_MAP: [(&str, usize); {}] = [\n",
        constant_entries.len()
    ));
    for (name, idx) in &constant_entries {
        out.push_str(&format!("    (\"{}\", {}),\n", escape(name), idx));
    }
    out.push_str("];\n");

    write_if_changed(&out);
}

// ── Stub fetching ───────────────────────────────────────────────────────

/// Read `stubs.lock` and return the pinned commit + hash.
///
/// Panics if the file is missing or malformed — `stubs.lock` is checked
/// into version control and must always be present.
fn read_stubs_lock(manifest_dir: &str) -> StubsLock {
    let lock_path = Path::new(manifest_dir).join("stubs.lock");
    let content = fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("Failed to read stubs.lock: {}\nThis file is required and should be checked into version control.", e));

    let mut repo: Option<String> = None;
    let mut commit: Option<String> = None;
    let mut sha256: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "repo" => repo = Some(value.to_string()),
                "commit" => commit = Some(value.to_string()),
                "sha256" => sha256 = Some(value.to_string()),
                _ => {}
            }
        }
    }

    StubsLock {
        repo: repo.unwrap_or_else(|| "JetBrains/phpstorm-stubs".to_string()),
        commit: commit.expect("stubs.lock is missing 'commit' field"),
        sha256: sha256.expect("stubs.lock is missing 'sha256' field"),
    }
}

/// Compute the SHA-256 hex digest of a byte slice.
fn sha256_hex(data: &[u8]) -> String {
    // Minimal SHA-256 implementation to avoid adding a build-dependency.
    // Based on the FIPS 180-4 specification.

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    #[inline(always)]
    fn ch(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (!x & z)
    }
    #[inline(always)]
    fn maj(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (x & z) ^ (y & z)
    }
    #[inline(always)]
    fn ep0(x: u32) -> u32 {
        x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
    }
    #[inline(always)]
    fn ep1(x: u32) -> u32 {
        x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
    }
    #[inline(always)]
    fn sig0(x: u32) -> u32 {
        x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
    }
    #[inline(always)]
    fn sig1(x: u32) -> u32 {
        x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
    }

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pre-processing: pad the message.
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block.
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            w[i] = sig1(w[i - 2])
                .wrapping_add(w[i - 7])
                .wrapping_add(sig0(w[i - 15]))
                .wrapping_add(w[i - 16]);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let t1 = hh
                .wrapping_add(ep1(e))
                .wrapping_add(ch(e, f, g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = ep0(a).wrapping_add(maj(a, b, c));
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    format!(
        "{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
        h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]
    )
}

fn fetch_stubs(stubs_root: &Path, lock: &StubsLock) -> Result<(), Box<dyn std::error::Error>> {
    let short = &lock.commit[..lock.commit.len().min(10)];
    let tarball_url = format!(
        "https://github.com/{}/archive/{}.tar.gz",
        lock.repo, lock.commit
    );

    eprintln!(
        "cargo:warning=Downloading phpstorm-stubs from {} pinned at {}",
        lock.repo, short
    );

    let mut tarball_response = ureq::get(&tarball_url)
        .header("User-Agent", "phpantom-lsp-build")
        .call()?;

    let mut tarball_bytes = Vec::new();
    tarball_response
        .body_mut()
        .as_reader()
        .read_to_end(&mut tarball_bytes)?;

    // Verify the SHA-256 hash against stubs.lock.
    let actual_hash = sha256_hex(&tarball_bytes);
    if actual_hash != lock.sha256 {
        return Err(format!(
            "SHA-256 mismatch for phpstorm-stubs tarball!\n  \
             expected: {}\n  \
             actual:   {}\n  \
             Run scripts/update-stubs.sh to refresh stubs.lock.",
            lock.sha256, actual_hash
        )
        .into());
    }
    eprintln!("cargo:warning=SHA-256 verified: {}", actual_hash);

    let decoder = GzDecoder::new(&tarball_bytes[..]);
    let mut archive = Archive::new(decoder);

    let target_dir = stubs_root.join("jetbrains/phpstorm-stubs");
    fs::create_dir_all(&target_dir)?;

    // Safety: disable platform-specific features we don't need.
    archive.set_unpack_xattrs(false);
    archive.set_preserve_permissions(false);

    // GitHub tarballs have a top-level directory like "phpstorm-stubs-abc1234/"
    // We need to strip that prefix when extracting.
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        let components: Vec<_> = path.components().collect();
        if components.len() <= 1 {
            continue;
        }

        let relative_path: std::path::PathBuf = components[1..].iter().collect();

        // Path confinement: reject any relative path that could escape
        // the target directory via `..` or absolute/prefix components.
        // We use component inspection instead of `canonicalize` because
        // the target directory may not exist yet.
        {
            use std::path::Component;
            let mut safe = true;
            for comp in relative_path.components() {
                match comp {
                    Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                        safe = false;
                        break;
                    }
                    _ => {}
                }
            }
            if !safe {
                continue;
            }
        }

        let dest_path = target_dir.join(&relative_path);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else if entry.header().entry_type().is_file() {
            let mut file = fs::File::create(&dest_path)?;
            std::io::copy(&mut entry, &mut file)?;
        }
    }

    // Write a commit marker so subsequent builds can detect staleness.
    let commit_marker = target_dir.join(".commit");
    fs::write(&commit_marker, &lock.commit)?;

    eprintln!(
        "cargo:warning=Successfully downloaded phpstorm-stubs from {} @ {}",
        lock.repo, short
    );
    Ok(())
}

// ── Stub map generation ─────────────────────────────────────────────────

/// Write an empty stub map when stubs aren't available.
fn write_empty_stubs() {
    let content = concat!(
        "pub(crate) static STUB_FILES: [&str; 0] = [];\n",
        "pub(crate) static STUB_CLASS_MAP: [(&str, usize); 0] = [];\n",
        "pub(crate) static STUB_FUNCTION_MAP: [(&str, usize); 0] = [];\n",
        "pub(crate) static STUB_CONSTANT_MAP: [(&str, usize); 0] = [];\n",
    );
    write_if_changed(content);
}

/// Parse one of the `const CLASSES = array(...)`, `const FUNCTIONS = array(...)`,
/// or `const CONSTANTS = array(...)` sections from the PhpStormStubsMap.php file.
///
/// Returns a `BTreeMap<String, String>` of `symbol_name → relative_file_path`.
fn parse_section(content: &str, section_name: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();

    // Find the start: `const SECTION = array (`
    let marker = format!("const {} = array (", section_name);
    let start = match content.find(&marker) {
        Some(pos) => pos + marker.len(),
        None => return map,
    };

    // Walk line by line until we hit `);`
    for line in content[start..].lines() {
        let trimmed = line.trim();
        if trimmed == ");" {
            break;
        }

        // Lines look like:  'ClassName' => 'relative/path.php',
        if let Some(entry) = parse_map_entry(trimmed) {
            map.insert(entry.0, entry.1);
        }
    }

    map
}

/// Parse a single `'key' => 'value',` line.
fn parse_map_entry(line: &str) -> Option<(String, String)> {
    // Strip leading whitespace and trailing comma.
    let trimmed = line.trim().trim_end_matches(',');

    // Split on ` => `.
    let (lhs, rhs) = trimmed.split_once(" => ")?;

    // Strip surrounding single quotes.
    let key = lhs.trim().strip_prefix('\'')?.strip_suffix('\'')?;
    let value = rhs.trim().strip_prefix('\'')?.strip_suffix('\'')?;

    // Unescape PHP single-quoted string escapes:
    //   `\\` → `\`   and   `\'` → `'`
    // This is needed because the PhpStormStubsMap.php file uses PHP
    // single-quoted strings where namespace separators are written as
    // `\\` (e.g. `'Couchbase\\GetUserOptions'` → `Couchbase\GetUserOptions`).
    let key = php_unescape_single_quoted(key);
    let value = php_unescape_single_quoted(value);

    Some((key, value))
}

/// Unescape a PHP single-quoted string value.
///
/// PHP single-quoted strings only recognise two escape sequences:
///   - `\\` → `\`
///   - `\'` → `'`
fn php_unescape_single_quoted(s: &str) -> String {
    s.replace("\\\\", "\x00")
        .replace("\\'", "'")
        .replace('\x00', "\\")
}

/// Escape a string for embedding in a Rust string literal.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Write the generated file only if its content has actually changed.
///
/// This avoids bumping the mtime on `stub_map_generated.rs` when nothing
/// changed, which in turn prevents `rustc` from unnecessarily recompiling
/// the main crate.
fn write_if_changed(content: &str) {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("stub_map_generated.rs");

    if let Ok(existing) = fs::read_to_string(&dest_path)
        && existing == content
    {
        return;
    }

    fs::write(&dest_path, content).expect("Failed to write generated stub map");
}
