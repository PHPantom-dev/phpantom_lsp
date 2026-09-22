use super::cache::*;
use super::*;
use tower_lsp::lsp_types::{CodeLens, Position, Range, Url};

const URI: &str = "file:///test.php";

fn parse_extra(backend: &Backend, uri: &str, content: &str) {
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    backend.update_ast(uri, content);
    backend
        .workspace_indexed
        .store(true, std::sync::atomic::Ordering::Release);
    // The cache exists for the warm lens a refresh-capable client is
    // shown once the background search lands, which is the path these
    // tests measure.
    backend
        .supports_code_lens_refresh
        .store(true, std::sync::atomic::Ordering::Release);
}

fn parse(backend: &Backend, content: &str) {
    parse_extra(backend, URI, content);
}

fn lenses_for(backend: &Backend, uri: &str, content: &str) -> Vec<CodeLens> {
    backend.handle_code_lens(uri, content).unwrap_or_default()
}

fn lenses(backend: &Backend, content: &str) -> Vec<CodeLens> {
    lenses_for(backend, URI, content)
}

/// The title of the lens on `line`, absent while the declaration's
/// references are still being computed.
fn count_on_line(lenses: &[CodeLens], line: u32) -> Option<String> {
    lenses
        .iter()
        .find(|lens| lens.range.start.line == line)
        .and_then(|lens| lens.command.as_ref())
        .map(|command| command.title.clone())
}

#[test]
fn exact_location_cache_is_bounded_and_interns_uris() {
    let cache = MemberRefCounts::default();
    let location = Location {
        uri: Url::parse("file:///uses.php").unwrap(),
        range: Range::new(Position::new(1, 2), Position::new(1, 6)),
    };

    for index in 0..=MAX_CACHED_LOCATIONS / MAX_LOCATIONS_PER_MEMBER {
        cache.store(
            crate::atom::atom("Order"),
            crate::atom::atom(&format!("member{index}")),
            false,
            vec![location.clone(); MAX_LOCATIONS_PER_MEMBER],
            false,
        );
    }

    let state = cache.counts.read();
    assert!(state.location_count <= MAX_CACHED_LOCATIONS);
    assert_eq!(state.location_count, MAX_LOCATIONS_PER_MEMBER);
    assert_eq!(state.by_member.len(), 1);
    assert_eq!(state.uris.len(), 1);
}

const ONE_CALL: &str = r#"<?php
class Order {
    public function save(): void {}
}
function persist(Order $order): void {
    $order->save();
}
"#;

#[test]
fn batch_member_counts_reuse_forward_walked_scope_snapshots() {
    const ORDER_URI: &str = "file:///Order.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
    const CONSUMER: &str = r#"<?php
function persist(Order $order): void {
    $order->save();
    $order->save();
    $order->save();
}
"#;

    let backend = Backend::new_test();
    parse_extra(&backend, ORDER_URI, ORDER);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);
    lenses_for(&backend, ORDER_URI, ORDER);

    crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
    backend.compute_pending_member_ref_counts();

    let declaration_offset = ORDER.find("save").unwrap() as u32;
    assert_eq!(
        backend
            .member_ref_locations_cached(
                ORDER_URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .unwrap()
            .len(),
        3
    );
    assert!(
        crate::type_engine::variable::resolution::test_scope_cache_hits() >= 3,
        "each repeated receiver lookup should reuse the one forward-walked file scope"
    );
}

#[test]
fn later_member_batches_reuse_the_semantic_file_index() {
    const SERVICE_URI: &str = "file:///Service.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const SERVICE: &str = r#"<?php
class Service {
    public function save(): void {}
    public function cancel(): void {}
}
"#;
    const CONSUMER: &str = r#"<?php
function run(Service $service): void {
    $service->save();
    $service->cancel();
}
"#;

    let backend = Backend::new_test();
    parse_extra(&backend, SERVICE_URI, SERVICE);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);
    backend.workspace_indexed.store(true, Ordering::Release);

    let class_fqn = crate::atom::atom("Service");
    let save_offset = SERVICE.find("save").unwrap() as u32;
    assert!(
        backend
            .member_ref_locations_cached(
                SERVICE_URI,
                save_offset,
                class_fqn,
                crate::atom::atom("save"),
                false,
            )
            .is_none()
    );
    crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
    backend.compute_pending_member_ref_counts();
    assert!(crate::type_engine::variable::resolution::test_scope_cache_hits() > 0);

    let consumer_map = backend
        .symbol_maps
        .read()
        .get(CONSUMER_URI)
        .cloned()
        .unwrap();
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_some(),
        "the first member query should index every receiver in its candidate file"
    );

    let cancel_offset = SERVICE.find("cancel").unwrap() as u32;
    assert!(
        backend
            .member_ref_locations_cached(
                SERVICE_URI,
                cancel_offset,
                class_fqn,
                crate::atom::atom("cancel"),
                false,
            )
            .is_none()
    );
    crate::type_engine::variable::resolution::reset_test_scope_cache_hits();
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        crate::type_engine::variable::resolution::test_scope_cache_hits(),
        0,
        "a later member name must not rebuild or query the file's variable scopes"
    );
}

