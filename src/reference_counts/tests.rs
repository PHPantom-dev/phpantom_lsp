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

/// A receiver walk is the type engine over a whole file, so a query pays for
/// the member names it asked about and no others.  A later name adds its own
/// accesses to the same entry rather than re-resolving the ones already there.
#[test]
fn later_member_batches_extend_the_semantic_file_index() {
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
    let save_span = consumer_map.member_access_indices("save")[0];
    let cancel_span = consumer_map.member_access_indices("cancel")[0];

    let indexed = backend
        .resolved_member_file(CONSUMER_URI, &consumer_map)
        .expect("the first member query should index its candidate file");
    assert!(
        indexed.covers([crate::atom::atom("save")]),
        "the query's own member name has to be covered"
    );
    assert!(
        !indexed.covers([crate::atom::atom("cancel")]),
        "a name nothing asked about must not be walked for"
    );
    assert!(
        !indexed.targets_for_span(save_span).is_empty(),
        "the receiver the query did ask about has to be resolved"
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
    backend.compute_pending_member_ref_counts();

    assert_eq!(
        backend
            .member_ref_locations_cached(
                SERVICE_URI,
                cancel_offset,
                class_fqn,
                crate::atom::atom("cancel"),
                false,
            )
            .unwrap()
            .len(),
        1,
        "the second name is still found in a file the first name already walked"
    );
    let extended = backend
        .resolved_member_file(CONSUMER_URI, &consumer_map)
        .expect("the entry survives the second query");
    assert!(
        extended.covers([crate::atom::atom("save"), crate::atom::atom("cancel")]),
        "the entry now answers for both names"
    );
    assert!(
        !extended.targets_for_span(save_span).is_empty()
            && !extended.targets_for_span(cancel_span).is_empty(),
        "the second query carries the first query's resolutions over rather \
         than discarding them"
    );
}

/// The semantic layer records where each access it resolved sits, so a search
/// whose names a candidate file is already indexed for never goes back to that
/// file's text: the file is neither opened nor read from disk again.
#[test]
fn a_warm_candidate_file_is_answered_without_reading_it() {
    const SERVICE_URI: &str = "file:///Service.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const SERVICE: &str = "<?php\nclass Service {\n    public function save(): void {}\n}\n";
    const CONSUMER: &str = r#"<?php
function run(Service $service): void {
    $service->save();
}
"#;

    let backend = Backend::new_test();
    parse_extra(&backend, SERVICE_URI, SERVICE);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);

    let save_offset = SERVICE.find("save").unwrap() as u32;
    let first = backend.member_declaration_references(SERVICE_URI, save_offset, "save", false);
    assert_eq!(first.len(), 1, "the access in the consumer is a reference");

    // Nothing but the warm entry can answer for the consumer now: it is not
    // open, and the URI has no file behind it.
    backend.open_files.write().remove(CONSUMER_URI);
    let second = backend.member_declaration_references(SERVICE_URI, save_offset, "save", false);
    assert_eq!(
        second, first,
        "a file already resolved for this member is answered from the layer"
    );
}

/// Building variable scopes is the type engine over a body, so a
/// candidate file holding one access among a dozen bodies pays for that
/// one body and no others.
#[test]
fn a_receiver_walk_builds_scopes_only_for_the_body_holding_the_access() {
    const SERVICE_URI: &str = "file:///Service.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const SERVICE: &str = "<?php\nclass Service {\n    public function save(): void {}\n}\n";
    const CONSUMER: &str = r#"<?php
class Consumer {
    public function first(Service $service): void {
        $other = $service;
    }
    public function second(Service $service): void {
        $other = $service;
        $other->save();
    }
    public function third(Service $service): void {
        $other = $service;
    }
    public function fourth(Service $service): void {
        $other = $service;
    }
}
"#;

    let backend = Backend::new_test();
    parse_extra(&backend, SERVICE_URI, SERVICE);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);
    lenses_for(&backend, SERVICE_URI, SERVICE);

    crate::type_engine::variable::forward_walk::reset_test_body_walks();
    backend.compute_pending_member_ref_counts();
    let walks = crate::type_engine::variable::forward_walk::test_body_walks();

    let save_offset = SERVICE.find("save").unwrap() as u32;
    assert_eq!(
        backend
            .member_ref_locations_cached(
                SERVICE_URI,
                save_offset,
                crate::atom::atom("Service"),
                crate::atom::atom("save"),
                false,
            )
            .unwrap()
            .len(),
        1,
        "the one call on `$other` has to be found"
    );
    assert_eq!(
        walks, 1,
        "only `second()` holds the access; the other three bodies must not \
         be walked for it"
    );
}

