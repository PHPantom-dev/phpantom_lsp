use crate::common::create_test_backend;
use phpantom_lsp::Visibility;

// ─── PHP Parsing / AST Extraction Tests ─────────────────────────────────────

#[tokio::test]
async fn test_parse_php_extracts_class_and_methods() {
    let backend = create_test_backend();
    let php = "<?php\nclass User {\n    function login() {}\n    function logout() {}\n}\n";

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "User");
    assert_eq!(classes[0].methods.len(), 2);
    assert_eq!(classes[0].methods[0].name, "login");
    assert_eq!(classes[0].methods[1].name, "logout");
}

#[tokio::test]
async fn test_parse_php_ignores_standalone_functions() {
    let backend = create_test_backend();
    let php = "<?php\nfunction standalone() {}\nclass Service {\n    function handle() {}\n}\n";

    let classes = backend.parse_php(php);
    assert_eq!(
        classes.len(),
        1,
        "Only class declarations should be extracted"
    );
    assert_eq!(classes[0].name, "Service");
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(classes[0].methods[0].name, "handle");
}

#[tokio::test]
async fn test_parse_php_no_classes_returns_empty() {
    let backend = create_test_backend();
    let php = "<?php\nfunction foo() {}\n$x = 1;\n";

    let classes = backend.parse_php(php);
    assert!(classes.is_empty(), "No classes should be found");
}

#[tokio::test]
async fn test_parse_php_extracts_properties() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public int $age;\n",
        "    private $secret;\n",
        "    function login() {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(
        classes[0].properties.len(),
        3,
        "Should extract 3 properties"
    );

    let prop_names: Vec<&str> = classes[0]
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(prop_names.contains(&"name"), "Should contain 'name'");
    assert!(prop_names.contains(&"age"), "Should contain 'age'");
    assert!(prop_names.contains(&"secret"), "Should contain 'secret'");

    // Verify type hints
    let name_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "name")
        .unwrap();
    assert_eq!(
        name_prop.type_hint_str().as_deref(),
        Some("string"),
        "name property should have string type hint"
    );

    let age_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "age")
        .unwrap();
    assert_eq!(
        age_prop.type_hint_str().as_deref(),
        Some("int"),
        "age property should have int type hint"
    );

    let secret_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "secret")
        .unwrap();
    assert_eq!(
        secret_prop.type_hint, None,
        "secret property should have no type hint"
    );
}

#[tokio::test]
async fn test_parse_php_extracts_static_properties() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Counter {\n",
        "    public static int $count = 0;\n",
        "    public string $label;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].properties.len(), 2);

    let count_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "count")
        .expect("Should have count property");
    assert!(count_prop.is_static, "count should be static");

    let label_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "label")
        .expect("Should have label property");
    assert!(!label_prop.is_static, "label should not be static");
}

#[tokio::test]
async fn test_parse_php_extracts_method_return_type() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    function greet(string $name): string {}\n",
        "    function doStuff() {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].methods.len(), 2);

    let greet = &classes[0].methods[0];
    assert_eq!(greet.name, "greet");
    assert_eq!(
        greet.return_type_str().as_deref(),
        Some("string"),
        "greet should have return type 'string'"
    );
    assert_eq!(greet.parameters.len(), 1);
    assert_eq!(greet.parameters[0].name, "$name");
    assert!(greet.parameters[0].is_required);
    assert_eq!(
        greet.parameters[0].type_hint_str().as_deref(),
        Some("string")
    );

    let do_stuff = &classes[0].methods[1];
    assert_eq!(do_stuff.name, "doStuff");
    assert_eq!(
        do_stuff.return_type, None,
        "doStuff should have no return type"
    );
}

#[tokio::test]
async fn test_parse_php_method_parameter_info() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    function process(string $input, int $count, ?string $label = null, ...$extras): bool {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let method = &classes[0].methods[0];
    assert_eq!(method.name, "process");
    assert_eq!(method.parameters.len(), 4);

    let input = &method.parameters[0];
    assert_eq!(input.name, "$input");
    assert!(input.is_required);
    assert_eq!(input.type_hint_str().as_deref(), Some("string"));
    assert!(!input.is_variadic);

    let count = &method.parameters[1];
    assert_eq!(count.name, "$count");
    assert!(count.is_required);
    assert_eq!(count.type_hint_str().as_deref(), Some("int"));

    let label = &method.parameters[2];
    assert_eq!(label.name, "$label");
    assert!(
        !label.is_required,
        "$label has a default value, should not be required"
    );
    assert_eq!(label.type_hint_str().as_deref(), Some("?string"));

    let extras = &method.parameters[3];
    assert_eq!(extras.name, "$extras");
    assert!(
        !extras.is_required,
        "variadic params should not be required"
    );
    assert!(extras.is_variadic);
}

/// A method's `@param` tags merge the same way a standalone function's
/// do: a description per named parameter, and — for a tag that omits its
/// variable name (common in phpstorm-stubs-style docs) — a positional
/// fallback once the name-based pass leaves a parameter unenriched.
#[tokio::test]
async fn test_parse_php_method_param_docblock_positional_fallback_and_description() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param mixed $items The initial items.\n",
        "     * @param callable(mixed): bool\n",
        "     */\n",
        "    function filter($items, $callback): array {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let method = &classes[0].methods[0];
    assert_eq!(method.parameters.len(), 2);

    let items = &method.parameters[0];
    assert_eq!(items.name, "$items");
    assert_eq!(
        items.description.as_deref(),
        Some("The initial items."),
        "a method parameter's @param description should merge, same as a function's"
    );

    let callback = &method.parameters[1];
    assert_eq!(callback.name, "$callback");
    assert_eq!(
        callback.type_hint_str().as_deref(),
        Some("callable(mixed): bool"),
        "an unnamed @param tag should match $callback by position"
    );
}

#[tokio::test]
async fn test_parse_php_property_with_default_value() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Settings {\n",
        "    public bool $debug = false;\n",
        "    public string $title = 'default';\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].properties.len(), 2);

    let prop_names: Vec<&str> = classes[0]
        .properties
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(prop_names.contains(&"debug"));
    assert!(prop_names.contains(&"title"));
}

#[tokio::test]
async fn test_parse_php_class_inside_implicit_namespace() {
    let backend = create_test_backend();
    let php = "<?php\nnamespace Demo;\n\nclass User {\n    function login() {}\n    function logout() {}\n}\n";

    let classes = backend.parse_php(php);
    assert_eq!(
        classes.len(),
        1,
        "Should find class inside implicit namespace"
    );
    assert_eq!(classes[0].name, "User");
    assert_eq!(classes[0].methods.len(), 2);
    assert_eq!(classes[0].methods[0].name, "login");
    assert_eq!(classes[0].methods[1].name, "logout");
}

