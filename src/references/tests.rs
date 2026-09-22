use crate::Backend;
use crate::virtual_members::laravel::extract_macro_registrations;
use std::sync::atomic::Ordering;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend and return the URI.
async fn open_file(backend: &Backend, uri: &Url, text: &str) {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;
}

/// Helper: send a find-references request and return the locations.
async fn find_references(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> Vec<Location> {
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration,
        },
    };

    backend
        .references(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

fn seed_macro_index(backend: &Backend, uri: &Url, text: &str) {
    let mut index = backend.laravel_macros.write();
    index.files.set_file(
        uri.to_string(),
        extract_macro_registrations(text, Some(*backend.workspace.php_version.lock())),
    );
    index.rebuild();
    backend
        .laravel_has_macros
        .store(!index.is_empty(), Ordering::Relaxed);
}

fn line_char_of(haystack: &str, needle: &str) -> (u32, u32) {
    for (line_idx, line) in haystack.lines().enumerate() {
        if let Some(char_idx) = line.find(needle) {
            return (line_idx as u32, char_idx as u32);
        }
    }
    panic!("needle not found: {needle}");
}

// ─── Laravel macro registrations ────────────────────────────────────────────

#[tokio::test]
async fn test_macro_registration_string_references_include_call_sites() {
    let backend = Backend::new_test();
    let class_uri = Url::parse("file:///Widget.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let class_text = concat!("<?php\n", "namespace App\\Support;\n", "class Widget {}\n",);
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "Widget::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "function demo(Widget $widget): void {\n",
        "    Widget::shine();\n",
        "    $widget->shine();\n",
        "}\n",
    );

    open_file(&backend, &class_uri, class_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let (line, character) = line_char_of(provider_text, "shine");
    let locs = find_references(&backend, &provider_uri, line, character, true).await;
    assert_eq!(
        locs.len(),
        3,
        "expected declaration + static + instance call: {locs:?}"
    );
    assert!(locs.iter().any(|loc| loc.uri == provider_uri));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 4 && loc.range.start.character == 12
    }));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 5 && loc.range.start.character == 13
    }));
}

#[tokio::test]
async fn test_macro_registration_string_references_use_reference_index_candidates() {
    let backend = Backend::new_test();
    let class_uri = Url::parse("file:///Widget.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();
    let unrelated_uri = Url::parse("file:///Unrelated.php").unwrap();

    let class_text = concat!("<?php\n", "namespace App\\Support;\n", "class Widget {}\n",);
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "Widget::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "function demo(Widget $widget): void {\n",
        "    Widget::shine();\n",
        "    $widget->shine();\n",
        "}\n",
    );
    let unrelated_text = concat!("<?php\n", "namespace App;\n", "class Unrelated {}\n",);

    open_file(&backend, &class_uri, class_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    open_file(&backend, &unrelated_uri, unrelated_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    // With the workspace marked indexed, the macro search prunes the
    // snapshot to reference-index candidates instead of scanning every
    // file; both the static and the instance call must survive pruning.
    backend.workspace_indexed.store(true, Ordering::Release);

    let (line, character) = line_char_of(provider_text, "shine");
    let locs = find_references(&backend, &provider_uri, line, character, true).await;
    assert_eq!(
        locs.len(),
        3,
        "expected declaration + static + instance call via candidate pruning: {locs:?}"
    );
    assert!(locs.iter().any(|loc| loc.uri == provider_uri));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 4 && loc.range.start.character == 12
    }));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 5 && loc.range.start.character == 13
    }));
}

#[tokio::test]
async fn test_macro_references_from_descendant_call_include_ancestor_and_sibling_calls() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let child_uri = Url::parse("file:///EloquentCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let child_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class EloquentCollection extends BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "use App\\Support\\EloquentCollection;\n",
        "function demo(BaseCollection $base, EloquentCollection $eloquent): void {\n",
        "    $base->shine();\n",
        "    $eloquent->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &child_uri, child_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let locs = find_references(&backend, &caller_uri, 6, 16, true).await;
    assert_eq!(
        locs.len(),
        3,
        "expected registration + base + descendant call: {locs:?}"
    );
    assert!(locs.iter().any(|loc| loc.uri == provider_uri));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 5 && loc.range.start.character == 11
    }));
    assert!(locs.iter().any(|loc| {
        loc.uri == caller_uri && loc.range.start.line == 6 && loc.range.start.character == 15
    }));
}

