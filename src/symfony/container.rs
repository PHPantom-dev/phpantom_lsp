//! Static Symfony compiled-container discovery.
//!
//! Compiled containers are PHP source, but loading one would execute project
//! code. This adapter only reads text and recovers the small pieces of runtime
//! wiring PHPantom needs.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use globset::Glob;
use ignore::WalkBuilder;

use crate::config::SymfonyContainerConfig;
use crate::text_scan::{decode_php_string_literal, find_matching_forward};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventSubscription {
    pub event: String,
    pub listener_fqn: String,
    pub method: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CompiledContainerMetadata {
    pub path: PathBuf,
    pub subscriptions: Vec<EventSubscription>,
    pub proxied_classes: Vec<String>,
}

pub(crate) fn load_compiled_container(
    workspace_root: &Path,
    config: &SymfonyContainerConfig,
) -> Option<CompiledContainerMetadata> {
    if !config.enabled() {
        return None;
    }

    let mut candidates = discover_container_files(workspace_root, config);
    candidates.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    candidates.reverse();

    let mut newest = None;
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut metadata = scan_compiled_container(&content);
        metadata.path = path;
        if !metadata.subscriptions.is_empty() || !metadata.proxied_classes.is_empty() {
            return Some(metadata);
        }
        if newest.is_none() {
            newest = Some(metadata);
        }
    }
    newest
}

pub(crate) fn path_may_be_compiled_container(
    workspace_root: &Path,
    path: &Path,
    config: &SymfonyContainerConfig,
) -> bool {
    if !config.enabled() || !path.extension().is_some_and(|ext| ext == "php") {
        return false;
    }
    let Ok(relative) = path.strip_prefix(workspace_root) else {
        return false;
    };

    if config.paths.is_empty() {
        let cache_root = Path::new("var").join("cache").join(config.environment());
        return relative.starts_with(cache_root)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("Container.php"));
    }

    config
        .paths
        .iter()
        .any(|spec| relative_matches_spec(relative, spec))
}

fn discover_container_files(
    workspace_root: &Path,
    config: &SymfonyContainerConfig,
) -> Vec<PathBuf> {
    if config.paths.is_empty() {
        let root = workspace_root
            .join("var")
            .join("cache")
            .join(config.environment());
        return walk_container_files(&root, 3, |_| true);
    }

    let mut files = BTreeSet::new();
    for spec in &config.paths {
        let Some(relative) = safe_relative_path(spec) else {
            tracing::warn!("PHPantom: ignored unsafe Symfony container path: {}", spec);
            continue;
        };
        if has_glob_meta(spec) {
            let Ok(glob) = Glob::new(spec) else {
                tracing::warn!("PHPantom: invalid Symfony container glob: {}", spec);
                continue;
            };
            let matcher = glob.compile_matcher();
            let base = workspace_root.join(fixed_glob_prefix(&relative));
            for path in walk_container_files(&base, 8, |path| {
                path.strip_prefix(workspace_root)
                    .is_ok_and(|relative| matcher.is_match(relative))
            }) {
                files.insert(path);
            }
            continue;
        }

        let absolute = workspace_root.join(relative);
        if absolute.is_file() {
            if is_container_php(&absolute) {
                files.insert(absolute);
            }
        } else if absolute.is_dir() {
            files.extend(walk_container_files(&absolute, 8, |_| true));
        }
    }
    files.into_iter().collect()
}

fn walk_container_files(
    root: &Path,
    max_depth: usize,
    matches: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }

    WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .max_depth(Some(max_depth))
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| is_container_php(path) && matches(path))
        .collect()
}

fn is_container_php(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("Container.php"))
}

fn relative_matches_spec(path: &Path, spec: &str) -> bool {
    let Some(relative) = safe_relative_path(spec) else {
        return false;
    };
    if has_glob_meta(spec) {
        return Glob::new(spec).is_ok_and(|glob| glob.compile_matcher().is_match(path));
    }
    path == relative || path.starts_with(relative)
}

fn safe_relative_path(spec: &str) -> Option<PathBuf> {
    let path = Path::new(spec);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(path.to_path_buf())
}

fn has_glob_meta(spec: &str) -> bool {
    spec.bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b'{'))
}

fn fixed_glob_prefix(path: &Path) -> PathBuf {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        let text = component.as_os_str().to_string_lossy();
        if has_glob_meta(&text) {
            break;
        }
        prefix.push(component.as_os_str());
    }
    prefix
}

