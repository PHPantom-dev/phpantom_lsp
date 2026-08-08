use super::*;

fn scan(content: &str) -> GateScan {
    scan_gate_registrations(content)
}

#[test]
fn extracts_gate_define_names_and_closure_signature() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        class AuthServiceProvider {\n\
            public function boot(): void {\n\
                Gate::define('update-post', function (User $user, Post $post) {\n\
                    return $user->id === $post->author_id;\n\
                });\n\
            }\n\
        }\n";
    let scan = scan(content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].name, "update-post");
    assert_eq!(
        scan.definitions[0].signature.as_deref(),
        Some("User $user, Post $post")
    );
    // The offset points inside the opening quote, at the name itself.
    let offset = scan.definitions[0].offset as usize;
    assert!(content[offset..].starts_with("update-post"));
}

#[test]
fn extracts_arrow_function_signature() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define('view-dashboard', fn (User $user) => $user->isAdmin());\n";
    let scan = scan(content);
    assert_eq!(scan.definitions[0].signature.as_deref(), Some("User $user"));
}

#[test]
fn a_non_closure_callback_leaves_the_signature_unknown() {
    // `'Class@method'` and `[Class::class, 'method']` callbacks are legal but
    // carry no parameter list at the registration site.
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define('update-post', [PostPolicy::class, 'update']);\n";
    let scan = scan(content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].signature, None);
}

#[test]
fn a_computed_ability_name_is_skipped() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define($ability, fn () => true);\n\
        Gate::define('real-one', fn () => true);\n";
    let scan = scan(content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].name, "real-one");
}

#[test]
fn an_unrelated_gate_class_is_not_read_as_the_facade() {
    // A local `Gate` in the current namespace resolves to `App\Gate`, not the
    // facade, so its `define()` call registers nothing.
    let content = "<?php\n\
        namespace App;\n\
        Gate::define('not-an-ability', fn () => true);\n";
    assert!(scan(content).definitions.is_empty());
}

#[test]
fn an_aliased_facade_import_still_matches() {
    let content = "<?php\n\
        namespace App\\Providers;\n\
        use Illuminate\\Support\\Facades\\Gate as Authorization;\n\
        Authorization::define('update-post', fn () => true);\n";
    let scan = scan(content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].name, "update-post");
}

#[test]
fn extracts_gate_resource_abilities() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::resource('photos', PhotoPolicy::class);\n";
    let names: Vec<String> = scan(content)
        .definitions
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        vec![
            "photos.viewAny",
            "photos.view",
            "photos.create",
            "photos.update",
            "photos.delete",
        ]
    );
}

#[test]
fn an_explicit_gate_resource_ability_map_replaces_the_defaults() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::resource('photos', PhotoPolicy::class, ['publish' => 'publish']);\n";
    let names: Vec<String> = scan(content)
        .definitions
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["photos.publish"]);
}

#[test]
fn extracts_gate_policy_registrations() {
    let content = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Policies\\PostPolicy;\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        class AuthServiceProvider {\n\
            public function boot(): void {\n\
                Gate::policy(Post::class, PostPolicy::class);\n\
            }\n\
        }\n";
    let scan = scan(content);
    assert_eq!(scan.policies.len(), 1);
    assert_eq!(scan.policies[0].model_fqn, "App\\Models\\Post");
    assert_eq!(scan.policies[0].policy_fqn, "App\\Policies\\PostPolicy");
}

#[test]
fn extracts_the_policies_property_array() {
    let content = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Models\\Comment;\n\
        use App\\Policies\\PostPolicy;\n\
        use App\\Policies\\CommentPolicy;\n\
        class AuthServiceProvider {\n\
            protected $policies = [\n\
                Post::class => PostPolicy::class,\n\
                Comment::class => CommentPolicy::class,\n\
            ];\n\
        }\n";
    let scan = scan(content);
    let pairs: Vec<(String, String)> = scan
        .policies
        .into_iter()
        .map(|p| (p.model_fqn, p.policy_fqn))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (
                "App\\Models\\Post".to_string(),
                "App\\Policies\\PostPolicy".to_string()
            ),
            (
                "App\\Models\\Comment".to_string(),
                "App\\Policies\\CommentPolicy".to_string()
            ),
        ]
    );
}