#[tokio::test]
async fn test_parse_php_class_inside_brace_delimited_namespace() {
    let backend = create_test_backend();
    let php =
        "<?php\nnamespace Demo {\n    class Service {\n        function handle() {}\n    }\n}\n";

    let classes = backend.parse_php(php);
    assert_eq!(
        classes.len(),
        1,
        "Should find class inside brace-delimited namespace"
    );
    assert_eq!(classes[0].name, "Service");
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(classes[0].methods[0].name, "handle");
}

#[tokio::test]
async fn test_parse_php_multiple_classes_in_brace_delimited_namespaces() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace Foo {\n",
        "    class A {\n",
        "        function doA() {}\n",
        "    }\n",
        "}\n",
        "namespace Bar {\n",
        "    class B {\n",
        "        function doB() {}\n",
        "    }\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 2, "Should find classes in both namespaces");
    assert_eq!(classes[0].name, "A");
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(classes[0].methods[0].name, "doA");
    assert_eq!(classes[1].name, "B");
    assert_eq!(classes[1].methods.len(), 1);
    assert_eq!(classes[1].methods[0].name, "doB");
}

#[tokio::test]
async fn test_parse_php_static_method() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public static function create(string $type): self {}\n",
        "    public function build(): void {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].methods.len(), 2);

    let create = &classes[0].methods[0];
    assert_eq!(create.name, "create");
    assert!(create.is_static, "create should be static");
    assert_eq!(create.parameters.len(), 1);
    assert_eq!(create.parameters[0].name, "$type");

    let build = &classes[0].methods[1];
    assert_eq!(build.name, "build");
    assert!(!build.is_static, "build should not be static");
}

#[tokio::test]
async fn test_parse_php_extracts_constants() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Config {\n",
        "    const VERSION = '1.0';\n",
        "    const int MAX_RETRIES = 3;\n",
        "    public string $name;\n",
        "    public function getName(): string {}\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].constants.len(), 2);

    let version = &classes[0].constants[0];
    assert_eq!(version.name, "VERSION");
    assert!(version.type_hint.is_none(), "VERSION has no type hint");

    let max_retries = &classes[0].constants[1];
    assert_eq!(max_retries.name, "MAX_RETRIES");
    assert_eq!(
        max_retries.type_hint_str().as_deref(),
        Some("int"),
        "MAX_RETRIES should have int type hint"
    );
}

#[tokio::test]
async fn test_parse_php_extracts_multiple_constants_in_one_declaration() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1, INACTIVE = 0;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].constants.len(), 2);
    assert_eq!(classes[0].constants[0].name, "ACTIVE");
    assert_eq!(classes[0].constants[1].name, "INACTIVE");
}

#[tokio::test]
async fn test_parse_php_extracts_parent_class() {
    let backend = create_test_backend();
    let classes = backend.parse_php(concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function breathe(): void {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function bark(): void {}\n",
        "}\n",
    ));

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, "Animal");
    assert!(classes[0].parent_class.is_none());
    assert_eq!(classes[1].name, "Dog");
    assert_eq!(classes[1].parent_class.as_deref(), Some("Animal"));
}

#[tokio::test]
async fn test_parse_php_extracts_visibility() {
    let backend = create_test_backend();
    let classes = backend.parse_php(concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function pubMethod(): void {}\n",
        "    protected function protMethod(): void {}\n",
        "    private function privMethod(): void {}\n",
        "    function defaultMethod(): void {}\n",
        "    public string $pubProp;\n",
        "    protected string $protProp;\n",
        "    private string $privProp;\n",
        "    public const PUB_CONST = 1;\n",
        "    protected const PROT_CONST = 2;\n",
        "    private const PRIV_CONST = 3;\n",
        "    const DEFAULT_CONST = 4;\n",
        "}\n",
    ));

    assert_eq!(classes.len(), 1);
    let cls = &classes[0];

    // Methods
    let pub_m = cls.methods.iter().find(|m| m.name == "pubMethod").unwrap();
    assert_eq!(pub_m.visibility, Visibility::Public);
    let prot_m = cls.methods.iter().find(|m| m.name == "protMethod").unwrap();
    assert_eq!(prot_m.visibility, Visibility::Protected);
    let priv_m = cls.methods.iter().find(|m| m.name == "privMethod").unwrap();
    assert_eq!(priv_m.visibility, Visibility::Private);
    let def_m = cls
        .methods
        .iter()
        .find(|m| m.name == "defaultMethod")
        .unwrap();
    assert_eq!(
        def_m.visibility,
        Visibility::Public,
        "No modifier defaults to public"
    );

    // Properties
    let pub_p = cls.properties.iter().find(|p| p.name == "pubProp").unwrap();
    assert_eq!(pub_p.visibility, Visibility::Public);
    let prot_p = cls
        .properties
        .iter()
        .find(|p| p.name == "protProp")
        .unwrap();
    assert_eq!(prot_p.visibility, Visibility::Protected);
    let priv_p = cls
        .properties
        .iter()
        .find(|p| p.name == "privProp")
        .unwrap();
    assert_eq!(priv_p.visibility, Visibility::Private);

    // Constants
    let pub_c = cls
        .constants
        .iter()
        .find(|c| c.name == "PUB_CONST")
        .unwrap();
    assert_eq!(pub_c.visibility, Visibility::Public);
    let prot_c = cls
        .constants
        .iter()
        .find(|c| c.name == "PROT_CONST")
        .unwrap();
    assert_eq!(prot_c.visibility, Visibility::Protected);
    let priv_c = cls
        .constants
        .iter()
        .find(|c| c.name == "PRIV_CONST")
        .unwrap();
    assert_eq!(priv_c.visibility, Visibility::Private);
    let def_c = cls
        .constants
        .iter()
        .find(|c| c.name == "DEFAULT_CONST")
        .unwrap();
    assert_eq!(
        def_c.visibility,
        Visibility::Public,
        "No modifier defaults to public"
    );
}

// ─── Interface Parsing Tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_parse_php_extracts_interface_methods() {
    let backend = create_test_backend();
    let php = r#"<?php
interface Loggable {
    public function log(string $message): void;
    public function getLogLevel(): int;
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Loggable");
    assert_eq!(classes[0].methods.len(), 2);
    assert_eq!(classes[0].methods[0].name, "log");
    assert_eq!(
        classes[0].methods[0].return_type_str().as_deref(),
        Some("void")
    );
    assert_eq!(classes[0].methods[1].name, "getLogLevel");
    assert_eq!(
        classes[0].methods[1].return_type_str().as_deref(),
        Some("int")
    );
}

#[tokio::test]
async fn test_parse_php_extracts_interface_constants() {
    let backend = create_test_backend();
    let php = r#"<?php
interface HasStatus {
    const STATUS_ACTIVE = 1;
    const STATUS_INACTIVE = 0;
    public function getStatus(): int;
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "HasStatus");
    assert_eq!(classes[0].constants.len(), 2);
    assert_eq!(classes[0].constants[0].name, "STATUS_ACTIVE");
    assert_eq!(classes[0].constants[1].name, "STATUS_INACTIVE");
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(classes[0].methods[0].name, "getStatus");
}

