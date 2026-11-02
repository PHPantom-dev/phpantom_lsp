//! Validation strings use the installed rules and the types of their owners.

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

use crate::common::{
    apply_edits, complete_at_opened, create_psr4_workspace, definition_locations,
    goto_definition_at, hover_text_at, item_labels, open_php_at, position_of,
    slow_diagnostic_messages,
};

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^13.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\": "vendor/illuminate/",
            "Livewire\\": "vendor/livewire/"
        }
    }
}"#;

const ATTRIBUTES: &str = r#"<?php
namespace Illuminate\Validation\Concerns;
trait ValidatesAttributes {
    /** Require a value to be present. */
    public function validateRequired($attribute, $value): bool { return true; }
    /** Validate a fictional cake supported by this framework version. */
    public function validateFictionalCake($attribute, $value): bool { return true; }
    public function validateString($attribute, $value): bool { return true; }
    public function validateInteger($attribute, $value): bool { return true; }
    public function validateBoolean($attribute, $value): bool { return true; }
    public function validateNullable(): bool { return true; }
    public function validateSometimes(): bool { return true; }
    public function validateEmail($attribute, $value, $parameters = []): bool { return true; }
    /** Limit the maximum size. */
    public function validateMax($attribute, $value, $parameters): bool { return true; }
    public function validateMin($attribute, $value, $parameters): bool { return true; }
    public function validateBetween($attribute, $value, $parameters): bool { return true; }
    public function validateSame($attribute, $value, $parameters): bool { return true; }
    public function validateDifferent($attribute, $value, $parameters): bool { return true; }
    public function validateRequiredWith($attribute, $value, $parameters): bool { return true; }
    public function validateGt($attribute, $value, $parameters): bool { return true; }
    public function validateAfter($attribute, $value, $parameters): bool { return true; }
    public function validateExists($attribute, $value, $parameters): bool { return true; }
    public function validateUnique($attribute, $value, $parameters): bool { return true; }
    public function validateIn($attribute, $value, $parameters): bool { return true; }
    public function validateRegex($attribute, $value, $parameters): bool { return true; }
    public function validateNotRegex($attribute, $value, $parameters): bool { return true; }
    protected function validateInternal($attribute, $value): bool { return true; }
}
"#;

const VALIDATOR: &str = r#"<?php
namespace Illuminate\Validation;
class Validator implements \Illuminate\Contracts\Validation\Validator {
    use Concerns\ValidatesAttributes;
    protected $dependentRules = ['Same', 'Different', 'RequiredWith', 'Gt', 'After'];
    public function sometimes($attribute, $rules, callable $callback): static { return $this; }
    public function validate(): array { return []; }
    protected function validateAttribute($attribute, $rule): void {}
}
"#;

const REQUEST: &str = r#"<?php
namespace Illuminate\Http;
class Request {
    public function validate(array $rules): array { return []; }
    public function validateWithBag(string $errorBag, array $rules): array { return []; }
    public function all(): array { return []; }
}
"#;

const FORM_REQUEST: &str = r#"<?php
namespace Illuminate\Foundation\Http;
class FormRequest extends \Illuminate\Http\Request {
    public function rules(): array { return []; }
    public function messages(): array { return []; }
    public function attributes(): array { return []; }
}
"#;

const FACADE: &str = r#"<?php
namespace Illuminate\Support\Facades;
class Validator {
    public static function make(array $data, array $rules): \Illuminate\Validation\Validator {
        return new \Illuminate\Validation\Validator();
    }
    public static function extend(string $rule, $extension): void {}
    public static function extendImplicit(string $rule, $extension): void {}
    public static function extendDependent(string $rule, $extension): void {}
    public static function resolver(\Closure $resolver): void {}
}
"#;

const CUSTOMER: &str = r#"<?php
namespace App\Models;
class Customer extends \Illuminate\Database\Eloquent\Model {
    protected $table = 'customers';
    protected $fillable = ['email', 'status'];
    protected $casts = ['status' => Status::class];
}
"#;

const STATUS: &str = r#"<?php
namespace App\Models;
enum Status: string {
    case Pending = 'pending';
    case Active = 'active';
}
"#;

const CUSTOM_RULE: &str = r#"<?php
namespace App\Rules;
/** Require the application's uppercase format. */
class UppercaseRule implements \Illuminate\Contracts\Validation\ValidationRule {
    public function validate(string $attribute, mixed $value, \Closure $fail): void {}
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Http/Request.php", REQUEST),
        (
            "vendor/illuminate/Foundation/Http/FormRequest.php",
            FORM_REQUEST,
        ),
        (
            "vendor/illuminate/Validation/Concerns/ValidatesAttributes.php",
            ATTRIBUTES,
        ),
        ("vendor/illuminate/Validation/Validator.php", VALIDATOR),
        ("vendor/illuminate/Support/Facades/Validator.php", FACADE),
        (
            "vendor/illuminate/Contracts/Validation/Factory.php",
            "<?php namespace Illuminate\\Contracts\\Validation; interface Factory { public function extend($rule, $extension, $message = null): void; public function extendImplicit($rule, $extension, $message = null): void; }",
        ),
        (
            "vendor/illuminate/Validation/Factory.php",
            "<?php namespace Illuminate\\Validation; class Factory implements \\Illuminate\\Contracts\\Validation\\Factory { public function extend($rule, $extension, $message = null): void {} public function extendImplicit($rule, $extension, $message = null): void {} public function extendDependent($rule, $extension, $message = null): void {} public function resolver(\\Closure $resolver): void {} }",
        ),
        (
            "vendor/illuminate/Contracts/Validation/Validator.php",
            "<?php namespace Illuminate\\Contracts\\Validation; interface Validator {}",
        ),
        (
            "vendor/illuminate/Contracts/Validation/ValidationRule.php",
            "<?php namespace Illuminate\\Contracts\\Validation; interface ValidationRule { public function validate(string $attribute, mixed $value, \\Closure $fail): void; }",
        ),
        (
            "vendor/illuminate/Validation/ValidationRuleParser.php",
            r#"<?php namespace Illuminate\Validation;
class ValidationRuleParser {
    protected static function normalizeRule(string $rule): string {
        return match ($rule) { 'Int' => 'Integer', 'Bool' => 'Boolean', default => $rule };
    }
}"#,
        ),
        (
            "vendor/illuminate/Validation/Rule.php",
            "<?php namespace Illuminate\\Validation; class Rule { public static function in(array $values): object { return new \\stdClass(); } public static function enum(string $class): object { return new \\stdClass(); } }",
        ),
        (
            "vendor/illuminate/Database/Eloquent/Model.php",
            "<?php namespace Illuminate\\Database\\Eloquent; class Model {}",
        ),
        (
            "vendor/illuminate/Support/ServiceProvider.php",
            "<?php namespace Illuminate\\Support; class ServiceProvider {}",
        ),
        (
            "vendor/livewire/Form.php",
            "<?php namespace Livewire; class Form { public function validate(array $rules): array { return []; } }",
        ),
        (
            "vendor/livewire/Component.php",
            "<?php namespace Livewire; class Component { public function validate(array $rules): array { return []; } }",
        ),
        (
            "vendor/illuminate/Foundation/Validation/ValidatesRequests.php",
            "<?php namespace Illuminate\\Foundation\\Validation; trait ValidatesRequests { public function validate(\\Illuminate\\Http\\Request $request, array $rules): array { return []; } }",
        ),
        ("src/Models/Customer.php", CUSTOMER),
        ("src/Models/Status.php", STATUS),
        ("bootstrap/providers.php", "<?php return [];"),
    ]
}

struct Fixture {
    backend: Backend,
    _dir: tempfile::TempDir,
    uri: Uri,
    content: String,
    position: Position,
}

impl Fixture {
    async fn open(marked: &str, extra: &[(&str, &str)]) -> Self {
        let position = position_of(marked, "§");
        let content = marked.replace('§', "");
        let mut sources = files();
        sources.extend_from_slice(extra);
        sources.push(("src/Probe.php", &content));
        let (backend, dir) = create_psr4_workspace(COMPOSER, &sources);
        backend.initialized(InitializedParams {}).await;
        let uri = open_php_at(&backend, &dir, "src/Probe.php", &content).await;
        Self {
            backend,
            _dir: dir,
            uri,
            content,
            position,
        }
    }

    async fn items(&self) -> Vec<CompletionItem> {
        complete_at_opened(
            &self.backend,
            &self.uri,
            self.position.line,
            self.position.character,
        )
        .await
    }

    async fn labels(&self) -> Vec<String> {
        item_labels(self.items().await)
    }

    async fn definitions(&self) -> Vec<Location> {
        definition_locations(
            goto_definition_at(
                &self.backend,
                &self.uri,
                self.position.line,
                self.position.character,
            )
            .await,
        )
    }

    fn diagnostics(&self, code: &str) -> Vec<String> {
        slow_diagnostic_messages(&self.backend, self.uri.as_str(), &self.content, code)
    }
}

fn request(body: &str) -> String {
    format!(
        r#"<?php
namespace App;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Validator;
class Probe {{
    public function run(Request $request): void {{
        {body}
    }}
}}
"#
    )
}

fn form(methods: &str) -> String {
    format!(
        r#"<?php
namespace App;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;
class Probe extends FormRequest {{
    {methods}
}}
"#
    )
}

