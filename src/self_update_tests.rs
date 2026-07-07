use super::*;

// ─── parse_semver ───────────────────────────────────────────────────────────

#[test]
fn parse_semver_plain_release() {
    assert_eq!(parse_semver("0.8.0"), (0, 8, 0));
    assert_eq!(parse_semver("1.2.3"), (1, 2, 3));
    assert_eq!(parse_semver("10.20.30"), (10, 20, 30));
}

#[test]
fn parse_semver_strips_git_describe_suffix() {
    assert_eq!(parse_semver("0.8.0-72-g49514663"), (0, 8, 0));
    assert_eq!(parse_semver("0.8.0-72-g49514663-dirty"), (0, 8, 0));
}

#[test]
fn parse_semver_strips_build_metadata() {
    assert_eq!(parse_semver("1.0.0+build.5"), (1, 0, 0));
}

#[test]
fn parse_semver_missing_components_default_to_zero() {
    assert_eq!(parse_semver("1.2"), (1, 2, 0));
    assert_eq!(parse_semver("1"), (1, 0, 0));
}

#[test]
fn parse_semver_bare_hash_is_zero() {
    // A build without any reachable tag embeds just the commit hash.
    assert_eq!(parse_semver("g37a901ed"), (0, 0, 0));
    assert_eq!(parse_semver("37a901ed"), (0, 0, 0));
}

// ─── is_newer ───────────────────────────────────────────────────────────────

#[test]
fn is_newer_detects_upgrade() {
    assert!(is_newer("0.7.0", "0.8.0"));
    assert!(is_newer("0.8.0", "0.8.1"));
    assert!(is_newer("0.9.9", "1.0.0"));
}

#[test]
fn is_newer_same_version_is_up_to_date() {
    assert!(!is_newer("0.8.0", "0.8.0"));
}

#[test]
fn is_newer_never_downgrades() {
    assert!(!is_newer("0.9.0", "0.8.0"));
}

#[test]
fn is_newer_dev_build_matching_release_base_is_up_to_date() {
    // A dev build made after the 0.8.0 tag reports 0.8.0-N-g<hash>;
    // the 0.8.0 release should not be offered as an "update".
    assert!(!is_newer("0.8.0-72-g49514663-dirty", "0.8.0"));
}

#[test]
fn is_newer_dev_build_sees_next_release() {
    assert!(is_newer("0.8.0-72-g49514663", "0.9.0"));
}

#[test]
fn is_newer_handles_v_prefix() {
    assert!(is_newer("v0.7.0", "0.8.0"));
    assert!(is_newer("0.7.0", "v0.8.0"));
    assert!(!is_newer("v0.8.0", "v0.8.0"));
}

// ─── parse_release_json ─────────────────────────────────────────────────────

#[test]
fn parse_release_json_extracts_tag_and_assets() {
    let json = r#"{
        "tag_name": "0.8.0",
        "assets": [
            {
                "name": "phpantom_lsp-x86_64-unknown-linux-gnu.tar.gz",
                "browser_download_url": "https://example.com/a.tar.gz"
            },
            {
                "name": "phpantom_lsp-x86_64-pc-windows-msvc.zip",
                "browser_download_url": "https://example.com/b.zip"
            }
        ]
    }"#;

    let release = parse_release_json(json).unwrap();
    assert_eq!(release.tag, "0.8.0");
    assert_eq!(release.version, "0.8.0");
    assert_eq!(release.assets.len(), 2);
    assert_eq!(
        release.assets[0].name,
        "phpantom_lsp-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert_eq!(
        release.assets[0].download_url,
        "https://example.com/a.tar.gz"
    );
}

#[test]
fn parse_release_json_strips_v_prefix_from_version() {
    let json = r#"{"tag_name": "v1.2.3", "assets": []}"#;
    let release = parse_release_json(json).unwrap();
    assert_eq!(release.tag, "v1.2.3");
    assert_eq!(release.version, "1.2.3");
}

#[test]
fn parse_release_json_skips_malformed_assets() {
    let json = r#"{
        "tag_name": "0.8.0",
        "assets": [
            {"name": "good.tar.gz", "browser_download_url": "https://example.com/good"},
            {"name": "missing-url.tar.gz"},
            {"browser_download_url": "https://example.com/missing-name"}
        ]
    }"#;
    let release = parse_release_json(json).unwrap();
    assert_eq!(release.assets.len(), 1);
    assert_eq!(release.assets[0].name, "good.tar.gz");
}

#[test]
fn parse_release_json_missing_tag_is_error() {
    let err = parse_release_json(r#"{"assets": []}"#).unwrap_err();
    assert!(matches!(err, UpdateError::Json(_)));
}

#[test]
fn parse_release_json_missing_assets_is_error() {
    let err = parse_release_json(r#"{"tag_name": "0.8.0"}"#).unwrap_err();
    assert!(matches!(err, UpdateError::Json(_)));
}

#[test]
fn parse_release_json_invalid_json_is_error() {
    let err = parse_release_json("not json").unwrap_err();
    assert!(matches!(err, UpdateError::Json(_)));
}