#[tokio::test]
async fn test_macro_registration_references_include_unresolved_chain_call() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "function demo($query): void {\n",
        "    $query->pluck('name', 'id')->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let shine_subject = {
        let maps = backend.symbol_maps.read();
        let map = maps
            .get(caller_uri.as_str())
            .expect("caller symbol map should exist");
        let source = map
            .source(caller_text)
            .expect("caller symbol map should describe the caller text");
        map.spans
            .iter()
            .find_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::MemberAccess {
                    member_name,
                    subject_text,
                    ..
                } if member_name == "shine" => Some(subject_text.as_str(source).to_string()),
                _ => None,
            })
            .expect("expected member-access span for unresolved chain call")
    };
    assert!(
        shine_subject.contains("pluck"),
        "expected chain subject text, got {shine_subject:?}"
    );
    assert!(
        shine_subject.contains('('),
        "expected call-chain subject text, got {shine_subject:?}"
    );

    let (line, character) = line_char_of(provider_text, "shine");
    let locs = find_references(&backend, &provider_uri, line, character, true).await;
    assert!(
        locs.iter().any(|loc| {
            loc.uri == caller_uri && loc.range.start.line == 3 && loc.range.start.character == 33
        }),
        "expected unresolved chain macro call to be included: {locs:?}"
    );
}

