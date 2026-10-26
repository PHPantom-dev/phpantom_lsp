use std::collections::HashSet;

use super::*;
use crate::Backend;

fn has_config_resource_span(
    backend: &Backend,
    uri: &str,
    key: &str,
    resource: crate::symbol_map::LaravelConfigResource,
) -> bool {
    backend.symbol_map_for(uri).is_some_and(|map| {
        map.spans.iter().any(|span| {
            matches!(
                &span.kind,
                crate::symbol_map::SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::ConfigResource(found),
                    key: found_key,
                    ..
                } if *found == resource && found_key == key
            )
        })
    })
}

fn reference_index_has_config_resource(
    backend: &Backend,
    uri: &str,
    key: &str,
    resource: crate::symbol_map::LaravelConfigResource,
) -> bool {
    let index_key = crate::reference_index::string_key_reference_key(
        crate::symbol_map::StringKeyKind::ConfigResource(resource),
        key,
    );
    backend
        .reference_index
        .read()
        .get(&index_key)
        .is_some_and(|entries| entries.keys().any(|entry_uri| entry_uri.as_ref() == uri))
}

#[test]
fn namespaced_auth_candidates_follow_cross_file_function_lifecycle() {
    use crate::symbol_map::LaravelConfigResource::AuthGuard;

    let backend = Backend::new_test();
    let consumer_uri = "file:///app/Consumer.php";
    let helper_uri = "file:///app/helpers.php";
    backend.update_ast(consumer_uri, "<?php\nnamespace App;\nauth('web');\n");
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));
    assert!(reference_index_has_config_resource(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));

    // An already-correct map must not be cloned or reindexed merely
    // because a workspace discovery refresh completed.
    assert!(!backend.refresh_all_published_laravel_candidates());

    let changed = backend.update_ast(
        helper_uri,
        "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
    );
    assert!(changed, "the consumer map changed even on a first parse");
    assert!(!has_config_resource_span(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));
    assert!(!reference_index_has_config_resource(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));

    let removed = HashSet::from([helper_uri.to_string()]);
    backend.withdraw_functions_for_uris(&removed);
    assert!(backend.refresh_all_published_laravel_candidates());
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));
    assert!(reference_index_has_config_resource(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));

    backend.update_ast(
        helper_uri,
        "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
    );
    assert!(!has_config_resource_span(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));

    backend.update_ast(helper_uri, "<?php\nnamespace App;\n");
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));
    assert!(reference_index_has_config_resource(
        &backend,
        consumer_uri,
        "web",
        AuthGuard
    ));
}

#[test]
fn prospective_batch_functions_shadow_auth_candidates() {
    use crate::symbol_map::LaravelConfigResource::AuthGuard;

    let backend = Backend::new_test();
    let consumer_uri = "file:///app/Consumer.php";
    let helper_uri = "file:///app/helpers.php";
    let consumer = backend
        .parse_ast_index_update_for_index(consumer_uri, "<?php\nnamespace App;\nauth('admin');\n");
    let helper = backend.parse_ast_index_update_for_index(
        helper_uri,
        "<?php\nnamespace App;\nfunction auth(?string $guard = null): object {}\n",
    );
    backend.apply_ast_index_parse_results_batch(vec![consumer, helper]);

    assert!(!has_config_resource_span(
        &backend,
        consumer_uri,
        "admin",
        AuthGuard
    ));
    assert!(!reference_index_has_config_resource(
        &backend,
        consumer_uri,
        "admin",
        AuthGuard
    ));
}

#[test]
fn root_facade_candidates_follow_global_class_lifecycle() {
    use crate::symbol_map::LaravelConfigResource::{CacheStore, StorageDisk};

    let backend = Backend::new_test();
    let consumer_uri = "file:///app/Consumer.php";
    let class_uri = "file:///app/Cache.php";
    backend.update_ast(
        consumer_uri,
        "<?php\nCache::store('redis');\nStorage::disk('local');\nRoute::get('/')->where('id')->middleware('auth:route');\n",
    );
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "redis",
        CacheStore
    ));
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "local",
        StorageDisk
    ));
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "route",
        crate::symbol_map::LaravelConfigResource::AuthGuard
    ));

    backend.update_ast(
        class_uri,
        "<?php\nclass Cache { public static function store() {} }\nclass Route { public static function get() {} }\n",
    );
    assert!(!has_config_resource_span(
        &backend,
        consumer_uri,
        "redis",
        CacheStore
    ));
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "local",
        StorageDisk
    ));
    assert!(!has_config_resource_span(
        &backend,
        consumer_uri,
        "route",
        crate::symbol_map::LaravelConfigResource::AuthGuard
    ));
    let map = backend
        .symbol_map_for(consumer_uri)
        .expect("consumer map should remain published");
    let disk_indices = map.member_access_indices("disk");
    assert_eq!(disk_indices.len(), 1);
    assert!(matches!(
        &map.spans[disk_indices[0]].kind,
        crate::symbol_map::SymbolKind::MemberAccess { member_name, .. }
            if member_name == "disk"
    ));

    backend.update_ast(class_uri, "<?php\nclass Other {}\n");
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "redis",
        CacheStore
    ));
    assert!(has_config_resource_span(
        &backend,
        consumer_uri,
        "route",
        crate::symbol_map::LaravelConfigResource::AuthGuard
    ));
}