fn replacement(item: &CompletionItem) -> TextEdit {
    match item
        .text_edit
        .as_ref()
        .expect("validation completion has an edit")
    {
        CompletionItemTextEdit::TextEdit(edit) => edit.clone(),
        other => panic!("expected a plain validation edit, got {other:?}"),
    }
}

#[tokio::test]
async fn validation_rules_come_from_the_installed_framework() {
    let fixture = Fixture::open(&request("$request->validate(['name' => '§']);"), &[]).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"fictional_cake".to_string()), "{labels:?}");
    assert!(labels.contains(&"required".to_string()), "{labels:?}");
    assert!(labels.contains(&"nullable".to_string()), "{labels:?}");
    assert!(
        !labels.contains(&"uuid".to_string()),
        "undeclared framework rule: {labels:?}"
    );
    assert!(
        !labels.contains(&"internal".to_string()),
        "private validator helper: {labels:?}"
    );
    assert!(
        !labels.contains(&"attribute".to_string()),
        "internal dispatcher: {labels:?}"
    );
}

#[tokio::test]
async fn validation_rule_completion_replaces_only_the_current_pipe_segment() {
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'required|str§ng|max:12']);"),
        &[],
    )
    .await;
    let items = fixture.items().await;
    let item = items
        .iter()
        .find(|item| item.label == "string")
        .expect("string rule");
    let edited = apply_edits(&fixture.content, &[replacement(item)]);
    assert!(edited.contains("'required|string|max:12'"), "{edited}");
}

#[tokio::test]
async fn validation_rule_completion_handles_utf16_before_the_literal() {
    let fixture = Fixture::open(
        &request("$emoji = '🍰'; $request->validate(['name' => 'fic§']);"),
        &[],
    )
    .await;
    let items = fixture.items().await;
    let item = items
        .iter()
        .find(|item| item.label == "fictional_cake")
        .expect("installed rule");
    let edit = replacement(item);
    let start = position_of(&fixture.content, "fic'");
    assert_eq!(
        edit.range,
        Range {
            start,
            end: Position {
                character: start.character + 3,
                ..start
            }
        }
    );
    assert_eq!(edit.new_text, "fictional_cake");
}

#[tokio::test]
async fn validation_rule_completion_uses_the_declared_parameter_shape() {
    let fixture = Fixture::open(&request("$request->validate(['name' => 'ma§']);"), &[]).await;
    let items = fixture.items().await;
    let item = items
        .iter()
        .find(|item| item.label == "max")
        .expect("maximum rule");
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::Snippet));
    assert!(replacement(item).new_text.starts_with("max:"));

    let fixture = Fixture::open(&request("$request->validate(['name' => 'ma§x:12']);"), &[]).await;
    let items = fixture.items().await;
    let item = items
        .iter()
        .find(|item| item.label == "max")
        .expect("maximum rule");
    let edited = apply_edits(&fixture.content, &[replacement(item)]);
    assert!(edited.contains("'max:12'"), "{edited}");
}

#[tokio::test]
async fn validation_rules_complete_array_values_and_named_arguments() {
    for body in [
        "$request->validate(['name' => ['required', 'fic§']]);",
        "$request->validate(['name' => ['key' => 'fic§']]);",
        "$request->validate(rules: ['name' => 'fic§']);",
        "$request->validateWithBag('checkout', ['name' => 'fic§']);",
        "$request->validateWithBag(rules: ['name' => 'fic§'], errorBag: 'checkout');",
        "Validator::make([], ['name' => 'fic§']);",
        "Validator::make(rules: ['name' => 'fic§'], data: []);",
        "$validator = Validator::make([], []); $validator->sometimes('name', 'fic§', fn () => true);",
        "$validator = Validator::make([], []); $validator->sometimes(rules: ['fic§'], attribute: 'name', callback: fn () => true);",
    ] {
        let fixture = Fixture::open(&request(body), &[]).await;
        assert!(
            fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{body}"
        );
    }
}

#[tokio::test]
async fn validation_rules_complete_in_form_request_and_livewire_form() {
    for parent in [
        "\\Illuminate\\Foundation\\Http\\FormRequest",
        "\\Livewire\\Form",
    ] {
        let source = format!(
            "<?php namespace App; class Probe extends {parent} {{ public function rules(): array {{ return ['name' => 'fic§']; }} }}"
        );
        let fixture = Fixture::open(&source, &[]).await;
        assert!(
            fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{parent}"
        );
    }
}

#[tokio::test]
async fn validation_rules_complete_in_controller_trait_validation() {
    let fixture = Fixture::open(
        r#"<?php
namespace App;
class Probe {
    use \Illuminate\Foundation\Validation\ValidatesRequests;
    public function run(\Illuminate\Http\Request $request): void {
        $this->validate($request, ['name' => 'fic§']);
    }
}
"#,
        &[],
    )
    .await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"fictional_cake".to_string())
    );
}

#[tokio::test]
async fn validation_rules_leave_field_keys_data_and_nested_values_alone() {
    for body in [
        "$request->validate(['fic§' => 'required']);",
        "Validator::make(['name' => 'fic§'], ['name' => 'required']);",
        "$request->validate(['name' => [['fic§']]]);",
        "$request->validate(['name' => ['fic§' => 'required']]);",
        "$request->validate(['name' => 'required']); $text = 'fic§';",
    ] {
        let fixture = Fixture::open(&request(body), &[]).await;
        assert!(
            !fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{body}"
        );
    }
}

#[tokio::test]
async fn validation_rules_require_the_receiver_type() {
    let fixture = Fixture::open(
        r#"<?php
namespace App;
class Unrelated {
    public function validate(array $rules): void {}
}
class Probe {
    public function run(Unrelated $request): void {
        $request->validate(['name' => 'fic§']);
    }
}
"#,
        &[],
    )
    .await;
    assert!(
        !fixture
            .labels()
            .await
            .contains(&"fictional_cake".to_string())
    );
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_rules_resolve_aliases_and_grouped_imports() {
    for import in [
        "use Illuminate\\Support\\Facades\\Validator as Checks;",
        "use Illuminate\\Support\\Facades\\{Validator as Checks};",
        "use Illuminate\\Support\\Facades as Framework;",
    ] {
        let name = if import.contains(" as Framework") {
            "Framework\\Validator"
        } else {
            "Checks"
        };
        let source = format!("<?php namespace App; {import} {name}::make([], ['name' => 'fic§']);");
        let fixture = Fixture::open(&source, &[]).await;
        assert!(
            fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{source}"
        );
    }
}

#[tokio::test]
async fn validation_rules_do_not_claim_a_namespaced_facade_homonym() {
    for source in [
        "<?php namespace App; class Validator { public static function make($data, $rules): void {} } Validator::make([], ['name' => 'fic§']);",
        "<?php namespace App; use App\\OtherValidator as Validator; class OtherValidator { public static function make($data, $rules): void {} } Validator::make([], ['name' => 'fic§']);",
        "<?php namespace App; class Probe { public function rules(): array { return ['name' => 'fic§']; } }",
    ] {
        let fixture = Fixture::open(source, &[]).await;
        assert!(
            !fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{source}"
        );
        assert!(
            fixture.diagnostics("invalid_validation_rule").is_empty(),
            "{source}"
        );
    }
}

#[tokio::test]
async fn validation_facade_resolution_distinguishes_global_and_namespaced_classes() {
    let declarations = r#"<?php
namespace {
    class Validator { public static function make($data, $rules): void {} }
}
namespace App {
    class Validator { public static function make($data, $rules): void {} }
}
"#;
    for (name, import, expected) in [
        ("Validator", "", false),
        ("\\Validator", "", false),
        (
            "Checks",
            "use Illuminate\\Support\\Facades\\Validator as Checks;",
            true,
        ),
        ("Checks", "use App\\Validator as Checks;", false),
    ] {
        let source = format!(
            "{declarations} namespace Caller {{ {import} {name}::make([], ['name' => 'fic§']); }}"
        );
        let fixture = Fixture::open(&source, &[]).await;
        assert_eq!(
            fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            expected,
            "{name}: {import}"
        );
    }
}

#[tokio::test]
async fn validation_rules_hover_and_navigate_to_the_framework_declaration() {
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'fic§tional_cake']);"),
        &[],
    )
    .await;
    let hover = hover_text_at(
        &fixture.backend,
        &fixture.uri,
        fixture.position.line,
        fixture.position.character,
    )
    .await
    .expect("validation rule hover");
    assert!(hover.contains("fictional_cake"), "{hover}");
    assert!(
        hover.contains("fictional cake supported by this framework version"),
        "{hover}"
    );
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/ValidatesAttributes.php"),
        "{definitions:?}"
    );
    assert_eq!(
        definitions[0].range.start.line,
        position_of(ATTRIBUTES, "validateFictionalCake").line
    );
}

#[tokio::test]
async fn validation_rules_report_only_unknown_rule_names() {
    let fixture = Fixture::open(&request("$request->validate(['name' => 'required|not_a_rule§|max:12', 'other' => ['fictional_cake', 'not_another_rule']]);"), &[]).await;
    let messages = fixture.diagnostics("invalid_validation_rule");
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .any(|message| message.contains("not_a_rule")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("not_another_rule")),
        "{messages:?}"
    );
}