/// A file whose receivers a signature edit cannot have moved keeps its
/// resolutions.  Clearing the layer on every signature keystroke is what
/// makes the next search rebuild the receiver layer for every candidate file
/// in the workspace, and most of them never consulted the edited class.
mod layer_survives_unrelated_edits {
    use super::*;

    const SERVICE_URI: &str = "file:///Service.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const OTHER_URI: &str = "file:///Other.php";
    const SERVICE: &str = "<?php\nclass Service {\n    public function save(): void {}\n}\n";
    const CONSUMER: &str = r#"<?php
function run(Service $service): void {
    $service->save();
}
"#;
    const OTHER: &str = "<?php\nclass Other {\n    public function ping(): void {}\n}\n";

    /// Warm the layer for the consumer by searching for `Service::save`.
    fn warmed() -> Backend {
        let backend = Backend::new_test();
        parse_extra(&backend, SERVICE_URI, SERVICE);
        parse_extra(&backend, CONSUMER_URI, CONSUMER);
        parse_extra(&backend, OTHER_URI, OTHER);

        let save_offset = SERVICE.find("save").unwrap() as u32;
        let found = backend.member_declaration_references(SERVICE_URI, save_offset, "save", false);
        assert_eq!(found.len(), 1, "the access in the consumer is a reference");
        assert!(
            is_warm(&backend),
            "the search has to leave the file indexed"
        );
        backend
    }

    /// Whether the consumer's resolutions are still cached.
    fn is_warm(backend: &Backend) -> bool {
        let map = backend
            .symbol_maps
            .read()
            .get(CONSUMER_URI)
            .cloned()
            .expect("the consumer is parsed");
        backend.resolved_member_file(CONSUMER_URI, &map).is_some()
    }

    #[test]
    fn a_signature_change_in_a_class_the_file_never_consulted_keeps_it() {
        let backend = warmed();
        backend.update_ast(
            OTHER_URI,
            "<?php\nclass Other {\n    public function ping(): string {}\n}\n",
        );
        assert!(
            is_warm(&backend),
            "nothing the consumer resolved goes through `Other`"
        );
    }

    #[test]
    fn a_signature_change_in_a_class_the_file_did_consult_drops_it() {
        let backend = warmed();
        backend.update_ast(
            SERVICE_URI,
            "<?php\nclass Service {\n    public function save(): string {}\n}\n",
        );
        assert!(
            !is_warm(&backend),
            "the consumer's receiver resolved through `Service`"
        );
    }

    #[test]
    fn a_new_class_of_a_name_the_file_looked_for_drops_it() {
        let backend = warmed();
        // The consumer's `Service` parameter was looked up while this file
        // declared something else, so the declaration it would find now is a
        // different one.
        backend.update_ast(
            OTHER_URI,
            "<?php\nclass Other {}\nclass Service {\n    public function save(): void {}\n}\n",
        );
        assert!(
            !is_warm(&backend),
            "a name that gains a declaration changes what it resolves to"
        );
    }
}