#[test]
fn workspace_indexing_batch_merges_disk_files() {
    use crate::reference_index::ReferenceIndexKey;

    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("Contracts")).expect("contracts dir");
    std::fs::create_dir_all(src.join("Impl")).expect("impl dir");

    std::fs::write(
        src.join("Contracts/Service.php"),
        "<?php\nnamespace App\\Contracts;\ninterface Service {}\n",
    )
    .expect("service file");
    std::fs::write(
        src.join("Impl/A.php"),
        "<?php\nnamespace App\\Impl;\nuse App\\Contracts\\Service;\nclass A implements Service { public function run(): void {} }\n",
    )
    .expect("a file");
    std::fs::write(
        src.join("Impl/B.php"),
        "<?php\nnamespace App\\Impl;\nclass B extends A {}\n",
    )
    .expect("b file");
    std::fs::write(
        src.join("Use.php"),
        "<?php\nnamespace App;\nuse App\\Impl\\A;\nfunction helper(): void {}\ndefine('APP_FLAG', 'yes');\n$a = new A();\n$a->run();\nhelper();\n",
    )
    .expect("use file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.ensure_workspace_indexed();

    assert!(
        backend
            .workspace_indexed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(
        backend.symbol_maps.read().len(),
        4,
        "all disk files should publish symbol maps through the batch merge"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\Contracts\\Service")
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\Impl\\A")
    );
    assert!(
        backend
            .symbols
            .global_functions
            .read()
            .contains_key("App\\helper")
    );
    assert!(
        backend
            .symbols
            .global_defines
            .read()
            .contains_key("APP_FLAG")
    );

    let service_children = backend
        .symbols
        .gti_index
        .read()
        .get("App\\Contracts\\Service")
        .cloned()
        .unwrap_or_default();
    assert!(service_children.contains(&"App\\Impl\\A".to_string()));

    let use_uri = crate::util::path_to_uri(&src.join("Use.php"));
    let class_candidates = backend
        .reference_candidate_uris_for_keys(&[ReferenceIndexKey::class("App\\Impl\\A")])
        .expect("reference index should be active after workspace indexing");
    assert!(class_candidates.contains(use_uri.as_str()));

    let member_candidates = backend
        .reference_candidate_uris_for_keys(&[ReferenceIndexKey::Member {
            name: "run".to_string(),
            is_static: false,
        }])
        .expect("reference index should be active after workspace indexing");
    assert!(member_candidates.contains(use_uri.as_str()));

    let function_snapshot = backend
        .user_file_symbol_maps_for_reference_keys(&[ReferenceIndexKey::function("App\\helper")]);
    assert_eq!(
        function_snapshot.len(),
        1,
        "reference-key snapshots should use the reference index instead of cloning every user file"
    );
    assert_eq!(function_snapshot[0].0, use_uri);
}

#[test]
fn indexing_work_order_processes_largest_files_first() {
    assert_eq!(
        crate::indexing::preload::largest_first_work_order(&[10, 1, 50, 3]),
        vec![2, 0, 3, 1]
    );
}

#[test]
fn reference_key_snapshot_falls_back_until_workspace_index_ready() {
    use crate::reference_index::ReferenceIndexKey;

    let backend = Backend::new_test();
    let matching_uri = "file:///project/src/Use.php";
    let unrelated_uri = "file:///project/src/Other.php";

    backend.update_ast(
        matching_uri,
        "<?php\nnamespace App;\nfunction helper(): void {}\nhelper();\n",
    );
    backend.update_ast(unrelated_uri, "<?php\nnamespace App;\nclass Other {}\n");

    let snapshot = backend
        .user_file_symbol_maps_for_reference_keys(&[ReferenceIndexKey::function("App\\helper")]);
    let uris: std::collections::HashSet<_> = snapshot.into_iter().map(|(uri, _)| uri).collect();

    assert!(
        !backend
            .workspace_indexed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(uris.contains(matching_uri));
    assert!(
        uris.contains(unrelated_uri),
        "before the full-index flag is ready, reference scans must fall back to all user files"
    );
}

#[test]
fn user_file_symbol_maps_exclude_vendor_and_stubs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(&vendor).expect("vendor dir");

    let backend = Backend::new_test();
    backend.add_vendor_dir(&vendor);

    let user_uri = "file:///project/src/User.php";
    let vendor_uri = crate::util::path_to_uri(&vendor.join("Package.php"));
    backend.update_ast(user_uri, "<?php\nnamespace App;\nclass User {}\n");
    backend.update_ast(&vendor_uri, "<?php\nnamespace Vendor;\nclass Package {}\n");
    backend.update_ast("phpantom-stub://core.php", "<?php\nclass StubClass {}\n");
    backend.update_ast(
        "phpantom-stub-fn://core.php",
        "<?php\nfunction stub_fn(): void {}\n",
    );

    let snapshot = backend.user_file_symbol_maps();
    let uris: std::collections::HashSet<_> = snapshot.into_iter().map(|(uri, _)| uri).collect();

    assert!(uris.contains(user_uri));
    assert!(!uris.contains(&vendor_uri));
    assert!(!uris.contains("phpantom-stub://core.php"));
    assert!(!uris.contains("phpantom-stub-fn://core.php"));
}

#[test]
fn workspace_index_progress_covers_known_and_discovered_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("src dir");

    let known_path = src.join("Known.php");
    let disk_path = src.join("Disk.php");
    std::fs::write(&known_path, "<?php\nnamespace App;\nclass Known {}\n").expect("known file");
    std::fs::write(&disk_path, "<?php\nnamespace App;\nclass Disk {}\n").expect("disk file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.symbols.fqn_uri_index.write().insert(
        "App\\Known".to_string(),
        crate::util::path_to_uri(&known_path),
    );

    let progress = std::sync::Mutex::new(Vec::new());
    backend.ensure_workspace_indexed_with_progress(Some(&|percentage, message| {
        progress
            .lock()
            .expect("progress lock")
            .push((percentage, message));
    }));

    let messages: Vec<String> = progress
        .lock()
        .expect("progress lock")
        .iter()
        .map(|(_, message)| message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message == "Preparing workspace index")
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("Parsing indexed files"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("Parsing workspace files"))
    );
    assert_eq!(
        progress
            .lock()
            .expect("progress lock")
            .last()
            .map(|(pct, _)| *pct),
        Some(100)
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\Known")
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\Disk")
    );

    let refresh_path = src.join("Refresh.php");
    std::fs::write(&refresh_path, "<?php\nnamespace App;\nclass Refresh {}\n")
        .expect("refresh file");
    backend.ensure_workspace_indexed_with_progress(None);
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\Refresh")
    );
}