#[test]
fn a_file_with_no_gate_reference_is_not_parsed() {
    let scan = scan("<?php\nclass Plain { public function boot(): void {} }\n");
    assert!(scan.is_empty());
}

/// A file whose only mention of `Gate` is an ordinary identifier registers
/// nothing, even though it does qualify for the walk.
#[test]
fn an_identifier_containing_gate_registers_nothing() {
    let scan = scan("<?php\n$subscriptionGateways = paymentGateway();\n");
    assert!(scan.is_empty());
}

/// PHP nests one AST node per link of a method chain, so a real file's chains
/// go deep enough to overflow a recursive walk.  The scan must survive one
/// alongside a registration it has to find.
#[test]
fn a_deep_method_chain_does_not_overflow_the_walk() {
    let mut content = String::from(
        "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define('deep', fn () => true);\n\
        $q = DB::table('t')",
    );
    for index in 0..2_000 {
        content.push_str(&format!("->where('c{index}', {index})"));
    }
    content.push_str(";\n");

    let scan = scan(&content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].name, "deep");
}

// ─── Registration shapes the scan declines to read ──────────────────────────

/// Every spelling of a `Gate::` call that names nothing recoverable. Each is
/// legal PHP the scanner must pass over rather than half-read.
#[test]
fn unrecoverable_gate_calls_register_nothing() {
    for source in [
        // No arguments at all.
        "Gate::define();",
        // The ability name is there but the string is empty.
        "Gate::define('', fn () => true);",
        // A dynamic method selector: which API this calls is not knowable.
        "Gate::{$method}('update-post', fn () => true);",
        // `policy()` needs both halves of the binding.
        "Gate::policy(Post::class);",
        // Neither half may be a runtime value.
        "Gate::policy($model, PostPolicy::class);",
        "Gate::policy(Post::class, $policy);",
        // A `::class` on a keyword names no loadable class here.
        "Gate::policy(self::class, PostPolicy::class);",
        // `Post::TABLE` is a class constant, not the class itself.
        "Gate::policy(Post::TABLE, PostPolicy::class);",
        // `resource()` derives its ability names from a literal prefix.
        "Gate::resource($name, PhotoPolicy::class);",
        // A check is not a registration.
        "Gate::allows('update-post', $post);",
        // The subject must resolve to the Gate, not to any object holding one.
        "$gate::define('update-post', fn () => true);",
    ] {
        let content = format!(
            "<?php\nuse Illuminate\\Support\\Facades\\Gate;\nuse App\\Models\\Post;\n{source}\n"
        );
        let scan = scan(&content);
        assert!(
            scan.is_empty(),
            "`{source}` should register nothing, got {scan:?}"
        );
    }
}

/// The `Gate` contract and its concrete implementation are the two other
/// names a provider may write the registration against.
#[test]
fn the_gate_contract_and_implementation_are_recognised() {
    for import in [
        "Illuminate\\Contracts\\Auth\\Access\\Gate",
        "Illuminate\\Auth\\Access\\Gate",
    ] {
        let content = format!(
            "<?php\nnamespace App;\nuse {import};\nGate::define('via-{import}', fn () => true);\n"
        );
        assert_eq!(
            scan(&content).definitions.len(),
            1,
            "`{import}` should be recognised as the gate"
        );
    }
}

#[test]
fn a_multi_line_closure_signature_is_normalised_to_one_line() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define('update-post', function (\n\
            User $user,\n\
            Post $post\n\
        ) {\n\
            return true;\n\
        });\n";
    assert_eq!(
        scan(content).definitions[0].signature.as_deref(),
        Some("User $user, Post $post")
    );
}

#[test]
fn an_ability_with_no_callback_has_no_signature() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::define('update-post');\n";
    let scan = scan(content);
    assert_eq!(scan.definitions.len(), 1);
    assert_eq!(scan.definitions[0].signature, None);
}

#[test]
fn gate_resource_accepts_a_plain_list_of_abilities() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::resource('photos', PhotoPolicy::class, ['publish', 'archive']);\n";
    let names: Vec<String> = scan(content)
        .definitions
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["photos.publish", "photos.archive"]);
}

