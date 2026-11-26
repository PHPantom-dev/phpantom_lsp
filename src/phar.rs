//! Minimal reader for PHP phar archives.
//!
//! Parses the phar binary format to extract PHP source files without
//! requiring PHP or any external tools.  Used during Composer autoload
//! scanning to discover classes inside phar-distributed packages
//! (e.g. PHPStan).
//!
//! Only uncompressed phars are supported (compressed file entries are
//! silently skipped).  This covers the most common case — PHPStan's
//! phar contains only uncompressed files.
//!
//! Only the manifest is held in memory.  A tool phar runs to tens of
//! megabytes (PHPStan's is 26 MB), so the class scan reads it through a
//! memory map that is dropped afterwards, and the rare later request for
//! one file's source reads just that file's byte range back off disk.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use memchr::memmem;

/// The marker that ends the phar stub.
const HALT_COMPILER_MARKER: &[u8] = b"__HALT_COMPILER(); ?>";

/// Compression flag mask (bits 12–15).
const COMPRESSION_MASK: u32 = 0xF000;

/// The smallest manifest entry: name length, uncompressed size,
/// timestamp, compressed size, CRC32, flags and metadata length.
const MIN_ENTRY_LEN: usize = 7 * 4;

/// A parsed phar archive with random-access file extraction.
pub(crate) struct PharArchive {
    /// Where the archive lives, so file contents can be read on demand.
    path: PathBuf,
    /// Length and modification time the archive had when its manifest was
    /// parsed.  A lazy read checks them first: the recorded offsets mean
    /// nothing once the archive has been replaced (a `composer update`
    /// swaps a tool phar wholesale) and returning bytes from the middle of
    /// a different archive would surface as nonsense classes.
    len: u64,
    modified: Option<std::time::SystemTime>,
    /// Map of internal file path → (offset into the archive, uncompressed
    /// size).
    files: HashMap<Arc<str>, (usize, usize)>,
    /// Internal file paths in manifest order.  Iteration must be
    /// deterministic: consumers register discovered classes
    /// first-wins, so a randomized `HashMap` order would make it
    /// nondeterministic which file resolves a class name that is
    /// declared in several files of the archive (e.g. the enum and
    /// legacy-class variants of a symfony polyfill).
    paths: Vec<Arc<str>>,
}

impl PharArchive {
    /// Parse a phar archive's manifest from the archive's bytes.
    ///
    /// Only the manifest is retained; `data` is not kept, so the caller is
    /// free to map the archive, scan it, and drop the mapping.  Returns
    /// `None` if the format is invalid or unsupported.
    pub fn parse(path: &Path, data: &[u8]) -> Option<Self> {
        // 1. Find the stub end marker.
        let marker_pos = memmem::find(data, HALT_COMPILER_MARKER)?;

        // PHP writes `?>\r\n` after the marker and its reader skips a
        // `\r\n` or `\n` if one is there, but it does not insist on one:
        // hand-built archives exist whose manifest starts at the marker.
        // Try the longest terminator first, since a manifest whose length
        // field happens to begin with 0x0A would otherwise be read one
        // byte short.  Each candidate is fully validated, so a wrong guess
        // fails rather than yielding a bogus index.
        let after_marker = marker_pos + HALT_COMPILER_MARKER.len();
        let mut manifest_start = after_marker;
        if data.get(after_marker..after_marker + 2) == Some(b"\r\n") {
            manifest_start = after_marker + 2;
        } else if data.get(after_marker..after_marker + 1) == Some(b"\n") {
            manifest_start = after_marker + 1;
        }

        Self::parse_manifest_at(path, data, manifest_start).or_else(|| {
            (manifest_start != after_marker)
                .then(|| Self::parse_manifest_at(path, data, after_marker))
                .flatten()
        })
    }