#[test]
fn batch_publication_indexes_runtime_writes_without_opening_the_writer() {
    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(true);
    let writer_uri = "file:///project/tests/DiskTest.php";
    let reader_uri = "file:///project/app/Reader.php";
    let writer = backend.parse_ast_index_update_for_index(
        writer_uri,
        "<?php\nconfig(['cache.stores.scratch' => []]);\nConfig::set('filesystems.disks.temporary', []);\nStorage::fake('temporary');\nStorage::persistentFake('persistent');\n",
    );
    let reader = backend.parse_ast_index_update_for_index(
        reader_uri,
        "<?php\nStorage::disk('temporary');\nCache::store('scratch');\n",
    );

    backend.apply_ast_index_parse_results_batch(vec![reader, writer]);

    assert_eq!(
        backend.laravel.runtime_config_keys.read()[writer_uri],
        [
            "cache.stores.scratch",
            "filesystems.disks.persistent",
            "filesystems.disks.temporary",
        ]
    );
    let writes = backend.laravel.runtime_config_keys.read();
    assert_eq!(
        writes[writer_uri].len(),
        3,
        "duplicate writes share one key"
    );
    assert!(!writes.contains_key(reader_uri));
}

#[test]
fn runtime_writes_follow_cross_file_facade_shadows() {
    for (facade, call, key) in [
        (
            "Storage",
            "Storage::fake('scratch')",
            "filesystems.disks.scratch",
        ),
        (
            "Config",
            "Config::set('cache.stores.scratch', [])",
            "cache.stores.scratch",
        ),
    ] {
        let backend = Backend::new_test();
        backend
            .resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(true);
        let writer_uri = "file:///project/tests/FixtureTest.php";
        let shadow_uri = "file:///project/app/LocalFacade.php";
        backend.update_ast(writer_uri, &format!("<?php\n{call};\n"));
        assert!(backend.runtime_config_key_covers(key), "{facade}");

        backend.update_ast(shadow_uri, &format!("<?php\nclass {facade} {{}}\n"));
        assert!(
            backend.laravel.runtime_config_keys.read().is_empty(),
            "a global {facade} class must withdraw the unedited file's write"
        );

        backend.update_ast(shadow_uri, "<?php\nclass Other {}\n");
        assert!(
            backend.runtime_config_key_covers(key),
            "removing the {facade} shadow must restore the unedited file's write"
        );
    }
}

#[test]
fn discovery_refresh_reconciles_runtime_writes_with_facade_aliases() {
    let backend = Backend::new_test();
    backend
        .resolved_class_cache
        .write()
        .laravel_mut()
        .set_enabled(true);
    let writer_uri = "file:///project/tests/DiskTest.php";
    let shadow_uri = "file:///project/app/Storage.php";
    let key = "filesystems.disks.scratch";
    backend.update_ast(writer_uri, "<?php\nStorage::fake('scratch');\n");
    assert!(backend.runtime_config_key_covers(key));

    backend.symbols.with_class_declarations(|declarations| {
        declarations.note_discovered("Storage", shadow_uri.to_string());
    });
    assert!(backend.refresh_all_published_laravel_candidates());
    assert!(backend.laravel.runtime_config_keys.read().is_empty());

    backend.symbols.with_class_declarations(|declarations| {
        declarations.withdraw_uris(&HashSet::from([shadow_uri.to_string()]));
    });
    assert!(backend.refresh_all_published_laravel_candidates());
    assert!(backend.runtime_config_key_covers(key));
    assert!(!backend.refresh_all_published_laravel_candidates());
}