#[tokio::test]
async fn validation_rules_use_installed_parser_aliases() {
    let fixture = Fixture::open(
        &request("$request->validate(['count' => 'int§', 'enabled' => 'bool']);"),
        &[],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    assert!(fixture.labels().await.contains(&"int".to_string()));
}

#[tokio::test]
async fn validation_rules_do_not_split_array_regex_patterns_at_pipes() {
    let fixture = Fixture::open(&request(r#"$request->validate(['name' => ['regex:/^(foo|bar)$/', 'not_regex:/^(bad|worse)$/', 'required§']]);"#), &[]).await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_rule_parameters_complete_sibling_fields() {
    for rule in ["same", "different", "required_with", "gt", "after"] {
        let fixture = Fixture::open(
            &request(&format!(
                "$request->validate(['password' => 'required', 'other' => '{rule}:pass§']);"
            )),
            &[],
        )
        .await;
        assert!(
            fixture.labels().await.contains(&"password".to_string()),
            "{rule}"
        );
        let definitions = fixture.definitions().await;
        assert!(
            definitions.is_empty(),
            "a partial field is not an exact reference: {definitions:?}"
        );
    }
}

#[tokio::test]
async fn validation_rule_parameters_navigate_to_sibling_field_keys() {
    let fixture = Fixture::open(
        &request("$request->validate(['password' => 'required', 'other' => 'same:pass§word']);"),
        &[],
    )
    .await;
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(definitions[0].uri, fixture.uri);
    assert_eq!(
        definitions[0].range.start,
        position_of(&fixture.content, "password' =>")
    );
}

#[tokio::test]
async fn validation_rule_parameters_replace_only_the_selected_comma_value() {
    let fixture = Fixture::open(&request("$request->validate(['password' => 'required', 'email' => 'required', 'other' => 'required_with:password,em§il']);"), &[]).await;
    let items = fixture.items().await;
    let item = items
        .iter()
        .find(|item| item.label == "email")
        .expect("sibling email");
    let edited = apply_edits(&fixture.content, &[replacement(item)]);
    assert!(
        edited.contains("'required_with:password,email'"),
        "{edited}"
    );
}

#[tokio::test]
async fn validation_database_parameters_complete_model_tables_and_columns() {
    for (value, expected) in [
        ("exists:cust§", "customers"),
        ("unique:cust§", "customers"),
        ("exists:customers,em§", "email"),
        ("unique:customers,sta§", "status"),
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['field' => '{value}']);")),
            &[],
        )
        .await;
        assert!(
            fixture.labels().await.contains(&expected.to_string()),
            "{value}"
        );
    }
}

#[tokio::test]
async fn validation_database_parameters_navigate_to_the_model() {
    for value in ["exists:cust§omers,email", "unique:customers,em§ail"] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['field' => '{value}']);")),
            &[],
        )
        .await;
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{value}: {definitions:?}");
        assert!(
            definitions[0]
                .uri
                .as_str()
                .ends_with("/Models/Customer.php"),
            "{definitions:?}"
        );
    }
}

#[tokio::test]
async fn validation_enum_values_come_from_the_referenced_model_column() {
    for rules in [
        "'exists:customers,status|in:pen§'",
        "['exists:customers,status', 'in:pen§']",
        "['exists:customers,status', \\Illuminate\\Validation\\Rule::in(['pen§'])]",
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['state' => {rules}]);")),
            &[],
        )
        .await;
        let labels = fixture.labels().await;
        assert!(
            labels.contains(&"pending".to_string()),
            "{rules}: {labels:?}"
        );
        assert!(!labels.contains(&"required".to_string()), "{labels:?}");
    }
}

#[tokio::test]
async fn validation_enum_values_do_not_guess_a_model_from_a_field_name() {
    let fixture = Fixture::open(
        &request("$request->validate(['status' => 'in:pen§']);"),
        &[],
    )
    .await;
    assert!(!fixture.labels().await.contains(&"pending".to_string()));
}

#[tokio::test]
async fn validation_message_keys_complete_fields_and_rules() {
    for (key, expected) in [("em§", "email"), ("email.req§", "email.required")] {
        let source = form(&format!(
            "public function rules(): array {{ return ['email' => 'required|email']; }} public function messages(): array {{ return ['{key}' => 'A message']; }}"
        ));
        let fixture = Fixture::open(&source, &[]).await;
        let labels = fixture.labels().await;
        assert!(labels.contains(&expected.to_string()), "{key}: {labels:?}");
    }
}

#[tokio::test]
async fn validation_attribute_keys_complete_and_navigate_to_fields() {
    let source = form(
        "public function rules(): array { return ['email' => 'required|email']; } public function attributes(): array { return ['em§ail' => 'Contact email']; }",
    );
    let fixture = Fixture::open(&source, &[]).await;
    assert!(fixture.labels().await.contains(&"email".to_string()));
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(definitions[0].uri, fixture.uri);
    assert_eq!(
        definitions[0].range.start,
        position_of(&fixture.content, "email' => 'required")
    );
}

#[tokio::test]
async fn validation_message_keys_report_unknown_fields_and_rules() {
    let source = form(
        "public function rules(): array { return ['email' => 'required|email']; } public function messages(): array { return ['emali.required' => 'Bad field', 'email.not_a_rule' => 'Bad rule', 'email.required' => 'Good']; } public function attributes(): array { return ['other§' => 'Unknown', 'email' => 'Contact']; }",
    );
    let fixture = Fixture::open(&source, &[]).await;
    let messages = fixture.diagnostics("invalid_validation_message");
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(
        messages.iter().any(|message| message.contains("emali")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("not_a_rule")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|message| message.contains("other")),
        "{messages:?}"
    );
}

#[tokio::test]
async fn validation_message_keys_leave_dynamic_rule_fields_open() {
    let source = form(
        "public function rules(): array { $field = 'runtime'; return [$field => 'required', 'email' => 'email']; } public function messages(): array { return ['runtime§.required' => 'Required']; }",
    );
    let fixture = Fixture::open(&source, &[]).await;
    assert!(fixture.diagnostics("invalid_validation_message").is_empty());
}

#[tokio::test]
async fn validation_message_keys_resolve_inherited_fields_in_the_declaration_file() {
    let parent = r#"<?php
namespace App\Requests;
class ParentRequest extends \Illuminate\Foundation\Http\FormRequest {
    public function rules(): array {
        return ['email' => 'required|email'];
    }
}
"#;
    let source = r#"<?php
namespace App;
class Probe extends \App\Requests\ParentRequest {
    public function attributes(): array { return ['em§ail' => 'Contact']; }
}
"#;
    let fixture = Fixture::open(source, &[("src/Requests/ParentRequest.php", parent)]).await;
    assert!(fixture.labels().await.contains(&"email".to_string()));
    assert!(fixture.diagnostics("invalid_validation_message").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Requests/ParentRequest.php"),
        "{definitions:?}"
    );
    assert_eq!(definitions[0].range.start, position_of(parent, "email' =>"));
}

#[tokio::test]
async fn validation_rule_classes_complete_as_objects_only_in_rule_arrays() {
    let extra = [("src/Rules/UppercaseRule.php", CUSTOM_RULE)];
    let fixture = Fixture::open(&request("$request->validate(['name' => ['§']]);"), &extra).await;
    let items = fixture.items().await;
    let rule = items
        .iter()
        .find(|item| item.label.contains("UppercaseRule"))
        .expect("custom rule class");
    let edited = apply_edits(&fixture.content, &[replacement(rule)]);
    assert!(
        edited.contains("new \\App\\Rules\\UppercaseRule()"),
        "{edited}"
    );
    assert!(
        !edited.contains("'new \\App\\Rules\\UppercaseRule()'"),
        "rule objects must not be quoted: {edited}"
    );

    let fixture = Fixture::open(&request("$request->validate(['name' => '§']);"), &extra).await;
    assert!(
        fixture
            .labels()
            .await
            .iter()
            .all(|label| !label.contains("UppercaseRule"))
    );
}

#[tokio::test]
async fn validation_custom_registrations_complete_and_navigate() {
    let provider = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\{Validator as Checks};
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(): void {
        Checks::extend('bakery_code', fn ($attribute, $value) => true);
        Checks::extendImplicit('bakery_required', fn ($attribute, $value) => true);
        Checks::extendDependent('bakery_matches', fn ($attribute, $value, $parameters) => true);
    }
}
"#;
    let extra = [("src/Providers/RuleProvider.php", provider)];
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'bakery_§']);"),
        &extra,
    )
    .await;
    let labels = fixture.labels().await;
    for name in ["bakery_code", "bakery_required", "bakery_matches"] {
        assert!(labels.contains(&name.to_string()), "{labels:?}");
    }
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'bakery_co§de']);"),
        &extra,
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0].uri.as_str().ends_with("/RuleProvider.php"),
        "{definitions:?}"
    );
}

#[tokio::test]
async fn validation_dynamic_registrations_keep_unknown_rules_open() {
    let provider = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\Validator;
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function registerRule(string $name): void {
        Validator::extend($name, fn ($attribute, $value) => true);
    }
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'runtime_only§']);"),
        &[("src/Providers/RuleProvider.php", provider)],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_rules_without_readable_framework_declarations_are_not_diagnosed() {
    let empty_attributes =
        "<?php namespace Illuminate\\Validation\\Concerns; trait ValidatesAttributes {}";
    let empty_validator = "<?php namespace Illuminate\\Validation; class Validator {}";
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'runtime_only§']);"),
        &[
            (
                "vendor/illuminate/Validation/Concerns/ValidatesAttributes.php",
                empty_attributes,
            ),
            (
                "vendor/illuminate/Validation/Validator.php",
                empty_validator,
            ),
        ],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_unrelated_registration_does_not_add_rules() {
    let source = r#"<?php
namespace App;
class Validator { public static function extend(string $name, $callback): void {} }
Validator::extend('unrelated_rule', fn () => true);
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'unrelated_§']);"),
        &[("src/Other.php", source)],
    )
    .await;
    assert!(
        !fixture
            .labels()
            .await
            .contains(&"unrelated_rule".to_string())
    );
}