#[tokio::test]
async fn test_parse_php_interface_extends() {
    let backend = create_test_backend();
    let php = r#"<?php
interface Readable {
    public function read(): string;
}
interface Writable extends Readable {
    public function write(string $data): void;
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 2);

    let readable = classes.iter().find(|c| c.name == "Readable").unwrap();
    assert!(readable.parent_class.is_none());
    assert_eq!(readable.methods.len(), 1);

    let writable = classes.iter().find(|c| c.name == "Writable").unwrap();
    assert_eq!(writable.parent_class.as_deref(), Some("Readable"));
    assert_eq!(writable.methods.len(), 1);
    assert_eq!(writable.methods[0].name, "write");
}

#[tokio::test]
async fn test_parse_php_interface_inside_namespace() {
    let backend = create_test_backend();
    let php = r#"<?php
namespace App\Contracts;

interface Repository {
    public function find(int $id): mixed;
    public function save(object $entity): void;
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Repository");
    assert_eq!(classes[0].methods.len(), 2);
    assert_eq!(classes[0].methods[0].name, "find");
    assert_eq!(classes[0].methods[1].name, "save");
}

#[tokio::test]
async fn test_parse_php_class_and_interface_together() {
    let backend = create_test_backend();
    let php = r#"<?php
interface Cacheable {
    public function getCacheKey(): string;
    const TTL = 3600;
}

class UserRepository implements Cacheable {
    public function getCacheKey(): string { return 'users'; }
    public function findAll(): array { return []; }
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 2);

    let iface = classes.iter().find(|c| c.name == "Cacheable").unwrap();
    assert_eq!(iface.methods.len(), 1);
    assert_eq!(iface.constants.len(), 1);
    assert_eq!(iface.constants[0].name, "TTL");

    let class = classes.iter().find(|c| c.name == "UserRepository").unwrap();
    assert_eq!(class.methods.len(), 2);
}

#[tokio::test]
async fn test_parse_php_interface_static_method() {
    let backend = create_test_backend();
    let php = r#"<?php
interface Factory {
    public static function create(): static;
    public function build(): object;
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Factory");
    assert_eq!(classes[0].methods.len(), 2);

    let create = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "create")
        .unwrap();
    assert!(create.is_static);
    assert_eq!(create.return_type_str().as_deref(), Some("static"));

    let build = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "build")
        .unwrap();
    assert!(!build.is_static);
}

// ─── Promoted Property Tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_parse_php_promoted_properties_basic() {
    let backend = create_test_backend();
    let php = r#"<?php