#[test]
fn controller_middleware_does_not_claim_auth_guards_without_a_typed_receiver() {
    use crate::symbol_map::LaravelConfigResource::AuthGuard;

    let backend = Backend::new_test();
    let uri = "file:///app/Controller.php";
    backend.update_ast(
        uri,
        "<?php\nclass Controller {\n    function boot() {\n        $this->middleware('auth:web');\n        $this->middleware('can:update,Post');\n    }\n}\nRoute::get('/')->middleware('auth:admin');\nRoute::get('/')->a()->b()->c()->d()->middleware('auth:too-deep');\n",
    );
    assert!(!has_config_resource_span(&backend, uri, "web", AuthGuard));
    assert!(has_config_resource_span(&backend, uri, "admin", AuthGuard));
    assert!(!has_config_resource_span(
        &backend, uri, "too-deep", AuthGuard
    ));
    assert!(backend.symbol_map_for(uri).is_some_and(|map| {
        map.spans.iter().any(|span| {
            matches!(
                &span.kind,
                crate::symbol_map::SymbolKind::StringKey {
                    kind: crate::symbol_map::StringKeyKind::GateAbility,
                    key,
                    ..
                } if key == "update"
            )
        })
    }));
}

/// Two parses of one template can race: an index worker lowers it with
/// the scope it had, while call-site inference re-parses it with a
/// larger one.  Whichever lands last, the virtual PHP, the source map
/// and the symbol map must all come from the same parse, or every
/// offset in the map points at the wrong text and every position
/// translated back to the template is off by the prologue difference.
#[test]
fn racing_template_parses_leave_one_consistent_lowering() {
    let backend = Backend::new_test();
    let uri = "file:///resources/views/shop.blade.php";
    let template = "<p>{{ $a }}</p>\n{{ \\App\\Item::class }}\n";

    let stale = backend.parse_ast_index_update_for_index(uri, template);
    backend.blade.injected_vars.write().insert(
        uri.to_string(),
        crate::blade::call_site_inference::BladeScope {
            vars: vec![
                ("a".to_string(), "int".to_string()),
                ("b".to_string(), "string".to_string()),
            ],
            ..Default::default()
        },
    );
    backend.update_ast(uri, template);
    backend.apply_ast_index_parse_results_batch(vec![stale]);

    let virtual_php = backend.blade_virtual_php_arc(uri).unwrap();
    let symbol_map = backend.symbol_map_for(uri).unwrap();
    assert!(
        symbol_map.matches_source(&virtual_php),
        "the symbol map must index the virtual PHP that was published with it"
    );

    let item = symbol_map
        .spans
        .iter()
        .find(|span| matches!(&span.kind, crate::symbol_map::SymbolKind::ClassReference { name, .. } if name.ends_with("Item")))
        .expect("the template names App\\Item");
    let position = crate::text_position::offset_to_position(&virtual_php, item.start as usize);
    assert_eq!(
        backend
            .try_translate_php_to_blade(uri, position)
            .map(|p| p.line),
        Some(1),
        "App\\Item is on the template's second line"
    );
}

/// A stale index parse must not pin a template back to the scope it
/// started with. If call-site re-inference computes a new scope and
/// re-parses the template before the index worker's older-scoped batch
/// applies, the batch's lowering describes variables the template no
/// longer has and must be dropped rather than published over the fresh
/// state.
#[test]
fn stale_scope_batch_does_not_clobber_a_fresher_reinference() {
    let backend = Backend::new_test();
    let uri = "file:///resources/views/shop.blade.php";
    let template = "<p>{{ $a }}</p>\n{{ \\App\\Item::class }}\n";

    // An index worker lowers the template while the scope is still
    // empty (nothing has been cached for this uri yet).
    let stale = backend.parse_ast_index_update_for_index(uri, template);

    // Call-site re-inference computes the real scope and re-parses
    // against it before the index worker's batch is applied.
    backend.blade.injected_vars.write().insert(
        uri.to_string(),
        crate::blade::call_site_inference::BladeScope {
            vars: vec![("a".to_string(), "int".to_string())],
            ..Default::default()
        },
    );
    backend.update_ast(uri, template);
    let fresh_virtual_php = backend.blade_virtual_php_arc(uri).unwrap();

    // The index worker's batch, built from the empty scope, lands last.
    backend.apply_ast_index_parse_results_batch(vec![stale]);

    assert_eq!(
        backend.blade_virtual_php_arc(uri).unwrap(),
        fresh_virtual_php,
        "a batch built from a scope the template has moved past must not \
         overwrite the lowering built from the current scope"
    );
}