#[test]
fn gate_resource_reads_a_legacy_array_of_abilities() {
    let content = "<?php\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        Gate::resource('photos', PhotoPolicy::class, array('publish' => 'publish'));\n";
    let names: Vec<String> = scan(content)
        .definitions
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["photos.publish"]);
}

/// An ability list the scan cannot read leaves the CRUD defaults in place: the
/// call does register *something*, and the default set is what Laravel uses
/// when the argument is omitted.
#[test]
fn gate_resource_falls_back_to_the_defaults_for_an_unreadable_ability_list() {
    for third in ["$abilities", "[$computed]", "[...$extra]"] {
        let content = format!(
            "<?php\n\
            use Illuminate\\Support\\Facades\\Gate;\n\
            Gate::resource('photos', PhotoPolicy::class, {third});\n"
        );
        let names: Vec<String> = scan(&content)
            .definitions
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "photos.viewAny",
                "photos.view",
                "photos.create",
                "photos.update",
                "photos.delete",
            ],
            "an unreadable `{third}` should leave the defaults"
        );
    }
}

// ─── `$policies` array shapes ───────────────────────────────────────────────

#[test]
fn a_legacy_policies_array_is_read() {
    let content = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Policies\\PostPolicy;\n\
        class AuthServiceProvider {\n\
            protected $policies = array(Post::class => PostPolicy::class);\n\
        }\n";
    let scan = scan(content);
    assert_eq!(scan.policies.len(), 1);
    assert_eq!(scan.policies[0].policy_fqn, "App\\Policies\\PostPolicy");
}

/// A registration may spell either half fully qualified rather than importing
/// it, and the leading separator is not part of the name.
#[test]
fn a_fully_qualified_registration_is_read() {
    let content = "<?php\n\
        namespace App\\Providers;\n\
        class AuthServiceProvider {\n\
            protected $policies = [\n\
                \\App\\Models\\Post::class => \\App\\Policies\\PostPolicy::class,\n\
            ];\n\
        }\n";
    let scan = scan(content);
    assert_eq!(scan.policies.len(), 1);
    assert_eq!(scan.policies[0].model_fqn, "App\\Models\\Post");
    assert_eq!(scan.policies[0].policy_fqn, "App\\Policies\\PostPolicy");
}

#[test]
fn a_policies_entry_written_as_a_string_is_read() {
    // `['App\Models\Post' => …]` is legal, if unidiomatic.  A value with no
    // namespace separator is not a class name, so it is passed over.
    let content = "<?php\n\
        namespace App\\Providers;\n\
        class AuthServiceProvider {\n\
            protected $policies = [\n\
                'App\\\\Models\\\\Post' => 'App\\\\Policies\\\\PostPolicy',\n\
                'post' => 'PostPolicy',\n\
            ];\n\
        }\n";
    let scan = scan(content);
    assert_eq!(scan.policies.len(), 1);
    assert_eq!(scan.policies[0].model_fqn, "App\\Models\\Post");
    assert_eq!(scan.policies[0].policy_fqn, "App\\Policies\\PostPolicy");
}

/// Every `$policies` shape that binds nothing readable.
#[test]
fn unreadable_policies_declarations_register_nothing() {
    for property in [
        // Declared but never assigned.
        "protected $policies;",
        // Built somewhere else.
        "protected $policies = self::POLICY_MAP;",
        // A list, not a map: nothing says which model it governs.
        "protected $policies = [PostPolicy::class];",
        // A key that is not knowable statically.
        "protected $policies = [$model => PostPolicy::class];",
        // A different property that happens to hold class pairs.
        "protected $middleware = [Post::class => PostPolicy::class];",
    ] {
        let content = format!(
            "<?php\n\
            namespace App\\Providers;\n\
            use App\\Models\\Post;\n\
            use App\\Policies\\PostPolicy;\n\
            class AuthServiceProvider {{\n\
                {property}\n\
                public function boot(): void {{ Gate::has('x'); }}\n\
            }}\n"
        );
        assert!(
            scan(&content).policies.is_empty(),
            "`{property}` should bind no policy"
        );
    }
}

// ─── Index ──────────────────────────────────────────────────────────────────