#[tokio::test]
async fn validation_catalog_updates_after_a_registration_edit() {
    let provider = "<?php namespace App; use Illuminate\\Support\\Facades\\Validator; Validator::extend('first_rule', fn () => true);";
    let fixture = Fixture::open(
        &request("$request->validate(['name' => '§']);"),
        &[("src/Rules.php", provider)],
    )
    .await;
    assert!(fixture.labels().await.contains(&"first_rule".to_string()));
    let changed = provider.replace("first_rule", "second_rule");
    let provider_uri = Uri::from_file_path(fixture._dir.path().join("src/Rules.php")).unwrap();
    crate::common::open_php(&fixture.backend, &provider_uri, &changed).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"second_rule".to_string()), "{labels:?}");
    assert!(!labels.contains(&"first_rule".to_string()), "{labels:?}");
}

#[tokio::test]
async fn validation_rules_complete_an_unterminated_rule_string() {
    let fixture = Fixture::open("<?php namespace App; function check(\\Illuminate\\Http\\Request $request) { $request->validate(['name' => 'fic§", &[]).await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"fictional_cake".to_string())
    );
}

#[tokio::test]
async fn validation_rule_names_follow_laravel_case_normalization() {
    let fixture = Fixture::open(
        &request(
            "$request->validate(['primary' => 'REQUIRED', 'other' => 'RequiredWith:pri§mary']);",
        ),
        &[],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    assert!(fixture.labels().await.contains(&"primary".to_string()));
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(
        definitions[0].range.start,
        position_of(&fixture.content, "primary' =>")
    );

    let fixture = Fixture::open(
        &request("$request->validate(['primary' => 'Required§With:other']);"),
        &[],
    )
    .await;
    let hover = hover_text_at(
        &fixture.backend,
        &fixture.uri,
        fixture.position.line,
        fixture.position.character,
    )
    .await
    .expect("normalized rule hover");
    assert!(
        hover.contains("required_with") || hover.contains("RequiredWith"),
        "{hover}"
    );

    let fixture = Fixture::open(
        &request("$request->validate(['primary' => 'RequiredW§']);"),
        &[],
    )
    .await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"required_with".to_string())
    );
}

#[tokio::test]
async fn validation_wildcard_message_keys_match_declared_fields() {
    let source = form(
        "public function rules(): array { return ['email' => 'required', 'items.*.email' => 'required|email']; } public function messages(): array { return ['*.required' => 'Required', 'items.*.email.required' => 'Item email required', 'items.*.email.email' => 'Invalid email', 'missing.*.required§' => 'Unknown field']; }",
    );
    let fixture = Fixture::open(&source, &[]).await;
    let messages = fixture.diagnostics("invalid_validation_message");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("missing.*.required"), "{messages:?}");
}

#[tokio::test]
async fn validation_rule_in_requires_the_framework_rule_class() {
    let source = r#"<?php
namespace App;
class Rule { public static function in(array $values): object { return new \stdClass(); } }
class Probe {
    public function run(\Illuminate\Http\Request $request): void {
        $request->validate(['state' => ['exists:customers,status', Rule::in(['pen§'])]]);
    }
}
"#;
    let fixture = Fixture::open(source, &[]).await;
    assert!(!fixture.labels().await.contains(&"pending".to_string()));
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_enum_rule_supplies_values_without_a_database_reference() {
    for value in ["'in:pen§'", "Rules::in(['pen§'])"] {
        let source = format!(
            r#"<?php
namespace App;
use App\Models\Status as State;
use Illuminate\Validation\Rule as Rules;
class Probe {{
    public function run(\Illuminate\Http\Request $request): void {{
        $request->validate(['state' => [Rules::enum(State::class), {value}]]);
    }}
}}
"#
        );
        let fixture = Fixture::open(&source, &[]).await;
        assert!(
            fixture.labels().await.contains(&"pending".to_string()),
            "{value}"
        );
    }
}

#[tokio::test]
async fn validation_connection_prefixed_tables_resolve_columns_and_enum_values() {
    let model = CUSTOMER.replace(
        "protected $table",
        "protected $connection = 'mysql';\n    protected $table",
    );
    let extra = [("src/Models/Customer.php", model.as_str())];
    for value in ["exists:mysql.customers,em§", "unique:mysql.customers,em§"] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['field' => '{value}']);")),
            &extra,
        )
        .await;
        assert!(
            fixture.labels().await.contains(&"email".to_string()),
            "{value}"
        );
    }
    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:mysql.customers,status|in:pen§']);"),
        &extra,
    )
    .await;
    assert!(fixture.labels().await.contains(&"pending".to_string()));

    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:mysql.customers,em§ail']);"),
        &extra,
    )
    .await;
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Models/Customer.php"),
        "{definitions:?}"
    );
}

#[tokio::test]
async fn validation_connection_prefix_is_preserved_when_completing_a_table() {
    let model = CUSTOMER.replace(
        "protected $table",
        "protected $connection = 'mysql';\n    protected $table",
    );
    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:mysql.cust§']);"),
        &[("src/Models/Customer.php", model.as_str())],
    )
    .await;
    let items = fixture.items().await;
    let table = items
        .iter()
        .find(|item| item.label == "customers" || item.label == "mysql.customers")
        .expect("table name after connection prefix");
    let edited = apply_edits(&fixture.content, &[replacement(table)]);
    assert!(edited.contains("'exists:mysql.customers'"), "{edited}");
}

#[tokio::test]
async fn validation_column_definitions_point_to_the_actual_model_array_entries() {
    let model = r#"<?php
namespace App\Models;
class Customer extends \Illuminate\Database\Eloquent\Model {
    protected $table = 'customers';
    protected $fillable = [
        'email',
    ];
    protected $casts = [
        'status' => Status::class,
    ];
}
"#;
    for (column, declaration) in [("em§ail", "email',"), ("sta§tus", "status' =>")] {
        let fixture = Fixture::open(
            &request(&format!(
                "$request->validate(['field' => 'exists:customers,{column}']);"
            )),
            &[("src/Models/Customer.php", model)],
        )
        .await;
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{column}: {definitions:?}");
        assert!(
            definitions[0]
                .uri
                .as_str()
                .ends_with("/Models/Customer.php"),
            "{definitions:?}"
        );
        assert_eq!(definitions[0].range.start, position_of(model, declaration));
    }
}

#[tokio::test]
async fn validation_inherited_columns_use_the_declaring_models_namespace() {
    let global = "<?php class BaseCustomer extends \\Illuminate\\Database\\Eloquent\\Model { protected $fillable = ['wrong_global']; }";
    let base = r#"<?php
namespace App\Models;
class BaseCustomer extends \Illuminate\Database\Eloquent\Model {
    protected $fillable = ['email'];
}
"#;
    let child = r#"<?php
namespace App\Models;
class PremiumCustomer extends BaseCustomer {
    protected $table = 'premium_customers';
}
"#;
    let caller = r#"<?php
namespace Caller;
class BaseCustomer extends \Illuminate\Database\Eloquent\Model {
    protected $fillable = ['wrong_caller'];
}
class Probe {
    public function run(\Illuminate\Http\Request $request): void {
        $request->validate(['field' => 'exists:premium_customers,em§ail']);
    }
}
"#;
    let fixture = Fixture::open(
        caller,
        &[
            ("src/GlobalBaseCustomer.php", global),
            ("src/Models/BaseCustomer.php", base),
            ("src/Models/PremiumCustomer.php", child),
        ],
    )
    .await;
    assert!(fixture.labels().await.contains(&"email".to_string()));
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Models/BaseCustomer.php"),
        "{definitions:?}"
    );
    assert_eq!(definitions[0].range.start, position_of(base, "email'"));
}

#[tokio::test]
async fn validation_model_subclass_edits_replace_cached_table_names() {
    let child = r#"<?php
namespace App\Models;
class ArchivedCustomer extends Customer {
    protected $table = 'archived_customers';
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:arch§']);"),
        &[("src/Models/ArchivedCustomer.php", child)],
    )
    .await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"archived_customers".to_string())
    );
    let changed = child.replace("archived_customers", "archived_accounts");
    open_php_at(
        &fixture.backend,
        &fixture._dir,
        "src/Models/ArchivedCustomer.php",
        &changed,
    )
    .await;
    let labels = fixture.labels().await;
    assert!(
        labels.contains(&"archived_accounts".to_string()),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"archived_customers".to_string()),
        "{labels:?}"
    );
}

#[tokio::test]
async fn validation_enum_declaration_edits_refresh_suggested_values() {
    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:customers,status|in:§']);"),
        &[],
    )
    .await;
    assert!(fixture.labels().await.contains(&"active".to_string()));
    let changed = STATUS.replace("case Active = 'active';", "case Archived = 'archived';");
    open_php_at(
        &fixture.backend,
        &fixture._dir,
        "src/Models/Status.php",
        &changed,
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"archived".to_string()), "{labels:?}");
    assert!(!labels.contains(&"active".to_string()), "{labels:?}");
}