class Service {
    public function __construct(
        private IShoppingCart $cart,
        protected Logger $logger,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    assert_eq!(
        cls.properties.len(),
        2,
        "Should extract 2 promoted properties"
    );

    let cart = cls.properties.iter().find(|p| p.name == "cart").unwrap();
    assert_eq!(cart.type_hint_str().as_deref(), Some("IShoppingCart"));
    assert_eq!(cart.visibility, Visibility::Private);
    assert!(!cart.is_static);

    let logger = cls.properties.iter().find(|p| p.name == "logger").unwrap();
    assert_eq!(logger.type_hint_str().as_deref(), Some("Logger"));
    assert_eq!(logger.visibility, Visibility::Protected);
    assert!(!logger.is_static);
}

#[tokio::test]
async fn test_parse_php_promoted_properties_mixed_with_regular() {
    let backend = create_test_backend();
    let php = r#"<?php
class ShoppingCartService {
    private IShoppingCart $regular;

    public function __construct(
        private IShoppingCart $promoted,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    assert_eq!(
        cls.properties.len(),
        2,
        "Should have regular + promoted property"
    );

    let regular = cls.properties.iter().find(|p| p.name == "regular").unwrap();
    assert_eq!(regular.type_hint_str().as_deref(), Some("IShoppingCart"));
    assert_eq!(regular.visibility, Visibility::Private);

    let promoted = cls
        .properties
        .iter()
        .find(|p| p.name == "promoted")
        .unwrap();
    assert_eq!(promoted.type_hint_str().as_deref(), Some("IShoppingCart"));
    assert_eq!(promoted.visibility, Visibility::Private);
}

#[tokio::test]
async fn test_parse_php_promoted_property_public_visibility() {
    let backend = create_test_backend();
    let php = r#"<?php
class Config {
    public function __construct(
        public string $name,
        public int $value,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    assert_eq!(cls.properties.len(), 2);

    for prop in &cls.properties {
        assert_eq!(prop.visibility, Visibility::Public);
    }

    let name = cls.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(name.type_hint_str().as_deref(), Some("string"));

    let value = cls.properties.iter().find(|p| p.name == "value").unwrap();
    assert_eq!(value.type_hint_str().as_deref(), Some("int"));
}

#[tokio::test]
async fn test_parse_php_non_promoted_constructor_params_ignored() {
    let backend = create_test_backend();
    let php = r#"<?php
class Service {
    public function __construct(
        private string $promoted,
        string $regularParam,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    assert_eq!(
        cls.properties.len(),
        1,
        "Only promoted params (with visibility) should become properties"
    );
    assert_eq!(cls.properties[0].name, "promoted");
}

#[tokio::test]
async fn test_parse_php_promoted_property_readonly() {
    let backend = create_test_backend();
    let php = r#"<?php
class User {
    public function __construct(
        public readonly string $name,
        private readonly int $id,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    assert_eq!(
        cls.properties.len(),
        2,
        "readonly promoted params are still promoted"
    );

    let name = cls.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(name.visibility, Visibility::Public);
    assert_eq!(name.type_hint_str().as_deref(), Some("string"));

    let id = cls.properties.iter().find(|p| p.name == "id").unwrap();
    assert_eq!(id.visibility, Visibility::Private);
    assert_eq!(id.type_hint_str().as_deref(), Some("int"));
}

// ─── Promoted Property @param Override Tests ────────────────────────────────

/// When a constructor docblock has `@param list<User> $users` and the native
/// hint is `array`, the promoted property should get `list<User>` as its type.
#[tokio::test]
async fn test_parse_promoted_property_param_docblock_override() {
    let backend = create_test_backend();
    let php = r#"<?php
class UserService {
    /**
     * @param list<User> $users
     * @param string $name
     */
    public function __construct(
        public array $users,
        public string $name,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    let users = cls.properties.iter().find(|p| p.name == "users").unwrap();
    assert_eq!(
        users.type_hint_str().as_deref(),
        Some("list<User>"),
        "@param list<User> should override native `array` for promoted property"
    );

    // `string` is a scalar — @param string should NOT override to a class name.
    // Both native and docblock agree, so the result stays `string`.
    let name = cls.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(
        name.type_hint_str().as_deref(),
        Some("string"),
        "Scalar @param string should keep native `string`"
    );
}

/// When the docblock provides a class type but the native hint is also a class,
/// the docblock should win (more specific).
#[tokio::test]
async fn test_parse_promoted_property_param_class_override() {
    let backend = create_test_backend();
    let php = r#"<?php
class Repository {
    /**
     * @param UserCollection $items
     */
    public function __construct(
        public object $items,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    let items = cls.properties.iter().find(|p| p.name == "items").unwrap();
    assert_eq!(
        items.type_hint_str().as_deref(),
        Some("UserCollection"),
        "@param UserCollection should override native `object` for promoted property"
    );
}

/// Without a docblock, promoted property should keep its native type as before.
#[tokio::test]
async fn test_parse_promoted_property_no_docblock_unchanged() {
    let backend = create_test_backend();
    let php = r#"<?php
class Service {
    public function __construct(
        public array $items,
        private string $name,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    let items = cls.properties.iter().find(|p| p.name == "items").unwrap();
    assert_eq!(items.type_hint_str().as_deref(), Some("array"));

    let name = cls.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(name.type_hint_str().as_deref(), Some("string"));
}

/// When the docblock has a `@param` for a non-promoted parameter, it should
/// not affect promoted properties that don't have their own `@param`.
#[tokio::test]
async fn test_parse_promoted_property_param_only_matching() {
    let backend = create_test_backend();
    let php = r#"<?php
class Service {
    /**
     * @param LoggerInterface $logger
     */
    public function __construct(
        public LoggerInterface $logger,
        public array $data,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    // $logger has matching @param — both agree on LoggerInterface
    let logger = cls.properties.iter().find(|p| p.name == "logger").unwrap();
    assert_eq!(logger.type_hint_str().as_deref(), Some("LoggerInterface"));

    // $data has no @param — should keep native `array`
    let data = cls.properties.iter().find(|p| p.name == "data").unwrap();
    assert_eq!(data.type_hint_str().as_deref(), Some("array"));
}

/// When a native hint is `int` (scalar) and @param says `UserId` (class),
/// `resolve_effective_type` should keep the native `int` because scalar
/// should not be overridden by a class name.
#[tokio::test]
async fn test_parse_promoted_property_param_scalar_not_overridden_by_class() {
    let backend = create_test_backend();
    let php = r#"<?php
class Service {
    /**
     * @param UserId $id
     */
    public function __construct(
        public int $id,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    let id = cls.properties.iter().find(|p| p.name == "id").unwrap();
    assert_eq!(
        id.type_hint_str().as_deref(),
        Some("int"),
        "Native scalar `int` should not be overridden by docblock class `UserId`"
    );
}

/// Generic Collection type in @param should override a plain `object` native hint.
#[tokio::test]
async fn test_parse_promoted_property_param_generic_override() {
    let backend = create_test_backend();
    let php = r#"<?php
class OrderService {
    /**
     * @param Collection<int, Order> $orders
     * @param array<string, mixed> $config
     */
    public function __construct(
        public object $orders,
        public array $config,
    ) {}
}
"#;

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let cls = &classes[0];
    let orders = cls.properties.iter().find(|p| p.name == "orders").unwrap();
    assert_eq!(
        orders.type_hint_str().as_deref(),
        Some("Collection<int, Order>"),
        "@param Collection<int, Order> should override native `object`"
    );

    // array<string, mixed> — although the base is `array` (scalar), the
    // generic parameters carry useful type info for destructuring and
    // foreach, so resolve_effective_type now keeps the docblock type.
    let config = cls.properties.iter().find(|p| p.name == "config").unwrap();
    assert_eq!(
        config.type_hint_str().as_deref(),
        Some("array<string, mixed>"),
        "Docblock `array<string, mixed>` should override native `array` (generic params preserved)"
    );
}

// ─── Standalone Function Parsing Tests ──────────────────────────────────────

#[tokio::test]
async fn test_parse_functions_standalone() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "function hello(): void {}\n",
        "function add(int $a, int $b): int { return $a + $b; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 2, "Should extract 2 standalone functions");

    let hello = functions.iter().find(|f| f.name == "hello").unwrap();
    assert!(hello.parameters.is_empty());
    assert_eq!(hello.return_type_str().as_deref(), Some("void"));
    assert!(hello.namespace.is_none());

    let add = functions.iter().find(|f| f.name == "add").unwrap();
    assert_eq!(add.parameters.len(), 2);
    assert_eq!(add.parameters[0].name, "$a");
    assert_eq!(add.parameters[0].type_hint_str().as_deref(), Some("int"));
    assert_eq!(add.parameters[1].name, "$b");
    assert_eq!(add.return_type_str().as_deref(), Some("int"));
    assert!(add.namespace.is_none());
}

#[tokio::test]
async fn test_parse_functions_inside_namespace() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace Amp;\n",
        "\n",
        "function delay(float $seconds): void {}\n",
        "function async(callable $callback): void {}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 2, "Should extract 2 namespaced functions");

    let delay = functions.iter().find(|f| f.name == "delay").unwrap();
    assert_eq!(delay.namespace.as_deref(), Some("Amp"));
    assert_eq!(delay.parameters.len(), 1);
    assert_eq!(delay.parameters[0].name, "$seconds");
    assert_eq!(
        delay.parameters[0].type_hint_str().as_deref(),
        Some("float")
    );
    assert_eq!(delay.return_type_str().as_deref(), Some("void"));

    let async_fn = functions.iter().find(|f| f.name == "async").unwrap();
    assert_eq!(async_fn.namespace.as_deref(), Some("Amp"));
}

#[tokio::test]
async fn test_parse_functions_ignores_class_methods() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "function standalone(): void {}\n",
        "class Service {\n",
        "    public function handle(): void {}\n",
        "}\n",
        "function another(): string { return ''; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(
        functions.len(),
        2,
        "Should only extract standalone functions, not class methods"
    );
    assert!(functions.iter().any(|f| f.name == "standalone"));
    assert!(functions.iter().any(|f| f.name == "another"));
    assert!(
        !functions.iter().any(|f| f.name == "handle"),
        "Class methods should not appear"
    );
}

#[tokio::test]
async fn test_parse_functions_no_return_type() {
    let backend = create_test_backend();
    let php = "<?php\nfunction legacy($x, $y) { return $x + $y; }\n";

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 1);

    let f = &functions[0];
    assert_eq!(f.name, "legacy");
    assert!(f.return_type.is_none(), "No return type hint");
    assert_eq!(f.parameters.len(), 2);
    assert!(f.parameters[0].type_hint.is_none());
}

#[tokio::test]
async fn test_parse_functions_nullable_and_union_types() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "function maybe(?string $val): ?int { return null; }\n",
        "function either(string|int $val): string|false { return ''; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 2);

    let maybe = functions.iter().find(|f| f.name == "maybe").unwrap();
    assert_eq!(
        maybe.parameters[0].type_hint_str().as_deref(),
        Some("?string")
    );
    assert_eq!(maybe.return_type_str().as_deref(), Some("?int"));

    let either = functions.iter().find(|f| f.name == "either").unwrap();
    assert_eq!(
        either.parameters[0].type_hint_str().as_deref(),
        Some("string|int")
    );
    assert_eq!(either.return_type_str().as_deref(), Some("string|false"));
}

#[tokio::test]
async fn test_parse_functions_variadic_and_reference() {
    let backend = create_test_backend();
    let php = "<?php\nfunction gather(string ...$items): array { return $items; }\nfunction swap(int &$a, int &$b): void {}\n";

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 2);

    let gather = functions.iter().find(|f| f.name == "gather").unwrap();
    assert_eq!(gather.parameters.len(), 1);
    assert!(gather.parameters[0].is_variadic);
    assert!(!gather.parameters[0].is_reference);
    assert!(!gather.parameters[0].is_required);

    let swap = functions.iter().find(|f| f.name == "swap").unwrap();
    assert_eq!(swap.parameters.len(), 2);
    assert!(swap.parameters[0].is_reference);
    assert!(!swap.parameters[0].is_variadic);
}

#[tokio::test]
async fn test_parse_functions_brace_delimited_namespace() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace Foo\\Bar {\n",
        "    function helper(): void {}\n",
        "}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "helper");
    assert_eq!(functions[0].namespace.as_deref(), Some("Foo\\Bar"));
}

#[tokio::test]
async fn test_parse_functions_empty_file() {
    let backend = create_test_backend();
    let php = "<?php\n// nothing here\n";

    let functions = backend.parse_functions(php);
    assert!(functions.is_empty(), "No functions in an empty file");
}

// ─── Functions inside if-guards ─────────────────────────────────────────────

#[tokio::test]
async fn test_parse_functions_inside_function_exists_guard() {
    let backend = create_test_backend();
    // This is the exact pattern used by Laravel helpers.php and many other
    // PHP libraries: functions wrapped in `if (! function_exists(...))`.
    let php = concat!(
        "<?php\n",
        "\n",
        "if (! function_exists('session')) {\n",
        "    /**\n",
        "     * Get / set the specified session value.\n",
        "     */\n",
        "    function session($key = null, $default = null)\n",
        "    {\n",
        "        if (is_null($key)) {\n",
        "            return app('session');\n",
        "        }\n",
        "        return app('session')->get($key, $default);\n",
        "    }\n",
        "}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(
        functions.len(),
        1,
        "Should find function inside if-guard block"
    );
    assert_eq!(functions[0].name, "session");
    assert_eq!(functions[0].parameters.len(), 2);
    assert!(
        functions[0].return_type.is_none(),
        "session() has no return type hint"
    );
}

#[tokio::test]
async fn test_parse_functions_multiple_function_exists_guards() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "\n",
        "if (! function_exists('app')) {\n",
        "    function app(?string $abstract = null, array $parameters = []): mixed\n",
        "    {\n",
        "        return Container::getInstance();\n",
        "    }\n",
        "}\n",
        "\n",
        "if (! function_exists('session')) {\n",
        "    function session($key = null, $default = null)\n",
        "    {\n",
        "        return app('session');\n",
        "    }\n",
        "}\n",
        "\n",
        "if (! function_exists('config')) {\n",
        "    function config($key = null, $default = null): mixed\n",
        "    {\n",
        "        return null;\n",
        "    }\n",
        "}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(
        functions.len(),
        3,
        "Should find all 3 functions inside separate if-guards"
    );

    let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"app"), "Should find app()");
    assert!(names.contains(&"session"), "Should find session()");
    assert!(names.contains(&"config"), "Should find config()");

    let app = functions.iter().find(|f| f.name == "app").unwrap();
    assert_eq!(app.return_type_str().as_deref(), Some("mixed"));

    let config = functions.iter().find(|f| f.name == "config").unwrap();
    assert_eq!(config.return_type_str().as_deref(), Some("mixed"));
}

#[tokio::test]
async fn test_parse_functions_inside_namespace_with_function_exists_guard() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace Illuminate\\Support;\n",
        "\n",
        "if (! function_exists('Illuminate\\Support\\enum_value')) {\n",
        "    function enum_value($value): mixed\n",
        "    {\n",
        "        return $value;\n",
        "    }\n",
        "}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(
        functions.len(),
        1,
        "Should find function inside if-guard within namespace"
    );
    assert_eq!(functions[0].name, "enum_value");
    assert_eq!(
        functions[0].namespace.as_deref(),
        Some("Illuminate\\Support"),
        "Should preserve namespace context"
    );
    assert_eq!(functions[0].return_type_str().as_deref(), Some("mixed"));
}

#[tokio::test]
async fn test_parse_functions_mixed_guarded_and_unguarded() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "\n",
        "function always_defined(): void {}\n",
        "\n",
        "if (! function_exists('maybe_defined')) {\n",
        "    function maybe_defined(): string { return ''; }\n",
        "}\n",
        "\n",
        "function also_always(): int { return 0; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(
        functions.len(),
        3,
        "Should find both guarded and unguarded functions"
    );

    let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"always_defined"));
    assert!(names.contains(&"maybe_defined"));
    assert!(names.contains(&"also_always"));
}

// ─── Enum Parsing ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_parse_php_extracts_backed_enum_cases_as_constants() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum CustomerAvailabilityStatus: int\n",
        "{\n",
        "    case CUSTOMER_NOT_IN_AUDIENCE = -1;\n",
        "    case AVAILABLE_TO_CUSTOMER = 0;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1, "Should parse the enum as a class-like");
    assert_eq!(classes[0].name, "CustomerAvailabilityStatus");
    assert_eq!(
        classes[0].constants.len(),
        2,
        "Enum cases should be extracted as constants"
    );

    let case0 = &classes[0].constants[0];
    assert_eq!(case0.name, "CUSTOMER_NOT_IN_AUDIENCE");
    assert_eq!(
        case0.visibility,
        phpantom_lsp::types::Visibility::Public,
        "Enum cases are always public"
    );

    let case1 = &classes[0].constants[1];
    assert_eq!(case1.name, "AVAILABLE_TO_CUSTOMER");
}

#[tokio::test]
async fn test_parse_php_extracts_unit_enum_cases() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Color\n",
        "{\n",
        "    case Red;\n",
        "    case Green;\n",
        "    case Blue;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Color");
    assert_eq!(
        classes[0].constants.len(),
        3,
        "Unit enum cases should be extracted as constants"
    );

    let names: Vec<&str> = classes[0]
        .constants
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["Red", "Green", "Blue"]);
}