    /// Parse the manifest that starts at `manifest_start`.
    ///
    /// Returns `None` when the bytes there do not describe a valid
    /// manifest, which is what lets [`parse`](Self::parse) treat the
    /// stub's line ending as optional.
    fn parse_manifest_at(path: &Path, data: &[u8], manifest_start: usize) -> Option<Self> {
        let mut cursor = manifest_start;

        // 2. Parse the manifest header.
        let manifest_length = read_u32(data, &mut cursor)? as usize;
        let manifest_end = manifest_start + 4 + manifest_length;
        if manifest_end > data.len() {
            return None;
        }

        let file_count = read_u32(data, &mut cursor)? as usize;
        let _api_version = read_u16(data, &mut cursor)?;
        let _global_flags = read_u32(data, &mut cursor)?;

        // Alias: 4-byte length + alias bytes.
        let alias_len = read_u32(data, &mut cursor)? as usize;
        if cursor + alias_len > data.len() {
            return None;
        }
        cursor += alias_len;

        // Metadata: 4-byte length + metadata bytes.
        let metadata_len = read_u32(data, &mut cursor)? as usize;
        if cursor + metadata_len > data.len() {
            return None;
        }
        cursor += metadata_len;

        // Every entry takes at least `MIN_ENTRY_LEN` bytes of the manifest, so a count the manifest cannot
        // hold is corrupt.  Rejecting it here also keeps the capacity
        // reservations below from trusting an attacker-sized number.
        if file_count > manifest_end.saturating_sub(cursor) / MIN_ENTRY_LEN {
            return None;
        }

        // 3. Parse each file entry and build the index.
        //    We collect entries first, then compute offsets into the data area.
        struct RawEntry {
            filename: Arc<str>,
            uncompressed_size: usize,
            compressed_size: usize,
            flags: u32,
        }

        let mut entries = Vec::with_capacity(file_count);

        for _ in 0..file_count {
            let filename_len = read_u32(data, &mut cursor)? as usize;
            if cursor + filename_len > data.len() {
                return None;
            }
            // A tool phar holds tens of thousands of entries, and the name
            // is needed both as a map key and in the manifest-order list;
            // sharing one allocation keeps this off the startup path.
            let filename: Arc<str> =
                Arc::from(String::from_utf8_lossy(&data[cursor..cursor + filename_len]).as_ref());
            cursor += filename_len;

            let uncompressed_size = read_u32(data, &mut cursor)? as usize;
            let _timestamp = read_u32(data, &mut cursor)?;
            let compressed_size = read_u32(data, &mut cursor)? as usize;
            let _crc32 = read_u32(data, &mut cursor)?;
            let flags = read_u32(data, &mut cursor)?;

            // Per-file metadata.
            let file_metadata_len = read_u32(data, &mut cursor)? as usize;
            if cursor + file_metadata_len > data.len() {
                return None;
            }
            cursor += file_metadata_len;

            entries.push(RawEntry {
                filename,
                uncompressed_size,
                compressed_size,
                flags,
            });
        }

        // 4. The file content area starts right after the manifest.
        //    manifest_start + 4 (manifest_length field) + manifest_length.
        let data_area_start = manifest_end;

        let mut files = HashMap::with_capacity(file_count);
        let mut paths = Vec::with_capacity(file_count);
        let mut offset = data_area_start;

        for entry in entries {
            // Only index uncompressed files (compression bits 12–15 clear).
            if entry.flags & COMPRESSION_MASK == 0 {
                if offset + entry.compressed_size > data.len() {
                    return None;
                }
                if files
                    .insert(
                        Arc::clone(&entry.filename),
                        (offset, entry.uncompressed_size),
                    )
                    .is_none()
                {
                    paths.push(entry.filename);
                }
            }
            offset += entry.compressed_size;
        }

        let metadata = std::fs::metadata(path).ok();
        Some(Self {
            path: path.to_owned(),
            len: data.len() as u64,
            modified: metadata.and_then(|m| m.modified().ok()),
            files,
            paths,
        })
    }

    /// The byte range one file inside the phar occupies.
    ///
    /// Used by the initial class scan, which already has the whole archive
    /// mapped and can slice it directly.
    pub fn file_range(&self, internal_path: &str) -> Option<std::ops::Range<usize>> {
        let &(offset, size) = self.files.get(internal_path)?;
        Some(offset..offset + size)
    }