#[tokio::test]
async fn validation_cast_edits_replace_the_enum_source() {
    let other_enum =
        "<?php namespace App\\Models; enum Fulfillment: string { case Paid = 'paid'; }";
    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:customers,status|in:§']);"),
        &[("src/Models/Fulfillment.php", other_enum)],
    )
    .await;
    assert!(fixture.labels().await.contains(&"pending".to_string()));
    let changed = CUSTOMER.replace("Status::class", "Fulfillment::class");
    open_php_at(
        &fixture.backend,
        &fixture._dir,
        "src/Models/Customer.php",
        &changed,
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"paid".to_string()), "{labels:?}");
    assert!(!labels.contains(&"pending".to_string()), "{labels:?}");
}

#[tokio::test]
async fn validation_sometimes_references_fields_from_its_validator_factory() {
    let fixture = Fixture::open(
        &request(
            r#"
        $validator = Validator::make([], ['password' => 'required']);
        $validator->sometimes('confirm', 'same:pass§word', fn () => true);
    "#,
        ),
        &[],
    )
    .await;
    assert!(fixture.labels().await.contains(&"password".to_string()));
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(definitions[0].uri, fixture.uri);
    assert_eq!(
        definitions[0].range.start,
        position_of(&fixture.content, "password' =>")
    );
}

#[tokio::test]
async fn validation_sometimes_does_not_read_another_validators_fields() {
    let fixture = Fixture::open(
        &request(
            r#"
        $first = Validator::make([], ['password' => 'required']);
        $second = Validator::make([], ['email' => 'required']);
        $first->sometimes('confirm', 'same:§', fn () => true);
    "#,
        ),
        &[],
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"password".to_string()), "{labels:?}");
    assert!(!labels.contains(&"email".to_string()), "{labels:?}");
}

