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

use std::collections::HashMap;

/// The marker that ends the phar stub.
const HALT_COMPILER_MARKER: &[u8] = b"__HALT_COMPILER(); ?>";

/// Compression flag mask (bits 12–15).
const COMPRESSION_MASK: u32 = 0xF000;

/// A parsed phar archive with random-access file extraction.
pub(crate) struct PharArchive {
    /// Raw bytes of the entire phar file.
    data: Vec<u8>,
    /// Map of internal file path → (offset into `data`, uncompressed size).
    files: HashMap<String, (usize, usize)>,
    /// Internal file paths in manifest order.  Iteration must be
    /// deterministic: consumers register discovered classes
    /// first-wins, so a randomized `HashMap` order would make it
    /// nondeterministic which file resolves a class name that is
    /// declared in several files of the archive (e.g. the enum and
    /// legacy-class variants of a symfony polyfill).
    paths: Vec<String>,
}

impl PharArchive {
    /// Parse a phar archive from raw bytes.
    /// Returns `None` if the format is invalid or unsupported.
    pub fn parse(data: Vec<u8>) -> Option<Self> {
        // 1. Find the stub end marker.
        let marker_pos = find_marker(&data)?;

        // The manifest starts after the marker + a line ending (\r\n or \n).
        let after_marker = marker_pos + HALT_COMPILER_MARKER.len();
        let manifest_start = if data.get(after_marker..after_marker + 2) == Some(b"\r\n") {
            after_marker + 2
        } else if data.get(after_marker..after_marker + 1) == Some(b"\n") {
            after_marker + 1
        } else {
            return None;
        };

        let mut cursor = manifest_start;

        // 2. Parse the manifest header.
        let manifest_length = read_u32(&data, &mut cursor)? as usize;
        let manifest_end = manifest_start + 4 + manifest_length;
        if manifest_end > data.len() {
            return None;
        }

        let file_count = read_u32(&data, &mut cursor)? as usize;
        let _api_version = read_u16(&data, &mut cursor)?;
        let _global_flags = read_u32(&data, &mut cursor)?;

        // Alias: 4-byte length + alias bytes.
        let alias_len = read_u32(&data, &mut cursor)? as usize;
        if cursor + alias_len > data.len() {
            return None;
        }
        cursor += alias_len; // skip alias bytes

        // Metadata: 4-byte length + metadata bytes.
        let metadata_len = read_u32(&data, &mut cursor)? as usize;
        if cursor + metadata_len > data.len() {
            return None;
        }
        cursor += metadata_len; // skip metadata bytes

        // 3. Parse each file entry and build the index.
        //    We collect entries first, then compute offsets into the data area.
        struct RawEntry {
            filename: String,
            uncompressed_size: usize,
            compressed_size: usize,
            flags: u32,
        }

        let mut entries = Vec::with_capacity(file_count);

        for _ in 0..file_count {
            let filename_len = read_u32(&data, &mut cursor)? as usize;
            if cursor + filename_len > data.len() {
                return None;
            }
            let filename =
                String::from_utf8_lossy(&data[cursor..cursor + filename_len]).into_owned();
            cursor += filename_len;

            let uncompressed_size = read_u32(&data, &mut cursor)? as usize;
            let _timestamp = read_u32(&data, &mut cursor)?;
            let compressed_size = read_u32(&data, &mut cursor)? as usize;
            let _crc32 = read_u32(&data, &mut cursor)?;
            let flags = read_u32(&data, &mut cursor)?;

            // Per-file metadata.
            let file_metadata_len = read_u32(&data, &mut cursor)? as usize;
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

        for entry in &entries {
            // Only index uncompressed files (compression bits 12–15 clear).
            if entry.flags & COMPRESSION_MASK == 0 {
                if offset + entry.compressed_size > data.len() {
                    return None;
                }
                if files
                    .insert(entry.filename.clone(), (offset, entry.uncompressed_size))
                    .is_none()
                {
                    paths.push(entry.filename.clone());
                }
            }
            offset += entry.compressed_size;
        }

        Some(Self { data, files, paths })
    }

    /// Extract the content of a file inside the phar.
    pub fn read_file(&self, internal_path: &str) -> Option<&[u8]> {
        let &(offset, size) = self.files.get(internal_path)?;
        self.data.get(offset..offset + size)
    }

    /// Iterate over all file paths in the archive, in manifest order.
    pub fn file_paths(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(String::as_str)
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Find the byte offset where `__HALT_COMPILER(); ?>` begins.
fn find_marker(data: &[u8]) -> Option<usize> {
    let marker = HALT_COMPILER_MARKER;
    let len = marker.len();
    if data.len() < len {
        return None;
    }
    for i in 0..=data.len() - len {
        if &data[i..i + len] == marker {
            return Some(i);
        }
    }
    None
}

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

    #[test]
    fn parse_minimal_phar() {
        let phar_bytes = build_test_phar(&[
            ("src/Foo.php", b"<?php class Foo {}"),
            ("src/Bar.php", b"<?php class Bar {}"),
        ]);

        let archive = PharArchive::parse(phar_bytes).expect("should parse valid phar");

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

        let archive = PharArchive::parse(phar_bytes).expect("should parse");

        assert_eq!(
            archive.read_file("src/Foo.php"),
            Some(foo_content.as_slice())
        );
        assert_eq!(
            archive.read_file("src/Bar.php"),
            Some(bar_content.as_slice())
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

        let archive = PharArchive::parse(phar_bytes).expect("should parse");

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

        let archive = PharArchive::parse(phar_bytes).expect("should parse");

        let paths: Vec<&str> = archive.file_paths().collect();
        assert_eq!(paths, vec!["zeta.php", "alpha.php", "mid.php"]);
    }

    #[test]
    fn parse_returns_none_for_garbage() {
        assert!(PharArchive::parse(vec![0, 1, 2, 3]).is_none());
        assert!(PharArchive::parse(Vec::new()).is_none());
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

        let archive = PharArchive::parse(buf).expect("should parse with CRLF");
        assert_eq!(archive.read_file("Foo.php"), Some(foo_content.as_slice()));
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

        let archive = PharArchive::parse(buf).expect("should parse");

        // Good.php should be readable.
        assert_eq!(archive.read_file("Good.php"), Some(good_content.as_slice()));
        // Compressed.php should be skipped.
        assert!(archive.read_file("Compressed.php").is_none());
        // Only one file path should be listed.
        let paths: Vec<&str> = archive.file_paths().collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], "Good.php");
    }
}