    /// Read the content of a file inside the phar from disk.
    ///
    /// Returns `None` when the archive has been replaced since it was
    /// parsed, since the recorded offsets no longer describe it.
    pub fn read_file(&self, internal_path: &str) -> Option<Vec<u8>> {
        let range = self.file_range(internal_path)?;
        let mut file = std::fs::File::open(&self.path).ok()?;
        let metadata = file.metadata().ok()?;
        if metadata.len() != self.len || metadata.modified().ok() != self.modified {
            return None;
        }
        file.seek(SeekFrom::Start(range.start as u64)).ok()?;
        let mut buf = vec![0u8; range.len()];
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// Iterate over all file paths in the archive, in manifest order.
    pub fn file_paths(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(Arc::as_ref)
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Read a little-endian `u32` from `data` at `*cursor`, advancing the cursor.
fn read_u32(data: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = *cursor + 4;
    if end > data.len() {
        return None;
    }
    let bytes: [u8; 4] = data[*cursor..end].try_into().ok()?;
    *cursor = end;
    Some(u32::from_le_bytes(bytes))
}

/// Read a little-endian `u16` from `data` at `*cursor`, advancing the cursor.
fn read_u16(data: &[u8], cursor: &mut usize) -> Option<u16> {
    let end = *cursor + 2;
    if end > data.len() {
        return None;
    }
    let bytes: [u8; 2] = data[*cursor..end].try_into().ok()?;
    *cursor = end;
    Some(u16::from_le_bytes(bytes))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid phar archive in memory with the given files.
    ///
    /// Each entry is `(filename, content)`.  All files are stored uncompressed.
    fn build_test_phar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();

        // ── Stub ────────────────────────────────────────────────────
        buf.extend_from_slice(b"<?php __HALT_COMPILER(); ?>\n");

        // ── Manifest ────────────────────────────────────────────────
        // We build the manifest body first so we can compute its length.
        let mut manifest_body = Vec::new();

        // File count (u32 LE).
        manifest_body.extend_from_slice(&(files.len() as u32).to_le_bytes());
        // API version 1.1.0 → 0x1100 stored LE (only 2 bytes).
        manifest_body.extend_from_slice(&0x1100u16.to_le_bytes());
        // Global flags (0 = no signature).
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        // Alias length + alias (empty).
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        // Metadata length + metadata (empty).
        manifest_body.extend_from_slice(&0u32.to_le_bytes());

        // File entries.
        for (name, content) in files {
            let name_bytes = name.as_bytes();
            // Filename length + filename.
            manifest_body.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            manifest_body.extend_from_slice(name_bytes);
            // Uncompressed size.
            manifest_body.extend_from_slice(&(content.len() as u32).to_le_bytes());
            // Timestamp.
            manifest_body.extend_from_slice(&0u32.to_le_bytes());
            // Compressed size (same as uncompressed — no compression).
            manifest_body.extend_from_slice(&(content.len() as u32).to_le_bytes());
            // CRC32 (0 for testing).
            manifest_body.extend_from_slice(&0u32.to_le_bytes());
            // Flags (0 = uncompressed, no signature verification).
            manifest_body.extend_from_slice(&0u32.to_le_bytes());
            // Metadata length (0).
            manifest_body.extend_from_slice(&0u32.to_le_bytes());
        }

        // Manifest length (does NOT include the 4 bytes of itself).
        buf.extend_from_slice(&(manifest_body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&manifest_body);

        // ── File contents ───────────────────────────────────────────
        for (_name, content) in files {
            buf.extend_from_slice(content);
        }

        buf
    }

    /// Write phar bytes to a temporary file and parse them, returning the
    /// archive together with the directory keeping the file alive.
    ///
    /// `read_file` reads from disk, so the file has to outlive the archive.
    fn parse_test_phar(bytes: &[u8]) -> Option<(tempfile::TempDir, PharArchive)> {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.phar");
        std::fs::write(&path, bytes).expect("write phar");
        let archive = PharArchive::parse(&path, bytes)?;
        Some((dir, archive))
    }

    #[test]
    fn parse_minimal_phar() {
        let phar_bytes = build_test_phar(&[
            ("src/Foo.php", b"<?php class Foo {}"),
            ("src/Bar.php", b"<?php class Bar {}"),
        ]);

        let (_dir, archive) = parse_test_phar(&phar_bytes).expect("should parse valid phar");

        assert_eq!(archive.files.len(), 2);
        assert!(archive.files.contains_key("src/Foo.php"));
        assert!(archive.files.contains_key("src/Bar.php"));
    }

    #[test]
    fn read_file_returns_correct_content() {
        let foo_content = b"<?php class Foo { public function hello() {} }";
        let bar_content = b"<?php class Bar extends Foo {}";

        let phar_bytes =
            build_test_phar(&[("src/Foo.php", foo_content), ("src/Bar.php", bar_content)]);

        let (_dir, archive) = parse_test_phar(&phar_bytes).expect("should parse");

        assert_eq!(
            archive.read_file("src/Foo.php").as_deref(),
            Some(&foo_content[..])
        );
        assert_eq!(
            archive.read_file("src/Bar.php").as_deref(),
            Some(&bar_content[..])
        );
        assert_eq!(archive.read_file("src/Missing.php"), None);
    }

    #[test]
    fn file_paths_lists_all_files() {
        let phar_bytes = build_test_phar(&[
            ("a.php", b"<?php // a"),
            ("b.php", b"<?php // b"),
            ("c.php", b"<?php // c"),
        ]);

        let (_dir, archive) = parse_test_phar(&phar_bytes).expect("should parse");

        let mut paths: Vec<&str> = archive.file_paths().collect();
        paths.sort();

        assert_eq!(paths, vec!["a.php", "b.php", "c.php"]);
    }

    #[test]
    fn file_paths_preserves_manifest_order() {
        let phar_bytes = build_test_phar(&[
            ("zeta.php", b"<?php // z"),
            ("alpha.php", b"<?php // a"),
            ("mid.php", b"<?php // m"),
        ]);

        let (_dir, archive) = parse_test_phar(&phar_bytes).expect("should parse");

        let paths: Vec<&str> = archive.file_paths().collect();
        assert_eq!(paths, vec!["zeta.php", "alpha.php", "mid.php"]);
    }

    /// Rebuild `bytes` with the stub's trailing line ending replaced.
    ///
    /// `build_test_phar` emits `__HALT_COMPILER(); ?>\n`; the manifest is
    /// position-independent, so swapping that one byte is enough to
    /// produce the other stub spellings.
    fn with_terminator(bytes: &[u8], terminator: &[u8]) -> Vec<u8> {
        let pos =
            memmem::find(bytes, HALT_COMPILER_MARKER).expect("marker") + HALT_COMPILER_MARKER.len();
        let mut out = bytes[..pos].to_vec();
        out.extend_from_slice(terminator);
        out.extend_from_slice(&bytes[pos + 1..]);
        out
    }

    #[test]
    fn stub_line_ending_is_optional() {
        // PHP's own reader accepts a manifest that starts right at the
        // marker; phars built by hand (e.g. pdepend's 0.1.1 release) do.
        let base = build_test_phar(&[("src/Foo.php", b"<?php class Foo {}")]);

        for terminator in [&b""[..], &b"\n"[..], &b"\r\n"[..]] {
            let bytes = with_terminator(&base, terminator);
            let (_dir, archive) = parse_test_phar(&bytes)
                .unwrap_or_else(|| panic!("should parse with {terminator:?}"));
            assert_eq!(
                archive.read_file("src/Foo.php").as_deref(),
                Some(&b"<?php class Foo {}"[..]),
                "terminator {terminator:?}"
            );
        }
    }

    #[test]
    fn no_terminator_with_newline_shaped_manifest_length() {
        // With no line ending, the manifest length's low byte sits right
        // after the marker.  Pad the archive until that byte is 0x0A so it
        // looks like the `\n` the reader would skip; the skipped-byte
        // reading must fail validation and fall back to the real offset.
        let mut padding = 0usize;
        let bytes = loop {
            assert!(padding < 512, "no padding produced a 0x0A length byte");
            let name = format!("src/{}.php", "a".repeat(padding + 1));
            let base = build_test_phar(&[(name.as_str(), b"<?php class Foo {}")]);
            let candidate = with_terminator(&base, b"");
            let pos = memmem::find(&candidate, HALT_COMPILER_MARKER).expect("marker")
                + HALT_COMPILER_MARKER.len();
            if candidate[pos] == b'\n' {
                break candidate;
            }
            padding += 1;
        };

        let (_dir, archive) = parse_test_phar(&bytes).expect("should fall back past the false \\n");
        assert_eq!(archive.file_paths().count(), 1);
    }

    #[test]
    fn parse_returns_none_for_garbage() {
        assert!(parse_test_phar(&[0, 1, 2, 3]).is_none());
        assert!(parse_test_phar(&[]).is_none());
    }

    #[test]
    fn file_count_larger_than_the_manifest_is_rejected() {
        // A count of 0xFFFFFFFF once went straight into `Vec::with_capacity`
        // and aborted the process on a 171 GB allocation.
        let mut bytes = build_test_phar(&[("src/Foo.php", b"<?php class Foo {}")]);
        let count_pos = memmem::find(&bytes, HALT_COMPILER_MARKER).expect("marker")
            + HALT_COMPILER_MARKER.len()
            + 1 // `\n`
            + 4; // manifest length
        bytes[count_pos..count_pos + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_test_phar(&bytes).is_none());

        // One more entry than the manifest holds is rejected too.
        bytes[count_pos..count_pos + 4].copy_from_slice(&2u32.to_le_bytes());
        assert!(parse_test_phar(&bytes).is_none());
    }

    #[test]
    fn crlf_line_ending_after_marker() {
        let foo_content = b"<?php class Foo {}";

        // Build the phar manually with \r\n after the marker.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"<?php __HALT_COMPILER(); ?>\r\n");

        // Manifest body.
        let mut manifest_body = Vec::new();
        manifest_body.extend_from_slice(&1u32.to_le_bytes()); // file count
        manifest_body.extend_from_slice(&0x1100u16.to_le_bytes()); // API version
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // global flags
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // alias len
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // metadata len

        // Single file entry.
        let name = b"Foo.php";
        manifest_body.extend_from_slice(&(name.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(name);
        manifest_body.extend_from_slice(&(foo_content.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // timestamp
        manifest_body.extend_from_slice(&(foo_content.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // crc32
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // flags
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // metadata len

        buf.extend_from_slice(&(manifest_body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&manifest_body);
        buf.extend_from_slice(foo_content);

        let (_dir, archive) = parse_test_phar(&buf).expect("should parse with CRLF");
        assert_eq!(
            archive.read_file("Foo.php").as_deref(),
            Some(&foo_content[..])
        );
    }

    #[test]
    fn read_file_refuses_a_replaced_archive() {
        let phar_bytes = build_test_phar(&[("src/Foo.php", b"<?php class Foo {}")]);
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.phar");
        std::fs::write(&path, &phar_bytes).expect("write phar");
        let archive = PharArchive::parse(&path, &phar_bytes).expect("should parse");

        // A `composer update` swaps the whole archive, which leaves the
        // recorded offsets describing a file that no longer exists.
        std::fs::write(&path, b"replaced with something else entirely").expect("replace phar");

        assert_eq!(archive.read_file("src/Foo.php"), None);
    }

    #[test]
    fn compressed_entries_are_skipped() {
        // Build a phar with one uncompressed and one "compressed" entry.
        let good_content = b"<?php class Good {}";
        let compressed_content = b"compressed-data";

        let mut buf = Vec::new();
        buf.extend_from_slice(b"<?php __HALT_COMPILER(); ?>\n");

        let mut manifest_body = Vec::new();
        manifest_body.extend_from_slice(&2u32.to_le_bytes()); // 2 files
        manifest_body.extend_from_slice(&0x1100u16.to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // alias
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // metadata

        // Entry 1: uncompressed.
        let name1 = b"Good.php";
        manifest_body.extend_from_slice(&(name1.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(name1);
        manifest_body.extend_from_slice(&(good_content.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        manifest_body.extend_from_slice(&(good_content.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        manifest_body.extend_from_slice(&0u32.to_le_bytes()); // flags = 0 (uncompressed)
        manifest_body.extend_from_slice(&0u32.to_le_bytes());

        // Entry 2: zlib compressed (flag 0x1000).
        let name2 = b"Compressed.php";
        manifest_body.extend_from_slice(&(name2.len() as u32).to_le_bytes());
        manifest_body.extend_from_slice(name2);
        manifest_body.extend_from_slice(&100u32.to_le_bytes()); // uncompressed
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        manifest_body.extend_from_slice(&(compressed_content.len() as u32).to_le_bytes()); // compressed
        manifest_body.extend_from_slice(&0u32.to_le_bytes());
        manifest_body.extend_from_slice(&0x1000u32.to_le_bytes()); // flags: zlib
        manifest_body.extend_from_slice(&0u32.to_le_bytes());

        buf.extend_from_slice(&(manifest_body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&manifest_body);
        buf.extend_from_slice(good_content);
        buf.extend_from_slice(compressed_content);

        let (_dir, archive) = parse_test_phar(&buf).expect("should parse");

        // Good.php should be readable.
        assert_eq!(
            archive.read_file("Good.php").as_deref(),
            Some(&good_content[..])
        );
        // Compressed.php should be skipped.
        assert!(archive.read_file("Compressed.php").is_none());
        // Only one file path should be listed.
        let paths: Vec<&str> = archive.file_paths().collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "Good.php");
    }
}