#[tokio::test]
async fn validation_sometimes_forgets_fields_after_receiver_reassignment() {
    let fixture = Fixture::open(
        &request(
            r#"
        $validator = Validator::make([], ['password' => 'required']);
        $validator = new \Illuminate\Validation\Validator();
        $validator->sometimes('confirm', 'same:pass§word', fn () => true);
    "#,
        ),
        &[],
    )
    .await;
    assert!(!fixture.labels().await.contains(&"password".to_string()));
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_sometimes_does_not_assume_a_conditional_assignment_ran() {
    let fixture = Fixture::open(
        &request(
            r#"
        $validator = Validator::make([], ['password' => 'required']);
        if ($request->all()) {
            $validator = new \Illuminate\Validation\Validator();
        }
        $validator->sometimes('confirm', 'same:pass§word', fn () => true);
    "#,
        ),
        &[],
    )
    .await;
    assert!(!fixture.labels().await.contains(&"password".to_string()));
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_unique_ignore_values_are_not_field_references() {
    let fixture = Fixture::open(&request("$request->validate(['ignored' => 'required', 'email' => 'unique:customers,email,ign§ored']);"), &[]).await;
    let labels = fixture.labels().await;
    assert!(!labels.contains(&"ignored".to_string()), "{labels:?}");
    assert!(!labels.contains(&"email".to_string()), "{labels:?}");
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_database_columns_exclude_guarded_wildcards_and_relationships() {
    let model = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Relations\HasMany;
class Customer extends \Illuminate\Database\Eloquent\Model {
    protected $table = 'customers';
    protected $fillable = ['email'];
    protected $guarded = ['*'];
    /** @return HasMany<Order, $this> */
    public function orders(): HasMany { return new HasMany(); }
}
"#;
    let relation = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/** @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model */
class HasMany {}
"#;
    let extra = [
        ("src/Models/Customer.php", model),
        (
            "src/Models/Order.php",
            "<?php namespace App\\Models; class Order extends \\Illuminate\\Database\\Eloquent\\Model {}",
        ),
        (
            "vendor/illuminate/Database/Eloquent/Relations/HasMany.php",
            relation,
        ),
    ];
    let property_site = r#"<?php
namespace App;
class Probe {
    public function run(\App\Models\Customer $customer): void { $customer->ord§ers; }
}
"#;
    let fixture = Fixture::open(property_site, &extra).await;
    let properties = fixture.labels().await;
    assert!(
        properties.contains(&"orders".to_string()),
        "fixture must expose a relation: {properties:?}"
    );
    assert!(
        properties.contains(&"orders_count".to_string()),
        "fixture must expose a count: {properties:?}"
    );

    let fixture = Fixture::open(
        &request("$request->validate(['field' => 'exists:customers,§']);"),
        &extra,
    )
    .await;
    let columns = fixture.labels().await;
    assert!(columns.contains(&"email".to_string()), "{columns:?}");
    for excluded in ["*", "orders", "orders_count"] {
        assert!(
            !columns.contains(&excluded.to_string()),
            "{excluded} is not a database column: {columns:?}"
        );
    }
}

#[tokio::test]
async fn validation_completion_ignores_config_and_ordinary_strings() {
    for body in [
        "config('fic§');",
        "$message = 'fic§';",
        "$options = ['name' => 'fic§'];",
        "$request->validate(['name' => 'required']); $message = 'fic§';",
        "$request->validate(['name' => 'required'], ['name.required' => 'fic§']);",
    ] {
        let fixture = Fixture::open(&request(body), &[]).await;
        assert!(
            !fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{body}"
        );
    }
}

#[tokio::test]
async fn validation_enum_completion_escapes_values_for_the_surrounding_php_quotes() {
    let enum_source = r#"<?php
namespace App\Models;
enum Status: string {
    case Apostrophe = "baker's";
    case Backslash = 'a\\b';
    case Dollar = '$cost';
}
"#;
    for quote in ['\'', '"'] {
        let body = format!(
            "$request->validate(['state' => [\\Illuminate\\Validation\\Rule::enum(\\App\\Models\\Status::class), \\Illuminate\\Validation\\Rule::in([{quote}§{quote}])]]);"
        );
        let fixture =
            Fixture::open(&request(&body), &[("src/Models/Status.php", enum_source)]).await;
        let items = fixture.items().await;
        let expected = if quote == '\'' {
            [
                ("baker's", "baker\\'s"),
                ("a\\b", "a\\\\b"),
                ("$cost", "$cost"),
            ]
        } else {
            [
                ("baker's", "baker's"),
                ("a\\b", "a\\\\b"),
                ("$cost", "\\$cost"),
            ]
        };
        let mut insertion = position_of(&fixture.content, &format!("{quote}{quote}"));
        insertion.character += 1;
        for (value, escaped) in expected {
            let item = items
                .iter()
                .find(|item| item.label == value)
                .unwrap_or_else(|| panic!("missing enum value {value:?}: {items:?}"));
            let edit = replacement(item);
            assert_eq!(
                edit.range,
                Range {
                    start: insertion,
                    end: insertion
                }
            );
            assert_eq!(edit.new_text, escaped);
            let edited = apply_edits(&fixture.content, &[edit]);
            assert!(
                edited.contains(&format!("::in([{quote}{escaped}{quote}])")),
                "{edited}"
            );
        }
    }
}

#[tokio::test]
async fn validation_integer_enum_completion_uses_runtime_values() {
    let enum_source = r#"<?php
namespace App\Models;
enum Status: int {
    case Hexadecimal = 0x10;
    case Octal = 010;
    case Computed = 1 + 2;
}
"#;
    let fixture = Fixture::open(&request("$request->validate(['state' => [\\Illuminate\\Validation\\Rule::enum(\\App\\Models\\Status::class), \\Illuminate\\Validation\\Rule::in(['§'])]]);"), &[("src/Models/Status.php", enum_source)]).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"16".to_string()), "{labels:?}");
    assert!(labels.contains(&"8".to_string()), "{labels:?}");
    for unsupported in ["0x10", "010", "1 + 2", "3"] {
        assert!(
            !labels.contains(&unsupported.to_string()),
            "raw or unevaluated backing expression: {labels:?}"
        );
    }
}

#[tokio::test]
async fn validation_escaped_enum_values_support_resumed_completion_hover_and_definition() {
    let enum_source = r#"<?php
namespace App\Models;
enum Status: string {
    case Apostrophe = "baker's";
    case Backslash = 'a\\b';
    case Dollar = '$cost';
}
"#;
    for (marked, value, escaped, case) in [
        (r"'baker\'§s'", "baker's", r"baker\'s", "Apostrophe"),
        (r"'a\\§b'", r"a\b", r"a\\b", "Backslash"),
        (r#""\$§cost""#, "$cost", r"\$cost", "Dollar"),
    ] {
        let body = format!(
            "$request->validate(['state' => [\\Illuminate\\Validation\\Rule::enum(\\App\\Models\\Status::class), \\Illuminate\\Validation\\Rule::in([{marked}])]]);"
        );
        let fixture =
            Fixture::open(&request(&body), &[("src/Models/Status.php", enum_source)]).await;
        let items = fixture.items().await;
        let item = items
            .iter()
            .find(|item| item.label == value)
            .unwrap_or_else(|| panic!("missing escaped prefix {marked:?}: {items:?}"));
        let prefix = &marked[1..marked.find('§').unwrap()];
        assert!(
            item.filter_text
                .as_deref()
                .is_some_and(|text| text.starts_with(prefix)),
            "{item:?}"
        );
        let edit = replacement(item);
        let literal = marked.replace('§', "");
        let mut start = position_of(&fixture.content, &literal);
        start.character += 1;
        let end = Position {
            character: start.character + literal.len() as u32 - 2,
            ..start
        };
        assert_eq!(edit.range, Range { start, end });
        assert_eq!(edit.new_text, escaped);
        assert_eq!(apply_edits(&fixture.content, &[edit]), fixture.content);

        let hover = hover_text_at(
            &fixture.backend,
            &fixture.uri,
            fixture.position.line,
            fixture.position.character,
        )
        .await
        .expect("escaped enum value hover");
        assert!(hover.contains(value), "{hover}");
        assert!(hover.contains(&format!("Status::{case}")), "{hover}");
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{definitions:?}");
        assert!(
            definitions[0].uri.as_str().ends_with("/Models/Status.php"),
            "{definitions:?}"
        );
        assert_eq!(definitions[0].range.start, position_of(enum_source, case));
    }
}

#[tokio::test]
async fn validation_in_completion_csv_quotes_enum_values_containing_commas() {
    let enum_source =
        "<?php namespace App\\Models; enum Status: string { case Comma = 'alpha,beta'; }";
    for (literal, escaped) in [
        ("'in:al§'", r#""alpha,beta""#),
        (r#""in:al§""#, r#"\"alpha,beta\""#),
    ] {
        let body = format!(
            "$request->validate(['state' => [\\Illuminate\\Validation\\Rule::enum(\\App\\Models\\Status::class), {literal}]]);"
        );
        let fixture =
            Fixture::open(&request(&body), &[("src/Models/Status.php", enum_source)]).await;
        let items = fixture.items().await;
        let item = items
            .iter()
            .find(|item| item.label == "alpha,beta")
            .expect("comma enum value");
        let edit = replacement(item);
        assert_eq!(edit.new_text, escaped);
        let edited = apply_edits(&fixture.content, &[edit]);
        assert!(edited.contains(&format!("in:{escaped}")), "{edited}");
    }
}

#[tokio::test]
async fn validation_csv_enum_values_keep_their_whole_reference_range() {
    let enum_source =
        "<?php namespace App\\Models; enum Status: string { case Comma = 'alpha,beta'; }";
    for (literal, escaped) in [
        (r#"'in:other,"alpha,§beta",tail'"#, r#""alpha,beta""#),
        (r#""in:other,\"alpha,§beta\",tail""#, r#"\"alpha,beta\""#),
    ] {
        let body = format!(
            "$request->validate(['state' => [\\Illuminate\\Validation\\Rule::enum(\\App\\Models\\Status::class), {literal}]]);"
        );
        let fixture =
            Fixture::open(&request(&body), &[("src/Models/Status.php", enum_source)]).await;
        let items = fixture.items().await;
        let item = items
            .iter()
            .find(|item| item.label == "alpha,beta")
            .expect("CSV enum prefix");
        let prefix_start = literal.find(',').unwrap() + 1;
        let prefix = &literal[prefix_start..literal.find('§').unwrap()];
        assert!(
            item.filter_text
                .as_deref()
                .is_some_and(|text| text.starts_with(prefix)),
            "{item:?}"
        );
        let edit = replacement(item);
        assert_eq!(edit.new_text, escaped);
        assert_eq!(apply_edits(&fixture.content, &[edit]), fixture.content);
        let hover = hover_text_at(
            &fixture.backend,
            &fixture.uri,
            fixture.position.line,
            fixture.position.character,
        )
        .await
        .expect("CSV enum value hover");
        assert!(hover.contains("alpha,beta"), "{hover}");
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{definitions:?}");
        assert_eq!(
            definitions[0].range.start,
            position_of(enum_source, "Comma")
        );
    }
}

#[tokio::test]
async fn validation_pipe_enum_values_are_offered_only_where_they_stay_one_rule() {
    let enum_source = "<?php namespace App\\Models; enum Status: string { case Pipe = 'alpha|beta'; case Plain = 'alphabet'; }";
    for (rules, pipe_is_valid) in [
        ("'exists:customers,status|in:al§'", false),
        ("['exists:customers,status', 'in:al§']", true),
        (
            "['exists:customers,status', \\Illuminate\\Validation\\Rule::in(['al§'])]",
            true,
        ),
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['state' => {rules}]);")),
            &[("src/Models/Status.php", enum_source)],
        )
        .await;
        let labels = fixture.labels().await;
        assert!(
            labels.contains(&"alphabet".to_string()),
            "{rules}: {labels:?}"
        );
        assert_eq!(
            labels.contains(&"alpha|beta".to_string()),
            pipe_is_valid,
            "{rules}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn validation_rule_diagnostics_decode_php_hex_escapes_only_in_double_quotes() {
    for rules in [r#""requ\x69red§|str\x69ng""#, r#""\x72equired§""#] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['name' => {rules}]);")),
            &[],
        )
        .await;
        let messages = fixture.diagnostics("invalid_validation_rule");
        assert!(messages.is_empty(), "{rules}: {messages:?}");
    }
    let fixture = Fixture::open(
        &request(r"$request->validate(['name' => 'requ\x69red§']);"),
        &[],
    )
    .await;
    assert_eq!(fixture.diagnostics("invalid_validation_rule").len(), 1);
}

#[tokio::test]
async fn validation_factory_instance_registrations_are_known_and_navigable() {
    for (factory, method) in [
        ("Illuminate\\Validation\\Factory", "extend"),
        ("Illuminate\\Validation\\Factory", "extendDependent"),
        ("Illuminate\\Contracts\\Validation\\Factory", "extend"),
        (
            "Illuminate\\Contracts\\Validation\\Factory",
            "extendImplicit",
        ),
    ] {
        let provider = format!(
            r#"<?php
namespace App\Providers;
use {factory} as RuleFactory;
class RuleProvider extends \Illuminate\Support\ServiceProvider {{
    public function boot(RuleFactory $factory): void {{
        $factory->{method}('factory_rule', fn ($attribute, $value) => true);
    }}
}}
"#
        );
        let fixture = Fixture::open(
            &request("$request->validate(['name' => 'factory_r§ule']);"),
            &[("src/Providers/RuleProvider.php", provider.as_str())],
        )
        .await;
        let labels = fixture.labels().await;
        assert!(
            labels.contains(&"factory_rule".to_string()),
            "{factory}::{method}: {labels:?}"
        );
        assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{factory}::{method}: {definitions:?}");
        assert!(
            definitions[0]
                .uri
                .as_str()
                .ends_with("/Providers/RuleProvider.php"),
            "{definitions:?}"
        );
        assert_eq!(
            definitions[0].range.start,
            position_of(&provider, "factory_rule")
        );
    }
}

#[tokio::test]
async fn validation_factory_local_assignment_registers_its_literal_rule() {
    let provider = r#"<?php
namespace App\Providers;
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(): void {
        $factory = new \Illuminate\Validation\Factory();
        $factory->extend('local_factory_rule', fn ($attribute, $value) => true);
    }
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'local_factory_§rule']);"),
        &[("src/Providers/RuleProvider.php", provider)],
    )
    .await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"local_factory_rule".to_string())
    );
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_unrelated_factory_instance_does_not_register_a_rule() {
    let provider = r#"<?php
namespace App\Providers;
class Factory { public function extend($name, $callback): void {} }
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(Factory $factory): void {
        $factory->extend('unrelated_factory_rule', fn ($attribute, $value) => true);
    }
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'unrelated_factory_§rule']);"),
        &[("src/Providers/RuleProvider.php", provider)],
    )
    .await;
    assert!(
        !fixture
            .labels()
            .await
            .contains(&"unrelated_factory_rule".to_string())
    );
    let diagnostics = fixture.diagnostics("invalid_validation_rule");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_schema_references_navigate_to_migrations_on_their_connection() {
    let config = "<?php return ['default' => 'mysql', 'connections' => ['mysql' => ['driver' => 'mysql'], 'audit' => ['driver' => 'pgsql']]];";
    let migration = r#"<?php
use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
return new class extends Migration {
    public function up(): void {
        Schema::create('inventory_items', function (Blueprint $table): void {
            $table->string('stock_code');
        });
        Schema::connection('audit')->create('inventory_items', function (Blueprint $table): void {
            $table->string('event_code');
        });
    }
};
"#;
    let path = "database/migrations/2024_01_01_000000_create_inventory.php";
    for (rule, declaration, connection) in [
        (
            "exists:inventory_§items,stock_code",
            "'inventory_items'",
            "default",
        ),
        (
            "unique:inventory_items,stock_§code",
            "'stock_code'",
            "default",
        ),
        (
            "exists:mysql.inventory_§items,stock_code",
            "'inventory_items'",
            "mysql",
        ),
        (
            "unique:audit.inventory_§items,event_code",
            "'inventory_items'",
            "audit",
        ),
        (
            "exists:audit.inventory_items,event_§code",
            "'event_code'",
            "audit",
        ),
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['code' => '{rule}']);")),
            &[("config/database.php", config), (path, migration)],
        )
        .await;
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{rule}: {definitions:?}");
        assert!(
            definitions[0].uri.as_str().ends_with(path),
            "{definitions:?}"
        );
        let source = if connection == "audit" && declaration == "'inventory_items'" {
            &migration[migration.find("Schema::connection").unwrap()..]
        } else {
            migration
        };
        let offset = migration.len() - source.len() + source.find(declaration).unwrap() + 1;
        let expected = position_of(migration, &migration[offset..]);
        assert_eq!(definitions[0].range.start, expected, "{rule}");
    }
}