#[tokio::test]
async fn test_parse_php_extracts_enum_with_methods() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Suit: string\n",
        "{\n",
        "    case Hearts = 'H';\n",
        "    case Diamonds = 'D';\n",
        "    case Clubs = 'C';\n",
        "    case Spades = 'S';\n",
        "\n",
        "    public function color(): string\n",
        "    {\n",
        "        return match($this) {\n",
        "            Suit::Hearts, Suit::Diamonds => 'red',\n",
        "            Suit::Clubs, Suit::Spades => 'black',\n",
        "        };\n",
        "    }\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Suit");
    assert_eq!(classes[0].constants.len(), 4, "Should have 4 enum cases");
    assert_eq!(classes[0].methods.len(), 1, "Should have 1 method");
    assert_eq!(classes[0].methods[0].name, "color");
    assert_eq!(
        classes[0].methods[0].return_type_str().as_deref(),
        Some("string")
    );
}

#[tokio::test]
async fn test_parse_php_extracts_enum_with_constants_and_cases() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Status: int\n",
        "{\n",
        "    const DEFAULT_STATUS = 0;\n",
        "    case Active = 1;\n",
        "    case Inactive = 2;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Status");
    // Both the `const` and the `case` entries should appear as constants.
    assert_eq!(
        classes[0].constants.len(),
        3,
        "Should have 1 real constant + 2 enum cases"
    );

    let names: Vec<&str> = classes[0]
        .constants
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(names.contains(&"DEFAULT_STATUS"));
    assert!(names.contains(&"Active"));
    assert!(names.contains(&"Inactive"));
}