/// A `belongsToMany` body with `->as(...)`, `->using(...)`, and
/// `->withPivot(...)` must populate `belongs_to_many_pivots`, with the
/// pivot class resolved to an FQN.
#[test]
fn parses_pivot_accessor_using_and_columns_from_relationship_body() {
    let backend = Backend::new_test();
    let uri = "file:///app/Models/User.php";
    let content = "<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany;
class User extends Model {
/** @return BelongsToMany<Role, $this> */
public function roles(): BelongsToMany {
    return $this->belongsToMany(Role::class)->as('participation')->using(RoleUser::class)->withPivot('expires_at', 'active');
}
}
";
    backend.update_ast(uri, content);
    let classes = backend.symbols.uri_classes_index.read();
    let user = classes
        .get(uri)
        .and_then(|c| c.iter().find(|c| c.name == "User"))
        .expect("User class should be indexed");
    let pivots = &user
        .laravel()
        .expect("User should have Laravel metadata")
        .belongs_to_many_pivots;
    assert_eq!(pivots.len(), 1, "one pivot relation, got: {pivots:?}");
    assert_eq!(pivots[0].method, "roles");
    assert_eq!(
        pivots[0].accessor,
        crate::types::PivotAccessor::Custom(crate::atom::atom("participation"))
    );
    assert_eq!(
        pivots[0].using.as_deref(),
        Some("App\\Models\\RoleUser"),
        "using() class should be resolved to an FQN"
    );
    assert_eq!(pivots[0].columns, vec!["expires_at", "active"]);
}

#[test]
fn marks_dynamic_pivot_accessor_as_unknown() {
    let backend = Backend::new_test();
    let uri = "file:///app/Models/User.php";
    let content = "<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany;
class User extends Model {
/** @return BelongsToMany<Role, $this> */
public function roles(string $accessor): BelongsToMany {
    return $this->belongsToMany(Role::class)->as($accessor);
}
}
";
    backend.update_ast(uri, content);
    let classes = backend.symbols.uri_classes_index.read();
    let user = classes
        .get(uri)
        .and_then(|c| c.iter().find(|c| c.name == "User"))
        .expect("User class should be indexed");
    let pivot = user
        .laravel()
        .and_then(|laravel| laravel.belongs_to_many_pivots.first())
        .expect("dynamic as() should still record pivot metadata");
    assert_eq!(pivot.accessor, crate::types::PivotAccessor::Unknown);
}

/// A background/workspace parse batch must not clobber the state of a
/// file that is currently open in the editor. The open buffer's own
/// `update_ast` has already published newer (post-edit) state; the
/// batch result was parsed from stale disk content and must be skipped.
#[test]
fn batch_publish_skips_open_files() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Widget.php";

    // The editor buffer has been edited: method `edited` exists.
    let edited = "<?php\nclass Widget {\n    public function edited(): int { return 1; }\n}\n";
    backend.update_ast(uri, edited);
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(edited.to_string()));

    // A background index parses the file straight from disk, where it
    // still has the pre-edit method `stale`.
    let stale = "<?php\nclass Widget {\n    public function stale(): int { return 1; }\n}\n";
    let result = backend.parse_ast_index_update_for_index(uri, stale);
    backend.apply_ast_index_parse_results_batch(vec![result]);

    // The open buffer's state must survive: the class still exposes
    // `edited`, not the stale `stale`.
    let classes = backend.symbols.uri_classes_index.read();
    let widget = classes
        .get(uri)
        .and_then(|c| c.first())
        .expect("Widget class should still be indexed");
    assert!(
        widget.methods.iter().any(|m| m.name == "edited"),
        "open buffer's edited method must survive the batch publish"
    );
    assert!(
        !widget.methods.iter().any(|m| m.name == "stale"),
        "stale disk parse must not clobber the open buffer"
    );
}

/// The open-file skip must not affect files that are not open: a
/// background parse of an unopened file still publishes normally.
#[test]
fn batch_publish_applies_unopened_files() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Gadget.php";

    let content = "<?php\nclass Gadget {\n    public function run(): int { return 1; }\n}\n";
    let result = backend.parse_ast_index_update_for_index(uri, content);
    backend.apply_ast_index_parse_results_batch(vec![result]);

    let classes = backend.symbols.uri_classes_index.read();
    let gadget = classes
        .get(uri)
        .and_then(|c| c.first())
        .expect("unopened Gadget class should be indexed by the batch");
    assert!(
        gadget.methods.iter().any(|m| m.name == "run"),
        "unopened file must publish normally"
    );
}