/// The receiver a chain ends on is settled by classes the file never names,
/// so the layer is invalidated by what its resolution consulted rather than
/// by what its text mentions.
#[test]
fn an_edit_to_a_class_reached_only_through_a_return_type_drops_the_entry() {
    const FACTORY_URI: &str = "file:///Factory.php";
    const PRODUCT_URI: &str = "file:///Product.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const FACTORY: &str = "<?php\nclass Factory {\n    public function make(): Product {}\n}\n";
    const PRODUCT: &str = "<?php\nclass Product {\n    public function ship(): void {}\n}\n";
    // Neither `Product` nor `Factory` is named here: the receiver of `ship`
    // comes from `make()`'s declared return type.
    const CONSUMER: &str = r#"<?php
function run(Factory $factory): void {
    $factory->make()->ship();
}
"#;

    let backend = Backend::new_test();
    parse_extra(&backend, FACTORY_URI, FACTORY);
    parse_extra(&backend, PRODUCT_URI, PRODUCT);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);

    let ship_offset = PRODUCT.find("ship").unwrap() as u32;
    assert_eq!(
        backend
            .member_declaration_references(PRODUCT_URI, ship_offset, "ship", false)
            .len(),
        1,
        "the chained call is a reference"
    );

    let consumer_map = backend
        .symbol_maps
        .read()
        .get(CONSUMER_URI)
        .cloned()
        .unwrap();
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_some()
    );

    backend.update_ast(
        FACTORY_URI,
        "<?php\nclass Factory {\n    public function make(): Other {}\n}\n",
    );
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_none(),
        "the return type the chain resolved through has moved"
    );
}

/// The same for a global function: a helper's return type is what decides
/// the receiver of everything chained off it.
#[test]
fn an_edit_to_a_function_the_receiver_came_from_drops_the_entry() {
    const HELPERS_URI: &str = "file:///helpers.php";
    const PRODUCT_URI: &str = "file:///Product.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const HELPERS: &str = "<?php\nfunction product(): Product {}\n";
    const PRODUCT: &str = "<?php\nclass Product {\n    public function ship(): void {}\n}\n";
    const CONSUMER: &str = "<?php\nfunction run(): void {\n    product()->ship();\n}\n";

    let backend = Backend::new_test();
    parse_extra(&backend, HELPERS_URI, HELPERS);
    parse_extra(&backend, PRODUCT_URI, PRODUCT);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);

    let ship_offset = PRODUCT.find("ship").unwrap() as u32;
    assert_eq!(
        backend
            .member_declaration_references(PRODUCT_URI, ship_offset, "ship", false)
            .len(),
        1,
        "the call chained off the helper is a reference"
    );

    let consumer_map = backend
        .symbol_maps
        .read()
        .get(CONSUMER_URI)
        .cloned()
        .unwrap();
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_some()
    );

    backend.update_ast(HELPERS_URI, "<?php\nfunction product(): Other {}\n");
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_none(),
        "the helper whose return type the receiver came from has changed"
    );
}

/// A class a candidate file resolved through can inherit the member from a
/// parent the file never mentions, and a cached full resolution reads that
/// parent from the resolved-class cache rather than loading it.  The changed
/// set is closed over that cache's dependency graph for exactly this.
#[test]
fn an_edit_to_a_parent_drops_an_entry_that_only_named_the_child() {
    const BASE_URI: &str = "file:///Base.php";
    const CHILD_URI: &str = "file:///Child.php";
    const CONSUMER_URI: &str = "file:///Consumer.php";
    const BASE: &str = "<?php\nclass Base {\n    public function save(): void {}\n}\n";
    const CHILD: &str = "<?php\nclass Child extends Base {}\n";
    const CONSUMER: &str = "<?php\nfunction run(Child $child): void {\n    $child->save();\n}\n";

    let backend = Backend::new_test();
    parse_extra(&backend, BASE_URI, BASE);
    parse_extra(&backend, CHILD_URI, CHILD);
    parse_extra(&backend, CONSUMER_URI, CONSUMER);

    let save_offset = BASE.find("save").unwrap() as u32;
    assert_eq!(
        backend
            .member_declaration_references(BASE_URI, save_offset, "save", false)
            .len(),
        1,
        "the inherited call is a reference to the declaration on the parent"
    );

    let consumer_map = backend
        .symbol_maps
        .read()
        .get(CONSUMER_URI)
        .cloned()
        .unwrap();
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_some()
    );

    backend.update_ast(
        BASE_URI,
        "<?php\nclass Base {\n    public function save(): string {}\n}\n",
    );
    assert!(
        backend
            .resolved_member_file(CONSUMER_URI, &consumer_map)
            .is_none(),
        "the consumer's `Child` receiver inherits from the edited parent"
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