#[tokio::test]
async fn test_parse_php_extracts_enum_inside_namespace() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "enum Direction\n",
        "{\n",
        "    case Up;\n",
        "    case Down;\n",
        "    case Left;\n",
        "    case Right;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Direction");
    assert_eq!(classes[0].constants.len(), 4);
}

// ─── Implicit UnitEnum / BackedEnum interface tests ─────────────────────────

/// A unit enum (no backing type) should have `UnitEnum` in its `used_traits`.
#[tokio::test]
async fn test_parse_php_unit_enum_has_implicit_unit_enum_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Color\n",
        "{\n",
        "    case Red;\n",
        "    case Green;\n",
        "    case Blue;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Color");
    // parse_php returns raw names before resolution; the leading backslash
    // marks the name as fully-qualified so resolve_name won't prepend a
    // namespace later.
    assert!(
        classes[0].used_traits.iter().any(|t| t == "\\UnitEnum"),
        "Unit enum should implicitly implement \\UnitEnum, got used_traits: {:?}",
        classes[0].used_traits
    );
    assert!(
        !classes[0].used_traits.iter().any(|t| t == "\\BackedEnum"),
        "Unit enum should NOT implement \\BackedEnum, got used_traits: {:?}",
        classes[0].used_traits
    );
}

/// A backed enum (`: int`) should have `BackedEnum` in its `used_traits`.
#[tokio::test]
async fn test_parse_php_backed_int_enum_has_implicit_backed_enum_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 0;\n",
        "    case High = 1;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Priority");
    assert!(
        classes[0].used_traits.iter().any(|t| t == "\\BackedEnum"),
        "Backed int enum should implicitly implement \\BackedEnum, got used_traits: {:?}",
        classes[0].used_traits
    );
    assert!(
        !classes[0].used_traits.iter().any(|t| t == "\\UnitEnum"),
        "Backed enum should NOT implement \\UnitEnum, got used_traits: {:?}",
        classes[0].used_traits
    );
}

/// A backed enum (`: string`) should have `BackedEnum` in its `used_traits`.
#[tokio::test]
async fn test_parse_php_backed_string_enum_has_implicit_backed_enum_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "enum Suit: string\n",
        "{\n",
        "    case Hearts = 'H';\n",
        "    case Spades = 'S';\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Suit");
    assert!(
        classes[0].used_traits.iter().any(|t| t == "\\BackedEnum"),
        "Backed string enum should implicitly implement \\BackedEnum, got used_traits: {:?}",
        classes[0].used_traits
    );
}

/// An enum inside a namespace should still have UnitEnum/BackedEnum resolved
/// to the root namespace (not prefixed with the current namespace).
#[tokio::test]
async fn test_parse_php_namespaced_enum_implicit_interface_is_not_namespace_prefixed() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "enum Mode\n",
        "{\n",
        "    case Automatic;\n",
        "    case Manual;\n",
        "}\n",
    );

    let mut classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "Mode");
    // parse_php returns the raw `\UnitEnum` (leading backslash marks it as
    // fully-qualified in the PHP source).
    assert!(
        classes[0].used_traits.iter().any(|t| t == "\\UnitEnum"),
        "Namespaced unit enum should have \\UnitEnum before resolution, got: {:?}",
        classes[0].used_traits
    );

    // Simulate the resolution step that update_ast performs.
    let use_map = std::collections::HashMap::new();
    let namespace = Some("App\\Enums".to_string());
    phpantom_lsp::Backend::resolve_parent_class_names(&mut classes, &use_map, &namespace);

    // After resolution the leading backslash is stripped — the canonical
    // FQN representation never has a leading `\`.  The name must NOT be
    // namespace-prefixed (i.e. must remain `UnitEnum`, not
    // `App\Enums\UnitEnum`).
    assert!(
        classes[0].used_traits.iter().any(|t| t == "UnitEnum"),
        "After resolution, should be bare UnitEnum (canonical FQN), got: {:?}",
        classes[0].used_traits
    );
    assert!(
        !classes[0]
            .used_traits
            .iter()
            .any(|t| t == "App\\Enums\\UnitEnum"),
        "Should NOT be namespace-prefixed as App\\Enums\\UnitEnum, got: {:?}",
        classes[0].used_traits
    );
}

/// An enum that explicitly uses a trait should have both the trait and the
/// implicit interface in `used_traits`.
#[tokio::test]
async fn test_parse_php_enum_with_trait_also_has_implicit_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "trait HasDescription {\n",
        "    public function describe(): string { return 'desc'; }\n",
        "}\n",
        "\n",
        "enum Status: int\n",
        "{\n",
        "    use HasDescription;\n",
        "\n",
        "    case Active = 1;\n",
        "    case Inactive = 0;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    let enum_info = classes.iter().find(|c| c.name == "Status").unwrap();
    assert!(
        enum_info.used_traits.iter().any(|t| t == "HasDescription"),
        "Should include the explicit trait, got: {:?}",
        enum_info.used_traits
    );
    assert!(
        enum_info.used_traits.iter().any(|t| t == "\\BackedEnum"),
        "Should include implicit \\BackedEnum, got: {:?}",
        enum_info.used_traits
    );
}

// ─── parse_defines (AST-based) tests ────────────────────────────────────────

#[tokio::test]
async fn test_parse_defines_single_quoted() {
    let backend = create_test_backend();
    let defines = backend.parse_defines("<?php\ndefine('MY_CONST', 42);\n");
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["MY_CONST"]);
    // The offset should point to the `define` keyword on line 1.
    assert!(defines[0].1 > 0, "define keyword offset should be non-zero");
    // The value should be extracted from the second argument.
    assert_eq!(defines[0].2.as_deref(), Some("42"));
}