#[test]
fn ast_index_parse_result_batch_records_failures_and_empty_noops() {
    let backend = Backend::new_test();
    assert!(!backend.apply_ast_index_parse_results_batch(Vec::new()));

    let uri = "file:///project/src/Broken.php";
    let changed =
        backend.apply_ast_index_parse_results_batch(vec![AstIndexParseResult::ParseFailed {
            uri: uri.to_string(),
            errors: vec![("Parse failed (internal error)".to_string(), 10, 20)],
        }]);

    assert!(!changed);
    assert_eq!(
        backend
            .file_state(uri)
            .map(|state| state.parse_errors.clone()),
        Some(vec![("Parse failed (internal error)".to_string(), 10, 20)])
    );
}

/// When a class name is declared in two files, the fqn-keyed derived
/// indexes must describe the same declaration the class index resolved
/// the name to.  Re-parsing the losing copy used to clear the winner's
/// reverse-inheritance entries without putting them back, so
/// go-to-implementation stopped listing the class.
#[test]
fn duplicate_class_derived_indexes_follow_the_winner() {
    let backend = Backend::new_test();

    let rich = "<?php namespace Vendor; class Variant extends Base { public function rich() {} }";
    let bare = "<?php namespace Vendor; class Variant {}";

    backend.update_ast("file:///b_bare.php", bare);
    backend.update_ast("file:///a_rich.php", rich);

    // Re-parse only the losing file.
    backend.update_ast(
        "file:///b_bare.php",
        "<?php namespace Vendor; class Variant { /* edited */ }",
    );

    assert!(
        backend
            .symbols
            .gti_index
            .read()
            .get("Vendor\\Base")
            .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
        "the winner's parent must still list it as an implementor"
    );
}

/// A class the batch declares for the first time still reaches the
/// derived indexes: restricting eviction to names whose owner changed
/// must not also restrict what gets populated.
#[test]
fn newly_declared_class_reaches_derived_indexes() {
    let backend = Backend::new_test();

    backend.update_ast(
        "file:///fresh.php",
        "<?php namespace Vendor; class Fresh extends Base { public function hello() {} }",
    );

    assert!(
        backend
            .symbols
            .gti_index
            .read()
            .get("Vendor\\Base")
            .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Fresh")),
        "a first-parse class must populate the reverse-inheritance index"
    );
}

/// When the winning file stops declaring a duplicated name, the
/// derived indexes must be rebuilt from the declaration that takes
/// over, not left describing the withdrawn one.
#[test]
fn promoted_class_declaration_refreshes_derived_indexes() {
    let backend = Backend::new_test();

    backend.update_ast(
        "file:///a_rich.php",
        "<?php namespace Vendor; class Variant extends Rich { public function rich() {} }",
    );
    backend.update_ast(
        "file:///b_bare.php",
        "<?php namespace Vendor; class Variant extends Bare { public function bare() {} }",
    );

    // The winner is renamed, so only b_bare.php declares the name.
    backend.update_ast(
        "file:///a_rich.php",
        "<?php namespace Vendor; class Renamed extends Rich { public function rich() {} }",
    );

    let gti = backend.symbols.gti_index.read();
    assert!(
        gti.get("Vendor\\Bare")
            .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
        "the promoted declaration's parent must list it as an implementor"
    );
    assert!(
        !gti.get("Vendor\\Rich")
            .is_some_and(|kids| kids.iter().any(|k| k == "Vendor\\Variant")),
        "the withdrawn declaration's parent must not still list it"
    );
}