pub(crate) fn scan_compiled_container(content: &str) -> CompiledContainerMetadata {
    let mut subscriptions = Vec::new();
    let mut proxied_classes = Vec::new();

    scan_method_calls(content, "addListener", |arguments| {
        let args = split_top_level(arguments, 0, arguments.len());
        let Some(event_arg) = args.first().and_then(|range| trimmed(arguments, *range)) else {
            return;
        };
        let Some(callback_arg) = args.get(1).and_then(|range| trimmed(arguments, *range)) else {
            return;
        };
        let Some(event) = decode_string(event_arg) else {
            return;
        };
        let Some((listener_fqn, method)) = listener_callback(callback_arg) else {
            return;
        };
        subscriptions.push(EventSubscription {
            event,
            listener_fqn,
            method,
        });
    });

    scan_method_calls(content, "createProxy", |arguments| {
        let bytes = arguments.as_bytes();
        let mut search = 0usize;
        while let Some(relative) = arguments[search..].find("new") {
            let keyword = search + relative;
            search = keyword + 3;
            if bytes
                .get(keyword.wrapping_sub(1))
                .is_some_and(|byte| is_php_identifier(*byte))
                || bytes
                    .get(keyword + 3)
                    .is_some_and(|byte| is_php_identifier(*byte))
            {
                continue;
            }
            let mut cursor = keyword + 3;
            skip_whitespace(bytes, &mut cursor);
            let start = cursor;
            while bytes.get(cursor).is_some_and(|byte| is_php_name(*byte)) {
                cursor += 1;
            }
            let fqn = normalize_fqn(&arguments[start..cursor]);
            if fqn.contains('\\') {
                proxied_classes.push(fqn);
            }
            break;
        }
    });

    subscriptions.sort_by(|left, right| {
        left.event
            .cmp(&right.event)
            .then(left.listener_fqn.cmp(&right.listener_fqn))
            .then(left.method.cmp(&right.method))
    });
    subscriptions.dedup();
    proxied_classes.sort_by_key(|name| name.to_ascii_lowercase());
    proxied_classes.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    CompiledContainerMetadata {
        path: PathBuf::new(),
        subscriptions,
        proxied_classes,
    }
}

fn scan_method_calls(content: &str, method: &str, mut visit: impl FnMut(&str)) {
    let needle = format!("->{method}");
    let bytes = content.as_bytes();
    let mut search = 0usize;
    while let Some(relative) = content[search..].find(&needle) {
        let found = search + relative;
        search = found + needle.len();
        if bytes
            .get(search)
            .is_some_and(|byte| is_php_identifier(*byte))
        {
            continue;
        }
        let mut open = search;
        skip_whitespace(bytes, &mut open);
        if bytes.get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_forward(content, open, b'(', b')') else {
            continue;
        };
        visit(&content[open + 1..close]);
        search = close + 1;
    }
}

fn listener_callback(callback: &str) -> Option<(String, String)> {
    let trimmed_callback = callback.trim();
    let inner = trimmed_callback.strip_prefix('[')?.strip_suffix(']')?;
    let parts = split_top_level(inner, 0, inner.len());
    let service = parts.first().and_then(|range| trimmed(inner, *range))?;
    let method = parts
        .get(1)
        .and_then(|range| trimmed(inner, *range))
        .and_then(decode_string)?;

    let listener_fqn = closure_target(service)
        .or_else(|| longest_fqn_string(service))
        .or_else(|| constructed_class(service))?;
    Some((listener_fqn, method))
}

fn closure_target(service: &str) -> Option<String> {
    let marker = "Closure";
    let marker_start = service.find(marker)?;
    let mut open = marker_start + marker.len();
    skip_whitespace(service.as_bytes(), &mut open);
    let close = find_matching_forward(service, open, b'(', b')')?;
    let arguments = &service[open + 1..close];
    let mut name = None;
    for range in split_top_level(arguments, 0, arguments.len()) {
        let Some(argument) = trimmed(arguments, range) else {
            continue;
        };
        let Some((key, raw_value)) = argument.split_once(':') else {
            continue;
        };
        let Some(value) = decode_string(raw_value) else {
            continue;
        };
        if !value.contains('\\') {
            continue;
        }
        if key.trim() == "class" {
            return Some(normalize_fqn(&value));
        }
        if key.trim() == "name" {
            name = Some(normalize_fqn(&value));
        }
    }
    name
}

fn longest_fqn_string(service: &str) -> Option<String> {
    let mut best = None;
    let mut search = 0usize;
    while let Some((value, consumed)) = decode_first_string(&service[search..]) {
        if value.contains('\\')
            && best
                .as_ref()
                .is_none_or(|candidate: &String| value.len() > candidate.len())
        {
            best = Some(normalize_fqn(&value));
        }
        search += consumed;
    }
    best
}