/// Scan order across files is not the runtime boot order, so a name declared
/// twice keeps whichever registration the index saw first — the point is that
/// the result is stable, not which file wins.
#[test]
fn a_duplicate_ability_keeps_one_registration() {
    let mut index = LaravelGateIndex::default();
    for uri in ["file:///a.php", "file:///b.php"] {
        index.set_file(
            uri.to_string(),
            scan(
                "<?php\nuse Illuminate\\Support\\Facades\\Gate;\nGate::define('shared', fn () => true);\n",
            ),
        );
    }
    index.rebuild();

    assert_eq!(index.definition_names(), vec!["shared".to_string()]);
    let target = index.definition("shared").expect("the ability is indexed");
    assert!(["file:///a.php", "file:///b.php"].contains(&target.uri.as_str()));
}

#[test]
fn a_policy_lookup_accepts_a_leading_separator_and_dedupes_the_policy_list() {
    let mut index = LaravelGateIndex::default();
    index.set_file(
        "file:///provider.php".to_string(),
        scan(
            "<?php\n\
            namespace App\\Providers;\n\
            use App\\Models\\Post;\n\
            use App\\Models\\Comment;\n\
            use App\\Policies\\ContentPolicy;\n\
            class AuthServiceProvider {\n\
                protected $policies = [\n\
                    Post::class => ContentPolicy::class,\n\
                    Comment::class => ContentPolicy::class,\n\
                ];\n\
            }\n",
        ),
    );
    index.rebuild();

    // A caller may hold either spelling of the model name.
    assert_eq!(
        index.policy_for("\\App\\Models\\Post"),
        Some("App\\Policies\\ContentPolicy")
    );
    assert_eq!(index.policy_for("App\\Models\\Unknown"), None);
    // Two models sharing a policy contribute it once.
    assert_eq!(
        index.registered_policy_fqns(),
        vec!["App\\Policies\\ContentPolicy".to_string()]
    );
}

#[test]
fn index_merges_files_and_keeps_the_first_registration() {
    let mut index = LaravelGateIndex::default();
    index.set_file(
        "file:///a.php".to_string(),
        scan("<?php\nuse Illuminate\\Support\\Facades\\Gate;\nGate::define('a', fn () => true);\n"),
    );
    index.set_file(
        "file:///b.php".to_string(),
        scan("<?php\nuse Illuminate\\Support\\Facades\\Gate;\nGate::define('b', fn () => true);\n"),
    );
    index.rebuild();

    let mut names = index.definition_names();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    assert!(index.has_uri("file:///a.php"));

    // Removing a file's contribution drops only its own abilities.
    index.set_file("file:///a.php".to_string(), GateScan::default());
    index.rebuild();
    assert_eq!(index.definition_names(), vec!["b".to_string()]);
    assert!(!index.has_uri("file:///a.php"));
}

#[test]
fn guessed_policy_names_follow_the_framework_order() {
    // Laravel tries the deepest namespace prefix first, so a policy sitting
    // next to the models wins over one under the application root.
    assert_eq!(
        guessed_policy_names("App\\Models\\Post"),
        vec![
            "App\\Models\\Policies\\PostPolicy".to_string(),
            "App\\Policies\\PostPolicy".to_string(),
        ]
    );
}

#[test]
fn guessed_policy_names_rewrite_a_nested_models_namespace() {
    let names = guessed_policy_names("App\\Models\\Blog\\Post");
    assert!(names.contains(&"App\\Policies\\Blog\\PostPolicy".to_string()));
    assert!(names.contains(&"App\\Models\\Blog\\Policies\\PostPolicy".to_string()));
    assert!(names.contains(&"App\\Policies\\PostPolicy".to_string()));
}

#[test]
fn guessed_policy_names_handle_a_global_namespace_model() {
    assert_eq!(
        guessed_policy_names("Post"),
        vec!["Policies\\PostPolicy".to_string()]
    );
}

// ─── Policy resolution against a workspace ──────────────────────────────────