#[tokio::test]
async fn validation_schema_references_navigate_to_committed_sql_dumps() {
    let config = "<?php return ['default' => 'mysql', 'connections' => ['mysql' => ['driver' => 'mysql'], 'audit' => ['driver' => 'pgsql']]];";
    let mysql = "CREATE TABLE `inventory_items` (\n  `stock_code` varchar(32) NOT NULL\n);\n";
    let audit = "CREATE TABLE \"inventory_items\" (\n  \"event_code\" varchar(32) NOT NULL\n);\n";
    for (rule, path, source, declaration) in [
        (
            "unique:inventory_§items,stock_code",
            "database/schema/mysql-schema.sql",
            mysql,
            "`inventory_items`",
        ),
        (
            "exists:inventory_items,stock_§code",
            "database/schema/mysql-schema.sql",
            mysql,
            "`stock_code`",
        ),
        (
            "exists:mysql.inventory_§items,stock_code",
            "database/schema/mysql-schema.sql",
            mysql,
            "`inventory_items`",
        ),
        (
            "exists:audit.inventory_§items,event_code",
            "database/schema/audit-schema.sql",
            audit,
            "\"inventory_items\"",
        ),
        (
            "unique:audit.inventory_items,event_§code",
            "database/schema/audit-schema.sql",
            audit,
            "\"event_code\"",
        ),
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['code' => '{rule}']);")),
            &[
                ("config/database.php", config),
                ("database/schema/mysql-schema.sql", mysql),
                ("database/schema/audit-schema.sql", audit),
            ],
        )
        .await;
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{rule}: {definitions:?}");
        assert!(
            definitions[0].uri.as_str().ends_with(path),
            "{definitions:?}"
        );
        assert_eq!(
            definitions[0].range.start,
            position_of(source, declaration),
            "{rule}"
        );
    }
}

#[tokio::test]
async fn validation_missing_schema_declarations_do_not_guess_a_location() {
    let sql = "CREATE TABLE inventory_items (stock_code varchar(32));";
    for rule in [
        "exists:missing_§table,stock_code",
        "unique:inventory_items,missing_§column",
        "exists:unknown.inventory_§items,stock_code",
    ] {
        let fixture = Fixture::open(
            &request(&format!("$request->validate(['code' => '{rule}']);")),
            &[("database/schema/default-schema.sql", sql)],
        )
        .await;
        assert!(fixture.definitions().await.is_empty(), "{rule}");
    }
}

#[tokio::test]
async fn validation_deleted_sql_dump_has_no_stale_definition() {
    let path = "database/schema/default-schema.sql";
    let fixture = Fixture::open(
        &request("$request->validate(['code' => 'exists:inventory_§items,stock_code']);"),
        &[(
            path,
            "CREATE TABLE inventory_items (stock_code varchar(32));",
        )],
    )
    .await;
    assert!(
        fixture
            .labels()
            .await
            .contains(&"inventory_items".to_string())
    );
    std::fs::remove_file(fixture._dir.path().join(path)).unwrap();
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_watched_migration_edit_refreshes_columns_and_their_definition() {
    let path = "database/migrations/2024_01_01_000000_create_inventory.php";
    let before = r#"<?php
use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
return new class extends Migration {
    public function up(): void {
        Schema::create('inventory_items', function (Blueprint $table): void {
            $table->string('stock_code');
        });
    }
};
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['code' => 'exists:inventory_items,§']);"),
        &[(path, before)],
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"stock_code".to_string()), "{labels:?}");
    let after = before.replace("stock_code", "product_code");
    let migration_path = fixture._dir.path().join(path);
    std::fs::write(&migration_path, &after).unwrap();
    fixture
        .backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&migration_path).unwrap(),
                kind: FileChangeType::Changed,
            }],
        })
        .await;
    let items = fixture.items().await;
    let labels = item_labels(items.clone());
    assert!(labels.contains(&"product_code".to_string()), "{labels:?}");
    assert!(!labels.contains(&"stock_code".to_string()), "{labels:?}");
    let item = items
        .iter()
        .find(|item| item.label == "product_code")
        .unwrap();
    let content = apply_edits(&fixture.content, &[replacement(item)]);
    open_php_at(&fixture.backend, &fixture._dir, "src/Probe.php", &content).await;
    let position = position_of(&content, "product_code");
    let definitions = definition_locations(
        goto_definition_at(
            &fixture.backend,
            &fixture.uri,
            position.line,
            position.character + 2,
        )
        .await,
    );
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0].uri.as_str().ends_with(path),
        "{definitions:?}"
    );
    assert_eq!(
        definitions[0].range.start,
        position_of(&after, "product_code")
    );
}

#[tokio::test]
async fn validation_resolver_subclass_rules_navigate_to_the_declaring_ancestor() {
    let base = r#"<?php
namespace App\Validation;
class BaseValidator extends \Illuminate\Validation\Validator {
    /** Require an application storage code. */
    public function validateStorageCode($attribute, $value, $parameters): bool { return true; }
}
"#;
    let child = "<?php namespace App\\Validation; class CustomValidator extends BaseValidator {}";
    for registration in [
        "\\Illuminate\\Support\\Facades\\Validator::resolver(fn ($translator, $data, $rules, $messages) => new ApplicationValidator($translator, $data, $rules, $messages));",
        "$factory->resolver(function ($translator, $data, $rules, $messages) { return new ApplicationValidator($translator, $data, $rules, $messages); });",
        "\\Illuminate\\Support\\Facades\\Validator::resolver(fn ($translator, $data, $rules, $messages): FrameworkValidator => new ApplicationValidator($translator, $data, $rules, $messages));",
        "$factory->resolver(function ($translator, $data, $rules, $messages): FrameworkValidator { return new ApplicationValidator($translator, $data, $rules, $messages); });",
    ] {
        let provider = format!(
            r#"<?php
namespace App\Providers;
use App\Validation\CustomValidator as ApplicationValidator;
use Illuminate\Validation\Validator as FrameworkValidator;
class RuleProvider extends \Illuminate\Support\ServiceProvider {{
    public function boot(\Illuminate\Validation\Factory $factory): void {{
        {registration}
    }}
}}
"#
        );
        let fixture = Fixture::open(
            &request("$request->validate(['code' => 'storage_§code']);"),
            &[
                ("src/Validation/BaseValidator.php", base),
                ("src/Validation/CustomValidator.php", child),
                ("src/Providers/RuleProvider.php", &provider),
            ],
        )
        .await;
        let labels = fixture.labels().await;
        assert!(
            labels.contains(&"storage_code".to_string()),
            "{registration}: {labels:?}"
        );
        assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
        let hover = hover_text_at(
            &fixture.backend,
            &fixture.uri,
            fixture.position.line,
            fixture.position.character,
        )
        .await
        .expect("custom validator rule hover");
        assert!(hover.contains("application storage code"), "{hover}");
        let definitions = fixture.definitions().await;
        assert_eq!(definitions.len(), 1, "{registration}: {definitions:?}");
        assert!(
            definitions[0]
                .uri
                .as_str()
                .ends_with("/Validation/BaseValidator.php"),
            "{definitions:?}"
        );
        assert_eq!(
            definitions[0].range.start,
            position_of(base, "validateStorageCode")
        );
    }
}

#[tokio::test]
async fn validation_dynamic_resolver_keeps_the_rule_catalog_open() {
    let provider = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\Validator;
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(\Closure $resolver): void {
        Validator::resolver($resolver);
    }
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['code' => 'runtime_§rule']);"),
        &[("src/Providers/RuleProvider.php", provider)],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    assert!(!fixture.labels().await.contains(&"runtime_rule".to_string()));
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_livewire_component_calls_use_the_installed_rules() {
    let fixture = Fixture::open(
        r#"<?php
namespace App;
class Probe extends \Livewire\Component {
    public function save(): void {
        $this->validate(['name' => 'fictional_§cake']);
    }
}
"#,
        &[],
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"fictional_cake".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Concerns/ValidatesAttributes.php"),
        "{definitions:?}"
    );
}

#[tokio::test]
async fn validation_controller_inherits_validation_trait_from_its_ancestor() {
    let parent = r#"<?php
namespace App\Http;
abstract class Controller {
    use \Illuminate\Foundation\Validation\ValidatesRequests;
}
"#;
    let child = "<?php namespace App\\Http; class CheckoutController extends Controller {}";
    let extra = [
        ("src/Http/Controller.php", parent),
        ("src/Http/CheckoutController.php", child),
    ];
    let source = r#"<?php
namespace App;
class Probe extends \App\Http\CheckoutController {
    public function save(\Illuminate\Http\Request $request): void {
        $this->validate($request, ['name' => 'fictional_§cake']);
    }
}
"#;
    let fixture = Fixture::open(source, &extra).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"fictional_cake".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());

    let source = source.replace("$this->validate($request,", "$this->validate(");
    let fixture = Fixture::open(&source, &extra).await;
    assert!(
        !fixture
            .labels()
            .await
            .contains(&"fictional_cake".to_string())
    );
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
}