/// Once the first workspace pass is complete, each reference-count or
/// CodeLens query must reuse it. In particular, a concurrent caller must not
/// queue behind the workspace lock and begin another disk walk.
#[test]
fn completed_workspace_index_is_reused_without_waiting() {
    let backend = Backend::new_test();
    backend
        .workspace_indexed
        .store(true, std::sync::atomic::Ordering::Release);
    let indexing = backend.workspace_index_lock.lock();

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let waiter = {
        let backend = backend.clone_for_blocking();
        std::thread::spawn(move || {
            backend.ensure_workspace_index_ready_with_progress(None);
            done_tx.send(()).expect("report completion");
        })
    };

    done_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("a completed index should bypass the in-flight lock");
    drop(indexing);
    waiter.join().expect("waiter thread");
}

#[test]
fn request_progress_maps_indexing_into_lower_window() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        src.join("Target.php"),
        "<?php\nnamespace App;\nclass Target {}\n",
    )
    .expect("target file");

    let mut backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    let state = crate::progress::ScanProgress::new();
    backend.request_progress = Some(std::sync::Arc::clone(&state));

    backend.ensure_workspace_indexed_for_request();

    // The indexing pass reports 100% into the request sink, which maps
    // it to the top of the 0..80 indexing window.
    let (percentage, message) = state.take_report().expect("indexing progress forwarded");
    assert_eq!(percentage, 80);
    assert_eq!(message, "Workspace index ready");
}

/// A request that arrives while the background full index holds
/// `workspace_index_lock` still waits for a complete index, but it must
/// not look stalled: it mirrors the in-flight index's own status into
/// its progress sink until the lock frees up.
#[test]
fn blocked_request_reports_in_flight_index_status() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        src.join("Target.php"),
        "<?php\nnamespace App;\nclass Target {}\n",
    )
    .expect("target file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    *backend.workspace_index_status.lock() =
        Some((37, "Parsing workspace files (3/9)".to_string()));
    let indexing = backend.workspace_index_lock.lock();

    let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let waiter = {
        let backend = backend.clone_for_blocking();
        let reports = std::sync::Arc::clone(&reports);
        std::thread::spawn(move || {
            backend.ensure_workspace_index_ready_with_progress(Some(&|percentage, message| {
                reports
                    .lock()
                    .expect("reports lock")
                    .push((percentage, message));
            }));
        })
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while reports.lock().expect("reports lock").is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "blocked request never reported the wait"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (percentage, message) = reports.lock().expect("reports lock")[0].clone();
    assert_eq!(percentage, 37);
    assert_eq!(
        message,
        "Waiting for workspace index: Parsing workspace files (3/9)"
    );

    // Stand in for the lock owner publishing the completed index before it
    // releases the single-flight guard.
    backend
        .workspace_indexed
        .store(true, std::sync::atomic::Ordering::Release);
    *backend.workspace_index_status.lock() = None;
    drop(indexing);
    waiter.join().expect("waiter thread");

    assert!(
        backend.workspace_index_status.lock().is_none(),
        "a finished indexing pass clears the shared status"
    );
    let reports = reports.lock().expect("reports lock");
    assert!(
        !reports
            .iter()
            .any(|(_, message)| message == "Preparing workspace index"),
        "the waiting request must reuse the index published by the lock owner"
    );
    assert_eq!(
        reports.last(),
        Some(&(100, "Workspace index ready".to_string()))
    );
}

#[test]
fn request_progress_reports_per_file_reference_scan() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        src.join("Target.php"),
        "<?php\nnamespace App;\nclass Target {}\n",
    )
    .expect("target file");
    std::fs::write(
        src.join("Usage.php"),
        "<?php\nnamespace App;\n$t = new Target();\n",
    )
    .expect("usage file");

    let mut backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    let state = crate::progress::ScanProgress::new();
    backend.request_progress = Some(std::sync::Arc::clone(&state));

    let locations = backend.find_class_references("App\\Target", true);
    assert!(!locations.is_empty(), "reference scan found the usage");

    // After the scan, the sink holds the completed 80..100 scan window
    // with per-file counts.
    let (percentage, message) = state.take_report().expect("scan progress reported");
    assert_eq!(percentage, 100);
    assert!(
        message.starts_with("Scanning for class references ("),
        "unexpected message: {message}"
    );
}