/// A backend holding `files` (uri → source), with `provider` — when given —
/// scanned into the gate index the way `build_laravel_gate_index` does.
fn backend_with(files: &[(&str, &str)], provider: Option<(&str, &str)>) -> crate::Backend {
    let backend = crate::Backend::new_test();
    for (uri, content) in files {
        backend.update_ast(uri, content);
    }
    if let Some((uri, content)) = provider {
        backend.update_ast(uri, content);
        let mut index = backend.laravel_gates.write();
        index.set_file(uri.to_string(), scan(content));
        index.rebuild();
    }
    backend
}

const POST_MODEL: &str = "<?php\nnamespace App\\Models;\nclass Post {}\n";

const CONVENTION_POLICY: &str = "<?php\n\
    namespace App\\Policies;\n\
    class PostPolicy {\n\
        public function update($user, $post): bool { return true; }\n\
    }\n";

const REGISTERED_POLICY: &str = "<?php\n\
    namespace App\\Policies;\n\
    class LegacyPostPolicy {\n\
        public function publish($user, $post): bool { return true; }\n\
    }\n";

#[test]
fn a_registration_wins_over_the_naming_convention() {
    let provider = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Policies\\LegacyPostPolicy;\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        class AuthServiceProvider {\n\
            public function boot(): void {\n\
                Gate::policy(Post::class, LegacyPostPolicy::class);\n\
            }\n\
        }\n";
    let backend = backend_with(
        &[
            ("file:///app/Models/Post.php", POST_MODEL),
            ("file:///app/Policies/PostPolicy.php", CONVENTION_POLICY),
            (
                "file:///app/Policies/LegacyPostPolicy.php",
                REGISTERED_POLICY,
            ),
        ],
        Some(("file:///app/Providers/AuthServiceProvider.php", provider)),
    );

    let (policy, abilities) = model_policy_abilities(&backend, "App\\Models\\Post")
        .expect("the registered policy should resolve");
    assert_eq!(policy.fqn().as_str(), "App\\Policies\\LegacyPostPolicy");
    assert_eq!(abilities, vec!["publish".to_string()]);
}

/// A registration naming a class the workspace does not hold falls through to
/// the next route rather than resolving to nothing.
#[test]
fn a_registration_pointing_at_a_missing_class_falls_through() {
    let provider = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Policies\\Deleted;\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        class AuthServiceProvider {\n\
            public function boot(): void {\n\
                Gate::policy(Post::class, Deleted::class);\n\
            }\n\
        }\n";
    let backend = backend_with(
        &[
            ("file:///app/Models/Post.php", POST_MODEL),
            ("file:///app/Policies/PostPolicy.php", CONVENTION_POLICY),
        ],
        Some(("file:///app/Providers/AuthServiceProvider.php", provider)),
    );

    let policy = policy_class_for_model(&backend, "App\\Models\\Post")
        .expect("the convention should still find a policy");
    assert_eq!(policy.fqn().as_str(), "App\\Policies\\PostPolicy");
}

#[test]
fn a_use_policy_attribute_wins_over_the_naming_convention() {
    let model = "<?php\n\
        namespace App\\Models;\n\
        use App\\Policies\\LegacyPostPolicy;\n\
        use Illuminate\\Database\\Eloquent\\Attributes\\UsePolicy;\n\
        #[UsePolicy(LegacyPostPolicy::class)]\n\
        class Post {}\n";
    let backend = backend_with(
        &[
            ("file:///app/Models/Post.php", model),
            ("file:///app/Policies/PostPolicy.php", CONVENTION_POLICY),
            (
                "file:///app/Policies/LegacyPostPolicy.php",
                REGISTERED_POLICY,
            ),
        ],
        None,
    );

    let policy = policy_class_for_model(&backend, "App\\Models\\Post")
        .expect("the attribute should name the policy");
    assert_eq!(policy.fqn().as_str(), "App\\Policies\\LegacyPostPolicy");
}

#[test]
fn an_attribute_pointing_at_a_missing_class_falls_through() {
    let model = "<?php\n\
        namespace App\\Models;\n\
        use App\\Policies\\Deleted;\n\
        use Illuminate\\Database\\Eloquent\\Attributes\\UsePolicy;\n\
        #[UsePolicy(Deleted::class)]\n\
        class Post {}\n";
    let backend = backend_with(
        &[
            ("file:///app/Models/Post.php", model),
            ("file:///app/Policies/PostPolicy.php", CONVENTION_POLICY),
        ],
        None,
    );

    let policy = policy_class_for_model(&backend, "App\\Models\\Post")
        .expect("the convention should still find a policy");
    assert_eq!(policy.fqn().as_str(), "App\\Policies\\PostPolicy");
}

