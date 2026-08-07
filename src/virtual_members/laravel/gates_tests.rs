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