/// An unclosed call swallows the `}` of the braced `namespace` block it
/// is in, which the parser then drops whole.  The block's namespace,
/// imports and declarations are still indexed, and the syntax errors
/// reported are those of the text as written.
#[test]
fn an_unclosed_call_keeps_its_braced_namespace_block_indexed() {
    let backend = Backend::new_test();
    let uri = "file:///blocks.php";
    backend.update_ast(
        uri,
        concat!(
            "<?php\n",
            "namespace Lib {\n",
            "    function greet(string $name): void {}\n",
            "}\n",
            "namespace App {\n",
            "    use function Lib\\greet;\n",
            "    class Widget {}\n",
            "    function wave(): void {}\n",
            "    greet(\n",
            "}\n",
        ),
    );

    let spans = backend.namespace_spans_for_uri(uri);
    let names: Vec<_> = spans.iter().map(|s| s.namespace.as_deref()).collect();
    assert_eq!(names, [Some("Lib"), Some("App")]);
    assert_eq!(
        spans[1]
            .symbol_imports
            .functions
            .get("greet")
            .map(String::as_str),
        Some("Lib\\greet")
    );

    let classes = backend.symbols.uri_classes_index.read();
    let widget = classes
        .get(uri)
        .and_then(|c| c.iter().find(|c| c.name == "Widget"))
        .expect("Widget is indexed");
    assert_eq!(widget.file_namespace.as_deref(), Some("App"));
    drop(classes);
    assert!(
        backend
            .symbols
            .global_functions
            .read()
            .contains_key("App\\wave")
    );

    let errors = backend.file_state(uri).unwrap().parse_errors.clone();
    assert!(
        errors
            .iter()
            .any(|(message, _, _)| message.ends_with("unexpected end of file, expected `}`")),
        "the block's missing `}}` is still reported: {errors:?}"
    );
}

/// An inheritance edge is deduplicated through the class's own list of
/// parents rather than through the parent's list of children, so a
/// re-parse that leaves the edge unchanged must still leave exactly one
/// entry for it.
#[test]
fn reparsing_a_class_does_not_duplicate_its_inheritance_edges() {
    let backend = Backend::new_test();
    let src =
        "<?php namespace Vendor; class Child extends Base implements Contract { use Helper; }";

    backend.update_ast("file:///child.php", src);
    backend.update_ast("file:///child.php", src);

    let gti = backend.symbols.gti_index.read();
    for parent in ["Vendor\\Base", "Vendor\\Contract", "Vendor\\Helper"] {
        let kids = gti
            .get(parent)
            .unwrap_or_else(|| panic!("{parent} should list Child as an implementor"));
        assert_eq!(
            kids.iter().filter(|k| *k == "Vendor\\Child").count(),
            1,
            "{parent} should list Child exactly once, got {kids:?}"
        );
    }
}

/// The diagnostic pass an edit schedules reads the parse the edit made
/// for the index instead of parsing the buffer again.
#[test]
fn open_file_parse_is_handed_to_its_diagnostic_pass() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Widget.php";
    let content = "<?php\nclass Widget {\n    public function run(): int { return 1; }\n}\n";
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));
    backend.update_ast(uri, content);

    let parsed = backend
        .take_parsed_for_diagnostics(uri, content)
        .expect("the open file's parse is kept");
    let kept_program = std::ptr::from_ref(parsed.program()) as usize;
    let text = std::sync::Arc::clone(parsed.content());
    let _guard = crate::parser::with_parsed_file_cache(parsed);
    let read_program = crate::parser::with_parsed_program(&text, "test", |program, _| {
        std::ptr::from_ref(program) as usize
    });
    assert_eq!(read_program, kept_program, "the pass reparsed the file");
    assert!(
        backend.take_parsed_for_diagnostics(uri, content).is_none(),
        "a parse is handed over once"
    );
}

#[test]
fn a_parse_is_only_kept_for_the_text_and_file_it_read() {
    let backend = Backend::new_test();
    let uri = "file:///project/src/Widget.php";
    let before = "<?php\nclass Widget {}\n";
    let after = "<?php\nclass Widget { const A = 1; }\n";

    // A file that is not open is not diagnosed after its parse.
    backend.update_ast(uri, before);
    assert!(backend.take_parsed_for_diagnostics(uri, before).is_none());

    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(before.to_string()));
    backend.update_ast(uri, before);
    assert!(
        backend
            .take_parsed_for_diagnostics("file:///project/src/Other.php", before)
            .is_none(),
        "another file's pass does not get this parse"
    );
    assert!(
        backend.take_parsed_for_diagnostics(uri, after).is_none(),
        "a pass over newer text does not get the older parse"
    );
}

#[test]
fn a_constant_removed_before_the_php_version_is_not_published() {
    let backend = Backend::new_test();
    backend.set_php_version(crate::types::PhpVersion::new(8, 0));
    backend.update_ast(
        "file:///standard_defines.php",
        "<?php\ndefine('ASSERT_ACTIVE', 1);\n/**\n * @removed 8.0\n */\ndefine('ASSERT_QUIET_EVAL', 5);\n",
    );

    let defines = backend.symbols.global_defines.read();
    assert!(defines.contains_key("ASSERT_ACTIVE"));
    assert!(!defines.contains_key("ASSERT_QUIET_EVAL"));
}