#[tokio::test]
async fn test_parse_defines_double_quoted() {
    let backend = create_test_backend();
    let defines = backend.parse_defines("<?php\ndefine(\"MY_CONST\", 'hello');\n");
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["MY_CONST"]);
}

#[tokio::test]
async fn test_parse_defines_multiple() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "define('PHP_EOL', \"\\n\");\n",
        "define('PHP_INT_MAX', 9223372036854775807);\n",
        "define('SORT_ASC', 4);\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["PHP_EOL", "PHP_INT_MAX", "SORT_ASC"]);
}

#[tokio::test]
async fn test_parse_defines_with_third_argument() {
    let backend = create_test_backend();
    let defines = backend.parse_defines("<?php\ndefine('__DIR__', '', true);\n");
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["__DIR__"]);
}

#[tokio::test]
async fn test_parse_defines_skips_non_define_calls() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "some_define('NOT_A_CONST', 1);\n",
        "user_define('ALSO_NOT', 2);\n",
        "define('REAL_CONST', 3);\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["REAL_CONST"]);
}

#[tokio::test]
async fn test_parse_defines_skips_dynamic_names() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "define($varName, 42);\n",
        "define('GOOD_CONST', 1);\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["GOOD_CONST"]);
}

#[tokio::test]
async fn test_parse_defines_empty_file() {
    let backend = create_test_backend();
    let defines = backend.parse_defines("<?php\n");
    assert!(defines.is_empty());
}

#[tokio::test]
async fn test_parse_defines_no_defines() {
    let backend = create_test_backend();
    let defines = backend.parse_defines("<?php\necho 'hello';\nfunction foo() {}\n");
    assert!(defines.is_empty());
}

#[tokio::test]
async fn test_parse_defines_inside_if_guard() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "if (!defined('MY_CONST')) {\n",
        "    define('MY_CONST', 'value');\n",
        "}\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["MY_CONST"]);
}

#[tokio::test]
async fn test_parse_defines_inside_namespace() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "define('APP_VERSION', '2.0');\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["APP_VERSION"]);
}

#[tokio::test]
async fn test_parse_defines_inside_block() {
    let backend = create_test_backend();
    let content = concat!("<?php\n", "{\n", "    define('BLOCK_CONST', 1);\n", "}\n",);
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["BLOCK_CONST"]);
}

#[tokio::test]
async fn test_parse_defines_mixed_with_classes_and_functions() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "define('VERSION', '1.0');\n",
        "class Foo { public function bar() {} }\n",
        "define('DEBUG', true);\n",
        "function helper() {}\n",
        "define('MAX_RETRIES', 3);\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["VERSION", "DEBUG", "MAX_RETRIES"]);
}

#[tokio::test]
async fn test_parse_defines_ignores_method_calls_named_define() {
    let backend = create_test_backend();
    let content = concat!(
        "<?php\n",
        "$obj->define('NOT_A_CONST', 1);\n",
        "define('REAL_CONST', 2);\n",
    );
    let defines = backend.parse_defines(content);
    let names: Vec<&str> = defines.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, vec!["REAL_CONST"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// #[Deprecated] attribute extraction
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_deprecated_attribute_on_class_bare() {
    let backend = create_test_backend();
    let php = concat!("<?php\n", "#[Deprecated]\n", "class OldHelper {}\n",);
    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert!(
        classes[0].deprecation_message.is_some(),
        "Bare #[Deprecated] should set deprecation_message"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_class_with_reason_and_since() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[Deprecated(reason: 'Use NewApi instead', since: '8.2')]\n",
        "class OldApi {}\n",
    );
    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    let msg = classes[0].deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use NewApi instead"),
        "Expected reason in message, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 8.2"),
        "Expected since in message, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_class_positional_reason() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[Deprecated('Use NewHelper instead')]\n",
        "class OldHelper {}\n",
    );
    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    let msg = classes[0].deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use NewHelper instead"),
        "Expected positional reason in message, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_method_with_reason() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    #[Deprecated(reason: 'Use sendAsync() instead', since: '8.1')]\n",
        "    public function sendLegacy(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "sendLegacy")
        .unwrap();
    let msg = method.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use sendAsync() instead"),
        "Expected reason in method deprecation, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 8.1"),
        "Expected since in method deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_method_bare() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    #[Deprecated]\n",
        "    public function sendLegacy(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "sendLegacy")
        .unwrap();
    assert!(
        method.deprecation_message.is_some(),
        "Bare #[Deprecated] on method should set deprecation_message"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_property_with_reason() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Doc {\n",
        "    #[Deprecated('The property is deprecated', since: '8.4')]\n",
        "    public string $encoding = 'UTF-8';\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "encoding")
        .unwrap();
    let msg = prop.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("The property is deprecated"),
        "Expected reason in property deprecation, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 8.4"),
        "Expected since in property deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_property_bare() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Doc {\n",
        "    #[Deprecated]\n",
        "    public string $config = '';\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "config")
        .unwrap();
    assert!(
        prop.deprecation_message.is_some(),
        "Bare #[Deprecated] on property should set deprecation_message"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_constant() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class PDO {\n",
        "    #[Deprecated(reason: 'Use ATTR_EMULATE_PREPARES instead')]\n",
        "    const ATTR_OLD = 1;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let constant = classes[0]
        .constants
        .iter()
        .find(|c| c.name == "ATTR_OLD")
        .unwrap();
    let msg = constant.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use ATTR_EMULATE_PREPARES instead"),
        "Expected reason in constant deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_constant_bare() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Config {\n",
        "    #[Deprecated]\n",
        "    const OLD_MODE = 0;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let constant = classes[0]
        .constants
        .iter()
        .find(|c| c.name == "OLD_MODE")
        .unwrap();
    assert!(
        constant.deprecation_message.is_some(),
        "Bare #[Deprecated] on constant should set deprecation_message"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_function() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[Deprecated(reason: 'Use new_helper() instead', since: '7.4')]\n",
        "function old_helper(): void {}\n",
    );
    let functions = backend.parse_functions(php);
    let func = functions.iter().find(|f| f.name == "old_helper").unwrap();
    let msg = func.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use new_helper() instead"),
        "Expected reason in function deprecation, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 7.4"),
        "Expected since in function deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_function_bare() {
    let backend = create_test_backend();
    let php = concat!("<?php\n", "#[Deprecated]\n", "function old_fn(): void {}\n",);
    let functions = backend.parse_functions(php);
    let func = functions.iter().find(|f| f.name == "old_fn").unwrap();
    assert!(
        func.deprecation_message.is_some(),
        "Bare #[Deprecated] on function should set deprecation_message"
    );
}

#[tokio::test]
async fn test_docblock_deprecated_takes_priority_over_attribute() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    /**\n",
        "     * @deprecated Use sendModern() instead.\n",
        "     */\n",
        "    #[Deprecated(reason: 'Attribute message')]\n",
        "    public function sendLegacy(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "sendLegacy")
        .unwrap();
    let msg = method.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use sendModern() instead"),
        "Docblock @deprecated should take priority, got: {msg}"
    );
    assert!(
        !msg.contains("Attribute message"),
        "Attribute message should not appear when docblock has @deprecated, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_since_only() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Config {\n",
        "    #[Deprecated(since: '7.4')]\n",
        "    const OLD_MODE = 0;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let constant = classes[0]
        .constants
        .iter()
        .find(|c| c.name == "OLD_MODE")
        .unwrap();
    let msg = constant.deprecation_message.as_deref().unwrap();
    assert_eq!(msg, "since PHP 7.4");
}