#[tokio::test]
async fn validation_unresolved_receivers_do_not_claim_validation_strings() {
    for declaration in ["\\Missing\\Validator", "mixed", "UnresolvedRequest"] {
        let source = format!(
            r#"<?php
namespace App;
class UnresolvedRequest extends \Missing\Request {{}}
class Probe {{
    public function save({declaration} $request): void {{
        $request->validate(['name' => 'fictional_§cake']);
    }}
}}
"#
        );
        let fixture = Fixture::open(&source, &[]).await;
        assert!(
            !fixture
                .labels()
                .await
                .contains(&"fictional_cake".to_string()),
            "{declaration}"
        );
        assert!(
            fixture.diagnostics("invalid_validation_rule").is_empty(),
            "{declaration}"
        );
        assert!(fixture.definitions().await.is_empty(), "{declaration}");
        assert!(
            hover_text_at(
                &fixture.backend,
                &fixture.uri,
                fixture.position.line,
                fixture.position.character
            )
            .await
            .is_none(),
            "{declaration}"
        );
    }
}

#[tokio::test]
async fn validation_missing_framework_rule_classes_leave_unknown_names_unjudged() {
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'runtime_only§']);"),
        &[
            (
                "vendor/illuminate/Validation/Concerns/ValidatesAttributes.php",
                "<?php",
            ),
            ("vendor/illuminate/Validation/Validator.php", "<?php"),
        ],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    assert!(!fixture.labels().await.contains(&"runtime_only".to_string()));
    assert!(fixture.definitions().await.is_empty());
}

#[tokio::test]
async fn validation_bootstrap_provider_discovers_aliased_package_helper_registrations() {
    let installed = r#"{"packages": [{
        "name": "acme/validation", "version": "1.0.0", "install-path": "../acme/validation",
        "autoload": {"psr-4": {"Acme\\Validation\\": ""}}
    }]}"#;
    let bootstrap =
        "<?php use Acme\\Validation\\RuleServiceProvider as Provider; return [Provider::class];";
    let provider = r#"<?php
namespace Acme\Validation;
use Acme\Validation\Support\RuleExtensions as Extensions;
class RuleServiceProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(): void { Extensions::register(); }
}
"#;
    let helper = r#"<?php
namespace Acme\Validation\Support;
use Illuminate\Support\Facades\{Validator as Checks};
class RuleExtensions {
    public static function register(): void {
        Checks::extend(extension: fn ($attribute, $value) => true, rule: 'warehouse_code');
        Checks::extendDependent(extension: fn ($attribute, $value, $parameters) => true, rule: 'matches_code');
    }
}
"#;
    let extra = [
        ("vendor/composer/installed.json", installed),
        ("bootstrap/providers.php", bootstrap),
        ("vendor/acme/validation/RuleServiceProvider.php", provider),
        ("vendor/acme/validation/Support/RuleExtensions.php", helper),
    ];
    let fixture = Fixture::open(
        &request("$request->validate(['code' => 'warehouse_§code']);"),
        &extra,
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"warehouse_code".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Support/RuleExtensions.php"),
        "{definitions:?}"
    );
    assert_eq!(
        definitions[0].range.start,
        position_of(helper, "warehouse_code")
    );

    let fixture = Fixture::open(
        &request("$request->validate(['stock_code' => 'required', 'confirmation' => 'matches_code:stock_§code']);"),
        &extra,
    ).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"stock_code".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert_eq!(definitions[0].uri, fixture.uri);
    assert_eq!(
        definitions[0].range.start,
        position_of(&fixture.content, "stock_code' =>")
    );
}

#[tokio::test]
async fn validation_unresolved_bootstrap_provider_keeps_unknown_rules_open() {
    let fixture = Fixture::open(
        &request("$request->validate(['name' => 'runtime_§rule']);"),
        &[(
            "bootstrap/providers.php",
            "<?php return [\\Missing\\ValidationProvider::class];",
        )],
    )
    .await;
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    assert!(!fixture.labels().await.contains(&"runtime_rule".to_string()));
}

#[tokio::test]
async fn validation_rule_class_completion_respects_the_typed_prefix() {
    let fixture = Fixture::open(
        &request("$request->validate(['name' => ['fic§']]);"),
        &[("src/Rules/UppercaseRule.php", CUSTOM_RULE)],
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"fictional_cake".to_string()), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.contains("UppercaseRule")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn validation_sometimes_accepts_the_concrete_validator_without_a_contract_stub() {
    let validator = VALIDATOR.replace(
        " implements \\Illuminate\\Contracts\\Validation\\Validator",
        "",
    );
    let fixture = Fixture::open(
        &request("$validator = Validator::make([], []); $validator->sometimes('name', 'fictional_§cake', fn () => true);"),
        &[("vendor/illuminate/Validation/Validator.php", &validator)],
    ).await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"fictional_cake".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Concerns/ValidatesAttributes.php"),
        "{definitions:?}"
    );
}

#[tokio::test]
async fn validation_resolver_rules_navigate_to_methods_declared_on_the_returned_class() {
    let validator = r#"<?php
namespace App\Validation;
class CustomValidator extends \Illuminate\Validation\Validator {
    /** Check the application's delivery reference. */
    public function validateDeliveryCode($attribute, $value): bool { return true; }
}
"#;
    let provider = r#"<?php
namespace App\Providers;
use App\Validation\CustomValidator;
use Illuminate\Support\Facades\Validator;
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(): void {
        Validator::resolver(fn ($translator, $data, $rules, $messages) => new CustomValidator($translator, $data, $rules, $messages));
    }
}
"#;
    let fixture = Fixture::open(
        &request("$request->validate(['code' => 'delivery_§code']);"),
        &[
            ("src/Validation/CustomValidator.php", validator),
            ("src/Providers/RuleProvider.php", provider),
        ],
    )
    .await;
    let labels = fixture.labels().await;
    assert!(labels.contains(&"delivery_code".to_string()), "{labels:?}");
    assert!(fixture.diagnostics("invalid_validation_rule").is_empty());
    let definitions = fixture.definitions().await;
    assert_eq!(definitions.len(), 1, "{definitions:?}");
    assert!(
        definitions[0]
            .uri
            .as_str()
            .ends_with("/Validation/CustomValidator.php"),
        "{definitions:?}"
    );
    assert_eq!(
        definitions[0].range.start,
        position_of(validator, "validateDeliveryCode")
    );
}

#[tokio::test]
async fn validation_undecodable_message_and_attribute_keys_remain_unjudged() {
    for key in [r#""email.\u{110000}§""#, r#""\xFF§""#] {
        for method in ["messages", "attributes"] {
            let fixture = Fixture::open(
                &form(&format!("public function rules(): array {{ return ['email' => 'required']; }} public function {method}(): array {{ return [{key} => 'Label']; }}")),
                &[],
            ).await;
            let diagnostics = fixture.diagnostics("invalid_validation_message");
            assert!(diagnostics.is_empty(), "{method} {key}: {diagnostics:?}");
        }
    }
}

#[tokio::test]
async fn validation_unavailable_indexed_registration_sources_keep_the_catalog_open() {
    let registration = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\Validator;
class RuleProvider extends \Illuminate\Support\ServiceProvider {
    public function boot(): void { Validator::extend('registered_code', fn ($attribute, $value) => true); }
}
"#;
    for bootstrap in [
        "<?php return [];",
        "<?php return [\\App\\Providers\\RuleProvider::class];",
    ] {
        let path = "src/Providers/RuleProvider.php";
        let mut fixture = Fixture::open(
            "<?php $placeholder = '§';",
            &[(path, registration), ("bootstrap/providers.php", bootstrap)],
        )
        .await;
        let source_uri = open_php_at(&fixture.backend, &fixture._dir, path, registration).await;
        fixture
            .backend
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: source_uri },
            })
            .await;
        std::fs::remove_file(fixture._dir.path().join(path)).unwrap();
        let marked = request("$request->validate(['code' => 'runtime_§rule']);");
        fixture.position = position_of(&marked, "§");
        fixture.content = marked.replace('§', "");
        open_php_at(
            &fixture.backend,
            &fixture._dir,
            "src/Probe.php",
            &fixture.content,
        )
        .await;
        assert!(
            fixture.diagnostics("invalid_validation_rule").is_empty(),
            "{bootstrap}"
        );
        assert!(
            !fixture
                .labels()
                .await
                .contains(&"registered_code".to_string())
        );
        assert!(fixture.definitions().await.is_empty());
    }
}

#[tokio::test]
async fn validation_missing_parser_and_alias_targets_do_not_invent_rules() {
    let unknown_target = r#"<?php
namespace Illuminate\Validation;
class ValidationRuleParser {
    protected static function normalizeRule(string $rule): string {
        return match ($rule) { 'Unavailable' => 'VersionSpecificRule', default => $rule };
    }
}
"#;
    for parser in ["<?php", unknown_target] {
        let fixture = Fixture::open(
            &request("$request->validate(['name' => 'unavailable§']);"),
            &[(
                "vendor/illuminate/Validation/ValidationRuleParser.php",
                parser,
            )],
        )
        .await;
        assert!(!fixture.labels().await.contains(&"unavailable".to_string()));
        let diagnostics = fixture.diagnostics("invalid_validation_rule");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].contains("unavailable"), "{diagnostics:?}");
        assert!(fixture.definitions().await.is_empty());
    }
}