/// A model with no policy anywhere resolves to nothing — which a caller must
/// read as "nothing is known", not as "no abilities".
#[test]
fn a_model_with_no_policy_resolves_to_nothing() {
    // A leading separator on the model name must not change the answer.
    let backend = backend_with(&[("file:///app/Models/Post.php", POST_MODEL)], None);
    assert!(policy_class_for_model(&backend, "\\App\\Models\\Post").is_none());
    assert!(model_policy_abilities(&backend, "App\\Models\\Post").is_none());
    // Nor does a model the workspace has never seen.
    assert!(policy_class_for_model(&backend, "App\\Models\\Ghost").is_none());
}

#[test]
fn enumerated_abilities_union_definitions_and_policy_methods() {
    // The registration names a policy the workspace does not hold, which
    // contributes no abilities rather than derailing the enumeration.
    let provider = "<?php\n\
        namespace App\\Providers;\n\
        use App\\Models\\Post;\n\
        use App\\Policies\\Deleted;\n\
        use Illuminate\\Support\\Facades\\Gate;\n\
        class AuthServiceProvider {\n\
            public function boot(): void {\n\
                Gate::define('manage-billing', fn () => true);\n\
                Gate::policy(Post::class, Deleted::class);\n\
            }\n\
        }\n";
    // A vendor policy governs the package's own models, and loading every
    // `*Policy` under `vendor/` would cost more than the names are worth.
    let vendor_policy = "<?php\n\
        namespace Acme\\Package\\Policies;\n\
        class InvoicePolicy {\n\
            public function refund($user, $invoice): bool { return true; }\n\
        }\n";
    let backend = backend_with(
        &[
            ("file:///app/Policies/PostPolicy.php", CONVENTION_POLICY),
            (
                "file:///vendor/acme/package/src/Policies/InvoicePolicy.php",
                vendor_policy,
            ),
        ],
        Some(("file:///app/Providers/AuthServiceProvider.php", provider)),
    );

    assert_eq!(
        enumerate_gate_abilities(&backend),
        vec!["manage-billing".to_string(), "update".to_string()],
    );
}

#[test]
fn an_inherited_ability_is_attributed_to_the_class_that_declares_it() {
    let base = "<?php\n\
        namespace App\\Policies;\n\
        class BasePolicy {\n\
            public function restore($user, $model): bool { return true; }\n\
        }\n";
    let child = "<?php\n\
        namespace App\\Policies;\n\
        class PostPolicy extends BasePolicy {\n\
            public function update($user, $post): bool { return true; }\n\
        }\n";
    let sibling = "<?php\n\
        namespace App\\Policies;\n\
        class CommentPolicy extends BasePolicy {}\n";
    let backend = backend_with(
        &[
            ("file:///app/Policies/BasePolicy.php", base),
            ("file:///app/Policies/PostPolicy.php", child),
            ("file:///app/Policies/CommentPolicy.php", sibling),
        ],
        None,
    );

    // Both subclasses offer `restore`, but only the base declares it.
    let owners: Vec<String> = policy_methods_named(&backend, "restore")
        .into_iter()
        .map(|(policy, _)| policy.fqn().to_string())
        .collect();
    assert_eq!(owners, vec!["App\\Policies\\BasePolicy".to_string()]);

    // A method declared on the subclass is still attributed to it.
    let owners: Vec<String> = policy_methods_named(&backend, "update")
        .into_iter()
        .map(|(policy, _)| policy.fqn().to_string())
        .collect();
    assert_eq!(owners, vec!["App\\Policies\\PostPolicy".to_string()]);

    // A name no policy declares matches nothing.
    assert!(policy_methods_named(&backend, "nonexistent").is_empty());
}