fn constructed_class(service: &str) -> Option<String> {
    let start = service.find("new")? + 3;
    let bytes = service.as_bytes();
    let mut cursor = start;
    skip_whitespace(bytes, &mut cursor);
    let name_start = cursor;
    while bytes.get(cursor).is_some_and(|byte| is_php_name(*byte)) {
        cursor += 1;
    }
    let fqn = normalize_fqn(&service[name_start..cursor]);
    fqn.contains('\\').then_some(fqn)
}

fn decode_first_string(text: &str) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let quote = bytes.iter().position(|byte| matches!(byte, b'\'' | b'"'))?;
    let end = crate::text_scan::skip_string_forward(bytes, quote);
    let raw = text.get(quote..end)?;
    let value = decode_php_string_literal(raw)?.into_owned();
    Some((value, end))
}

fn decode_string(text: &str) -> Option<String> {
    decode_php_string_literal(text.trim())
        .map(|value| value.into_owned())
        .filter(|value| !value.is_empty())
}

fn split_top_level(content: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut segment_start = start;
    let mut cursor = start;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;
    let mut brace_depth = 0u32;
    while cursor < end {
        match bytes[cursor] {
            b'\'' | b'"' => {
                cursor = crate::text_scan::skip_string_forward(bytes, cursor).min(end);
                continue;
            }
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                ranges.push((segment_start, cursor));
                segment_start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    ranges.push((segment_start, end));
    ranges
}

fn trimmed(content: &str, range: (usize, usize)) -> Option<&str> {
    let value = content.get(range.0..range.1)?.trim();
    (!value.is_empty()).then_some(value)
}

fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

fn is_php_identifier(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric() || byte >= 0x80
}

fn is_php_name(byte: u8) -> bool {
    is_php_identifier(byte) || byte == b'\\'
}

fn normalize_fqn(name: &str) -> String {
    name.trim().trim_start_matches('\\').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_listeners_and_proxy_factory_calls_without_executing_php() {
        let content = r#"<?php
$instance->addListener('post.create_course', [#[\Closure(name: 'App\\Listener\\CourseListener')] fn () => ($container->privates['App\\Listener\\CourseListener'] ?? self::getCourseListenerService($container)), 'onCreated'], 10);
$instance->addListener(
    "pre.update_course",
    [new \App\Listener\AuditListener(), '__invoke'],
);
$proxy = $factory->createProxy(new \App\UseCase\CreateCourse($dependency));
"#;

        let metadata = scan_compiled_container(content);
        assert_eq!(
            metadata.subscriptions,
            vec![
                EventSubscription {
                    event: "post.create_course".to_string(),
                    listener_fqn: "App\\Listener\\CourseListener".to_string(),
                    method: "onCreated".to_string(),
                },
                EventSubscription {
                    event: "pre.update_course".to_string(),
                    listener_fqn: "App\\Listener\\AuditListener".to_string(),
                    method: "__invoke".to_string(),
                },
            ]
        );
        assert_eq!(
            metadata.proxied_classes,
            vec!["App\\UseCase\\CreateCourse".to_string()]
        );
    }

    #[test]
    fn automatic_discovery_prefers_the_newest_useful_container() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("var/cache/dev");
        std::fs::create_dir_all(cache.join("ContainerOld")).unwrap();
        std::fs::create_dir_all(cache.join("ContainerNew")).unwrap();
        std::fs::write(
            cache.join("KernelDevDebugContainer.php"),
            "<?php class Wrapper {}",
        )
        .unwrap();
        let useful = cache.join("ContainerNew/KernelDevDebugContainer.php");
        std::fs::write(&useful, "<?php $x->createProxy(new \\App\\UseCase\\Run());").unwrap();

        let metadata = load_compiled_container(dir.path(), &SymfonyContainerConfig::default())
            .expect("compiled container should be discovered");
        assert_eq!(metadata.path, useful);
        assert_eq!(metadata.proxied_classes, vec!["App\\UseCase\\Run"]);
    }

    #[test]
    fn closure_class_wins_when_the_service_name_is_not_a_class() {
        let callback = "[#[\\Closure(name: 'app.listener', class: 'App\\\\Listener\\\\AuditListener')] fn () => null, 'audit']";
        assert_eq!(
            listener_callback(callback),
            Some((
                "App\\Listener\\AuditListener".to_string(),
                "audit".to_string()
            ))
        );
    }
}