#[test]
fn ready_only_location_lookup_does_not_queue_background_work() {
    let backend = Backend::new_test();
    parse(&backend, ONE_CALL);
    let declaration_offset = ONE_CALL.find("save").unwrap() as u32;

    assert!(
        backend
            .member_ref_locations_ready(
                URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .is_none()
    );
    assert!(!backend.member_ref_counts.has_pending());
}

#[test]
fn an_edit_that_adds_an_access_recomputes_the_count() {
    let backend = Backend::new_test();
    parse(&backend, ONE_CALL);
    lenses(&backend, ONE_CALL);
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        count_on_line(&lenses(&backend, ONE_CALL), 2).as_deref(),
        Some("1 reference")
    );
    let declaration_offset = ONE_CALL.find("save").unwrap() as u32;
    assert_eq!(
        backend
            .member_ref_locations_cached(
                URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .expect("exact reference locations should be cached")
            .len(),
        1
    );

    let edited = ONE_CALL.replace("$order->save();", "$order->save();\n    $order->save();");
    parse(&backend, &edited);

    assert!(
        backend
            .member_ref_locations_cached(
                URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .is_none(),
        "stale locations must not be served to a clickable lens"
    );

    // A lens the user can click has to list what it counted, so the
    // pre-edit references are not shown again.  It keeps its line with a
    // placeholder rather than disappearing and shifting the file.
    assert_eq!(
        count_on_line(&lenses(&backend, &edited), 2).as_deref(),
        Some("- references")
    );
    assert!(backend.compute_pending_member_ref_counts());
    assert_eq!(
        count_on_line(&lenses(&backend, &edited), 2).as_deref(),
        Some("2 references")
    );
    assert_eq!(
        backend
            .member_ref_locations_cached(
                URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .expect("edited exact locations should replace the stale cache")
            .len(),
        2
    );
}

#[test]
fn changing_only_a_receiver_type_invalidates_cached_locations() {
    const ORDER_URI: &str = "file:///Order.php";
    const BUYER_URI: &str = "file:///Buyer.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    let backend = Backend::new_test();
    let order = "<?php\nclass Order { public function save(): void {} }\n";
    let buyer = "<?php\nclass Buyer { public function save(): void {} }\n";
    let consumer = "<?php\nfunction persist(Order $value): void { $value->save(); }\n";
    parse_extra(&backend, ORDER_URI, order);
    parse_extra(&backend, BUYER_URI, buyer);
    parse_extra(&backend, CONSUMER_URI, consumer);

    let declaration_offset = order.find("save").unwrap() as u32;
    assert!(
        backend
            .member_ref_locations_cached(
                ORDER_URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .is_none()
    );
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        backend
            .member_ref_locations_cached(
                ORDER_URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .unwrap()
            .len(),
        1
    );

    let edited = consumer.replace("Order $value", "Buyer $value");
    parse_extra(&backend, CONSUMER_URI, &edited);
    assert!(
        backend
            .member_ref_locations_cached(
                ORDER_URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .is_none(),
        "a type-only edit must not leave a clickable lens pointing at stale locations"
    );
    backend.compute_pending_member_ref_counts();
    assert!(
        backend
            .member_ref_locations_cached(
                ORDER_URI,
                declaration_offset,
                crate::atom::atom("Order"),
                crate::atom::atom("save"),
                false,
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_edit_that_leaves_the_accesses_alone_keeps_the_count() {
    let backend = Backend::new_test();
    parse(&backend, ONE_CALL);
    lenses(&backend, ONE_CALL);
    backend.compute_pending_member_ref_counts();

    // The first edit is the one that records what the file's classes
    // inherit, so start measuring from the second.
    let edited = format!("{ONE_CALL}// a trailing comment\n");
    parse(&backend, &edited);
    lenses(&backend, &edited);
    backend.compute_pending_member_ref_counts();

    let edited_again = format!("{edited}// another trailing comment\n");
    parse(&backend, &edited_again);

    let cached = backend
        .member_ref_counts
        .get(crate::atom::atom("Order"), crate::atom::atom("save"), false)
        .expect("the computed count should survive the edit");
    assert_eq!(cached.count, 1);
    assert!(
        !cached.count_stale,
        "an edit that touches no access should not invalidate the count"
    );
    // Exact locations are another matter: any edit can move them, so
    // the clickable lens recomputes before it is shown again.
    assert!(!cached.locations_stale.is_fresh());
}

#[test]
fn an_edit_in_an_unrelated_file_leaves_a_cached_declaration_alone() {
    const ORDER_URI: &str = "file:///Order.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const UNRELATED_URI: &str = "file:///helpers.php";
    const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
    const CONSUMER: &str =
        "<?php\nfunction persist(Order $order): void {\n    $order->save();\n}\n";

    let backend = Backend::new_test();
    parse_extra(&backend, ORDER_URI, ORDER);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);
    parse_extra(&backend, UNRELATED_URI, "<?php\nfunction noop(): void {}\n");
    lenses_for(&backend, ORDER_URI, ORDER);
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
        Some("1 reference")
    );

    // A file that holds none of the cached locations was reparsed.
    parse_extra(
        &backend,
        UNRELATED_URI,
        "<?php\nfunction noop(): void {}\n// touched\n",
    );

    assert_eq!(
        count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
        Some("1 reference"),
        "an edit that cannot have moved a cached location must not blank the lens"
    );
    assert!(
        !backend.member_ref_counts.has_pending(),
        "nor queue the declaration for another workspace search"
    );
}

#[test]
fn an_edit_rescans_the_file_it_touched_and_keeps_the_rest() {
    const ORDER_URI: &str = "file:///Order.php";
    const FIRST_URI: &str = "file:///First.php";
    const SECOND_URI: &str = "file:///Second.php";
    const ORDER: &str = "<?php\nclass Order {\n    public function save(): void {}\n}\n";
    const FIRST: &str = "<?php\nfunction first(Order $order): void {\n    $order->save();\n    $order->save();\n}\n";
    let second = |calls: usize| {
        let body = "    $order->save();\n".repeat(calls);
        format!("<?php\nfunction second(Order $order): void {{\n{body}}}\n")
    };

    let backend = Backend::new_test();
    parse_extra(&backend, ORDER_URI, ORDER);
    parse_extra(&backend, FIRST_URI, FIRST);
    parse_extra(&backend, SECOND_URI, &second(1));
    lenses_for(&backend, ORDER_URI, ORDER);
    backend.compute_pending_member_ref_counts();
    assert_eq!(
        count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
        Some("3 references")
    );

    // Only the second file changes.  Its accesses are counted again and
    // the first file's cached ones are carried over untouched.
    parse_extra(&backend, SECOND_URI, &second(3));
    assert_eq!(
        count_on_line(&lenses_for(&backend, ORDER_URI, ORDER), 2).as_deref(),
        Some("- references"),
        "the lens holds its line while the touched file is rescanned"
    );
    assert!(backend.compute_pending_member_ref_counts());

    let locations = backend
        .member_ref_locations_cached(
            ORDER_URI,
            ORDER.find("save").unwrap() as u32,
            crate::atom::atom("Order"),
            crate::atom::atom("save"),
            false,
        )
        .expect("the rescan should leave a complete result");
    assert_eq!(locations.len(), 5);
    assert_eq!(
        locations
            .iter()
            .filter(|location| location.uri.as_str() == FIRST_URI)
            .count(),
        2,
        "the untouched file's references must survive the rescan"
    );
}

#[test]
fn a_result_computed_before_an_edit_is_not_marked_fresh() {
    let backend = Backend::new_test();
    parse(&backend, ONE_CALL);
    lenses(&backend, ONE_CALL);
    backend.compute_pending_member_ref_counts();

    let class_fqn = crate::atom::atom("Order");
    let member = crate::atom::atom("save");
    let declaration_offset = ONE_CALL.find("save").unwrap() as u32;

    // What a search finishing after an edit landed looks like: it carries
    // locations read from content the editor has already replaced.
    backend.queue_member_references(URI, declaration_offset, class_fqn, member, false);
    backend.member_ref_counts.invalidate_locations_all();
    backend
        .member_ref_counts
        .store(class_fqn, member, false, Vec::new(), true);

    assert!(
        !backend.member_ref_counts.is_fresh(&PendingCount {
            uri: Arc::from(URI),
            offset: declaration_offset,
            class_fqn,
            member,
            is_static: false,
        }),
        "a result read from replaced content must stay stale"
    );
    assert!(
        backend
            .member_ref_locations_cached(URI, declaration_offset, class_fqn, member, false)
            .is_none(),
        "and must not be served to a clickable lens"
    );
    assert!(
        backend.member_ref_counts.has_pending(),
        "the recomputation the edit asked for must survive"
    );
}