/// Unrelated policies may each declare an ability of the same name, and both
/// are real answers — they are ordered by FQN so the list is stable.
#[test]
fn an_ability_declared_by_two_policies_reports_both_in_order() {
    let post = "<?php\n\
        namespace App\\Policies;\n\
        class PostPolicy {\n\
            public function update($user, $post): bool { return true; }\n\
        }\n";
    let comment = "<?php\n\
        namespace App\\Policies;\n\
        class CommentPolicy {\n\
            public function update($user, $comment): bool { return true; }\n\
        }\n";
    let backend = backend_with(
        &[
            ("file:///app/Policies/PostPolicy.php", post),
            ("file:///app/Policies/CommentPolicy.php", comment),
        ],
        None,
    );

    let owners: Vec<String> = policy_methods_named(&backend, "update")
        .into_iter()
        .map(|(policy, _)| policy.fqn().to_string())
        .collect();
    assert_eq!(
        owners,
        vec![
            "App\\Policies\\CommentPolicy".to_string(),
            "App\\Policies\\PostPolicy".to_string(),
        ]
    );
}

/// A policy extending a class the workspace does not hold cannot be walked any
/// further, so the ability is attributed to the policy itself.
#[test]
fn a_policy_extending_a_missing_class_stops_the_walk() {
    let policy = "<?php\n\
        namespace App\\Policies;\n\
        class PostPolicy extends \\Vendor\\Absent\\BasePolicy {\n\
            public function update($user, $post): bool { return true; }\n\
        }\n";
    let backend = backend_with(&[("file:///app/Policies/PostPolicy.php", policy)], None);

    // `update` is declared here, so the walk never needs the missing parent.
    let owners: Vec<String> = policy_methods_named(&backend, "update")
        .into_iter()
        .map(|(policy, _)| policy.fqn().to_string())
        .collect();
    assert_eq!(owners, vec!["App\\Policies\\PostPolicy".to_string()]);

    // Nothing here declares `restore`, and the parent that might cannot be
    // loaded, so the walk ends rather than looping or guessing.
    assert!(policy_methods_named(&backend, "restore").is_empty());
}

/// An ability a trait supplies is declared by no class in the parent chain, so
/// the policy the caller started from is reported instead of nothing.  The
/// walk has to reach that answer whether the chain ends at a class with no
/// parent or at one whose parent cannot be loaded.
#[test]
fn a_trait_supplied_ability_falls_back_to_the_policy_itself() {
    let trait_src = "<?php\n\
        namespace App\\Policies\\Concerns;\n\
        trait Restores {\n\
            public function restore($user, $model): bool { return true; }\n\
        }\n";
    for extends in ["", " extends \\Vendor\\Absent\\BasePolicy"] {
        let policy = format!(
            "<?php\n\
            namespace App\\Policies;\n\
            use App\\Policies\\Concerns\\Restores;\n\
            class PostPolicy{extends} {{\n\
                use Restores;\n\
            }}\n"
        );
        let backend = backend_with(
            &[
                ("file:///app/Policies/Concerns/Restores.php", trait_src),
                ("file:///app/Policies/PostPolicy.php", &policy),
            ],
            None,
        );

        let owners: Vec<String> = policy_methods_named(&backend, "restore")
            .into_iter()
            .map(|(policy, _)| policy.fqn().to_string())
            .collect();
        assert_eq!(
            owners,
            vec!["App\\Policies\\PostPolicy".to_string()],
            "a trait-supplied ability on `class PostPolicy{extends}`"
        );
    }
}

#[test]
fn policy_abilities_skip_hooks_and_non_public_methods() {
    let src = r#"<?php
namespace App\Policies;

class PostPolicy
{
    public function before($user, $ability) { return null; }
    public function after($user, $ability) { return null; }
    public function viewAny($user) { return true; }
    public function update($user, $post) { return true; }
    protected function helper() { return true; }
    private function secret() { return true; }
    public static function make(): self { return new self(); }
    public function __construct() {}
}
"#;
    let classes = crate::Backend::parse_php_versioned_with_namespaces(src, None);
    let class = classes
        .iter()
        .find(|(c, _)| c.name == crate::atom::atom("PostPolicy"))
        .map(|(c, _)| c)
        .unwrap();
    let names: Vec<String> = policy_abilities(class)
        .into_iter()
        .map(|m| m.name.to_string())
        .collect();
    assert_eq!(names, vec!["viewAny".to_string(), "update".to_string()]);
}