#[test]
fn parse_files_parallel_with_progress_merges_large_batches() {
    let backend = Backend::new_test();
    let files = (0..3)
        .map(|idx| {
            (
                format!("file:///project/src/File{idx}.php"),
                Some(format!("<?php\nnamespace App;\nclass File{idx} {{}}\n")),
            )
        })
        .collect();
    let progress = std::sync::Mutex::new(Vec::new());

    backend.parse_files_parallel_with_progress(
        files,
        Some(&|done, total, done_units, total_units| {
            progress
                .lock()
                .expect("progress lock")
                .push((done, total, done_units, total_units));
        }),
    );

    for idx in 0..3 {
        assert!(
            backend
                .symbols
                .fqn_class_index
                .read()
                .contains_key(format!("App\\File{idx}").as_str())
        );
    }
    assert!(progress.lock().expect("progress lock").iter().any(
        |(done, total, done_units, total_units)| {
            *done == 3 && *total == 3 && *done_units == *total_units
        }
    ));
}

#[test]
fn parse_paths_parallel_with_progress_handles_small_batches_and_missing_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    let first = dir.path().join("First.php");
    let missing = dir.path().join("Missing.php");
    std::fs::write(&first, "<?php\nnamespace App;\nclass First {}\n").expect("first file");

    let backend = Backend::new_test();
    let work = vec![
        (crate::util::path_to_uri(&first), first),
        (crate::util::path_to_uri(&missing), missing),
    ];
    let progress = std::sync::Mutex::new(Vec::new());
    backend.parse_paths_parallel_with_progress(
        &work,
        Some(&|done, total, done_units, total_units| {
            progress
                .lock()
                .expect("progress lock")
                .push((done, total, done_units, total_units));
        }),
    );

    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .contains_key("App\\First")
    );
    assert!(progress.lock().expect("progress lock").iter().any(
        |(done, total, done_units, total_units)| {
            *done == 2 && *total == 2 && *done_units == *total_units
        }
    ));
}

#[test]
fn workspace_parse_percentage_handles_empty_and_weighted_totals() {
    assert_eq!(
        crate::indexing::preload::workspace_parse_percentage(0, 0),
        95
    );
    assert_eq!(
        crate::indexing::preload::workspace_parse_percentage(0, 200),
        5
    );
    assert_eq!(
        crate::indexing::preload::workspace_parse_percentage(100, 200),
        50
    );
    assert_eq!(
        crate::indexing::preload::workspace_parse_percentage(200, 200),
        95
    );
    assert_eq!(
        crate::indexing::preload::workspace_parse_percentage(500, 200),
        95
    );
}

#[test]
fn index_progress_weight_prefers_supplied_and_open_file_content() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Open.php";

    assert_eq!(backend.index_progress_weight_for_uri(uri, Some("")), 1);

    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new("abcdef".to_string()));
    assert_eq!(backend.index_progress_weight_for_uri(uri, None), 6);
}

// ─── Laravel string-key gating (non-Laravel projects) ──────────────────────

/// A non-Laravel project can define its own `config()` function (common in
/// home-grown micro-frameworks). `SymbolKind::LaravelStringKey` spans are
/// extracted by name match alone, so find-references must not treat the two
/// calls below as Laravel config-key references unless the project is
/// actually classified as Laravel.
#[tokio::test]
async fn laravel_string_key_references_gated_on_is_laravel() {
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                   // L0
        "function demo(): void {\n", // L1
        "    config('app.name');\n", // L2
        "    config('app.name');\n", // L3
        "}\n",                       // L4
    );
    let (line, character) = line_char_of(text, "'app.name'");

    let laravel_backend = Backend::new_test();
    laravel_backend
        .resolved_class_cache
        .write()
        .set_laravel(true);
    open_file(&laravel_backend, &uri, text).await;
    let laravel_locs = find_references(&laravel_backend, &uri, line, character + 2, true).await;
    assert!(
        laravel_locs.len() >= 2,
        "expected both config('app.name') calls to be found on a Laravel project, got {}",
        laravel_locs.len()
    );

    let plain_backend = Backend::new_test();
    plain_backend
        .resolved_class_cache
        .write()
        .set_laravel(false);
    open_file(&plain_backend, &uri, text).await;
    let plain_locs = find_references(&plain_backend, &uri, line, character + 2, true).await;
    assert!(
        plain_locs.is_empty(),
        "a non-Laravel project's own config() must not produce Laravel string-key \
         references, got {plain_locs:?}"
    );
}