#[tokio::test]
async fn test_deprecated_attribute_on_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[Deprecated(reason: 'Use NewInterface instead')]\n",
        "interface OldInterface {\n",
        "    public function doThing(): void;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let iface = classes.iter().find(|c| c.name == "OldInterface").unwrap();
    let msg = iface.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use NewInterface instead"),
        "Expected reason in interface deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_on_enum() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[Deprecated(reason: 'Use StatusV2 instead')]\n",
        "enum Status {\n",
        "    case Active;\n",
        "    case Inactive;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let enm = classes.iter().find(|c| c.name == "Status").unwrap();
    let msg = enm.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use StatusV2 instead"),
        "Expected reason in enum deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_non_deprecated_elements_unaffected() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function handle(): void {}\n",
        "    public string $name = '';\n",
        "    const VERSION = 1;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "handle")
        .unwrap();
    assert!(method.deprecation_message.is_none());
    let prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "name")
        .unwrap();
    assert!(prop.deprecation_message.is_none());
    let constant = classes[0]
        .constants
        .iter()
        .find(|c| c.name == "VERSION")
        .unwrap();
    assert!(constant.deprecation_message.is_none());
}

#[tokio::test]
async fn test_deprecated_attribute_native_php84_message_named_arg() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[\\Deprecated(message: 'Use safe_replacement() instead', since: '1.5')]\n",
        "function unsafe_function(): void {}\n",
    );
    let functions = backend.parse_functions(php);
    let func = functions
        .iter()
        .find(|f| f.name == "unsafe_function")
        .unwrap();
    let msg = func.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use safe_replacement() instead"),
        "Expected message in function deprecation, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 1.5"),
        "Expected since in function deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_native_php84_fqn_on_method() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    #[\\Deprecated(message: 'Use processV2() instead', since: '8.4')]\n",
        "    public function process(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "process")
        .unwrap();
    let msg = method.deprecation_message.as_deref().unwrap();
    assert!(
        msg.contains("Use processV2() instead"),
        "Expected message in method deprecation, got: {msg}"
    );
    assert!(
        msg.contains("since PHP 8.4"),
        "Expected since in method deprecation, got: {msg}"
    );
}

#[tokio::test]
async fn test_deprecated_attribute_native_php84_message_only() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Config {\n",
        "    #[\\Deprecated(message: 'Use NEW_LIMIT instead')]\n",
        "    const OLD_LIMIT = 100;\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let constant = classes[0]
        .constants
        .iter()
        .find(|c| c.name == "OLD_LIMIT")
        .unwrap();
    let msg = constant.deprecation_message.as_deref().unwrap();
    assert_eq!(msg, "Use NEW_LIMIT instead");
}

#[tokio::test]
async fn test_deprecated_attribute_fqn_without_backslash_prefix() {
    // JetBrains stubs use `#[Deprecated]` (short name via use import).
    // Native PHP 8.4 uses `#[\Deprecated]` (FQN with leading backslash).
    // Both should work identically.
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Demo {\n",
        "    #[Deprecated(reason: 'JetBrains style')]\n",
        "    public function jetbrainsStyle(): void {}\n",
        "\n",
        "    #[\\Deprecated(message: 'Native PHP style')]\n",
        "    public function nativeStyle(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let jb = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "jetbrainsStyle")
        .unwrap();
    assert!(
        jb.deprecation_message
            .as_deref()
            .unwrap()
            .contains("JetBrains style"),
        "JetBrains-style #[Deprecated(reason:)] should work"
    );
    let native = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "nativeStyle")
        .unwrap();
    assert!(
        native
            .deprecation_message
            .as_deref()
            .unwrap()
            .contains("Native PHP style"),
        "Native-style #[\\Deprecated(message:)] should work"
    );
}

#[tokio::test]
async fn test_custom_namespaced_deprecated_attribute_does_not_trigger() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "#[\\Test\\Deprecated(reason: 'Not a real deprecation')]\n",
        "function still_fine(): void {}\n",
    );
    let functions = backend.parse_functions(php);
    let func = functions.iter().find(|f| f.name == "still_fine").unwrap();
    assert!(
        func.deprecation_message.is_none(),
        "#[\\Test\\Deprecated] should NOT trigger deprecation, got: {:?}",
        func.deprecation_message
    );
}

#[tokio::test]
async fn test_custom_namespaced_deprecated_attribute_on_method_does_not_trigger() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    #[\\App\\Attributes\\Deprecated(reason: 'Custom attribute')]\n",
        "    public function process(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "process")
        .unwrap();
    assert!(
        method.deprecation_message.is_none(),
        "#[\\App\\Attributes\\Deprecated] should NOT trigger deprecation, got: {:?}",
        method.deprecation_message
    );
}

#[tokio::test]
async fn test_custom_namespaced_deprecated_attribute_on_class_does_not_trigger() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "#[\\Vendor\\Deprecated]\n",
        "class OldService {}\n",
    );
    let classes = backend.parse_php(php);
    assert!(
        classes[0].deprecation_message.is_none(),
        "#[\\Vendor\\Deprecated] should NOT trigger class deprecation, got: {:?}",
        classes[0].deprecation_message
    );
}

#[tokio::test]
async fn test_legitimate_deprecated_attributes_still_work() {
    let backend = create_test_backend();
    // Verify \Deprecated and \JetBrains\PhpStorm\Deprecated still match.
    let php = concat!(
        "<?php\n",
        "class Demo {\n",
        "    #[\\Deprecated(message: 'native')]\n",
        "    public function nativeAttr(): void {}\n",
        "\n",
        "    #[\\JetBrains\\PhpStorm\\Deprecated(reason: 'jetbrains')]\n",
        "    public function jbAttr(): void {}\n",
        "}\n",
    );
    let classes = backend.parse_php(php);
    let native = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "nativeAttr")
        .unwrap();
    assert!(
        native.deprecation_message.is_some(),
        "#[\\Deprecated] should trigger deprecation"
    );
    let jb = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "jbAttr")
        .unwrap();
    assert!(
        jb.deprecation_message.is_some(),
        "#[\\JetBrains\\PhpStorm\\Deprecated] should trigger deprecation"
    );
}
