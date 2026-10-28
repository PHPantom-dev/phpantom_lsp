use super::*;

fn keys(rules: &RulesArray) -> Vec<&str> {
    rules.entries.iter().map(|r| r.key.as_str()).collect()
}

#[test]
fn parses_form_request_rules_method() {
    let content = "<?php
namespace App\\Http\\Requests;
class StoreUserRequest extends FormRequest {
    public function rules(): array {
        return [
            'name' => 'required|string|max:255',
            'age' => ['nullable', 'integer'],
        ];
    }
}
";
    let rules = rules_from_class_source(content, "App\\Http\\Requests\\StoreUserRequest");
    assert_eq!(keys(&rules), vec!["name", "age"]);
    assert_eq!(rules.entries[0].rules, "required|string|max:255");
    assert_eq!(rules.entries[1].rules, "nullable|integer");
}

#[test]
fn follows_array_merge_in_rules_method() {
    let content = "<?php
class UpdateUserRequest extends FormRequest {
    public function rules(): array {
        return array_merge(parent::rules(), [
            'email' => 'required|email',
        ]);
    }
}
";
    let rules = rules_from_class_source(content, "UpdateUserRequest");
    assert_eq!(keys(&rules), vec!["email"]);
}

#[test]
fn parses_rules_method_declared_in_a_trait() {
    let content = "<?php
namespace App\\Http\\Requests\\Concerns;
trait HasPostRules {
    public function rules(): array {
        return ['headline' => 'required|string'];
    }
}
";
    let rules = rules_from_class_source(content, "App\\Http\\Requests\\Concerns\\HasPostRules");
    assert_eq!(keys(&rules), vec!["headline"]);
}

#[test]
fn ignores_rules_method_of_another_class() {
    let content = "<?php
class OtherRequest extends FormRequest {
    public function rules(): array { return ['other' => 'required']; }
}
class StoreUserRequest extends FormRequest {
    public function rules(): array { return ['name' => 'required']; }
}
";
    assert_eq!(
        keys(&rules_from_class_source(content, "StoreUserRequest")),
        vec!["name"]
    );
}

#[test]
fn renders_non_literal_rule_entries_from_source() {
    let content = "<?php
class StoreUserRequest extends FormRequest {
    public function rules(): array {
        return [
            'role' => [
                'required',
                new Enum(Role::class),
            ],
        ];
    }
}
";
    let rules = rules_from_class_source(content, "StoreUserRequest");
    assert_eq!(rules.entries[0].rules, "required|new Enum(Role::class)");
}

#[test]
fn an_enum_rule_is_resolved_against_the_declaring_files_imports() {
    // Resolution happens during the parse that recovered the rules, so the
    // entry already carries the enum's FQN.
    let content = "<?php
namespace App\\Http\\Requests;
use App\\Enums\\Role;
use Illuminate\\Validation\\Rules\\Enum;
class StoreUserRequest extends FormRequest {
    public function rules(): array {
        return ['role' => ['required', new Enum(Role::class)]];
    }
}
";
    let rules = rules_from_class_source(content, "App\\Http\\Requests\\StoreUserRequest");
    assert_eq!(
        rules.entries[0].enum_class.as_deref(),
        Some("App\\Enums\\Role")
    );
}

#[test]
fn enum_factories_and_targets_resolve_from_the_original_source_range() {
    let array = r"['role' => [Checks::enum(type: Role::class)->only([])], 'status' => new EnumRule(type: State::class)]";
    let content = format!(
        r"<?php
namespace Requests {{
    use Illuminate\Validation\{{Rule as Checks, Rules\Enum as EnumRule}};
    use Domain\{{Role, Status as State}};
    $rules = {array};
}}
namespace Caller {{ use Other\Role; }}
"
    );
    let start = content.find(array).unwrap();
    let rules = rules_from_source_range(&content, start, start + array.len()).unwrap();
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Domain\\Role"));
    assert_eq!(
        rules.entries[1].enum_class.as_deref(),
        Some("Domain\\Status")
    );
    assert_eq!(
        &content[rules.entries[0].key_start..rules.entries[0].key_start + 4],
        "role"
    );
    assert_eq!(
        &content[rules.entries[1].key_start..rules.entries[1].key_start + 6],
        "status"
    );
}

#[test]
fn unrelated_enum_factories_do_not_supply_enum_types() {
    let array = r"['a' => Rule::enum(Role::class), 'b' => new Enum(Role::class), 'c' => \App\Rule::enum(Role::class), 'd' => new \App\Enum(Role::class)]";
    for imports in ["", "use App\\Rule; use App\\Enum;"] {
        let content = format!("<?php namespace App; {imports} $rules = {array};");
        let start = content.find(array).unwrap();
        let rules = rules_from_source_range(&content, start, start + array.len()).unwrap();
        assert!(rules.entries.iter().all(|entry| entry.enum_class.is_none()));
    }
}

#[test]
fn enum_argument_context_uses_its_namespace_block_and_global_imports() {
    let content = r"<?php
namespace First {
    use Illuminate\Validation\Rule as Checks;
    use Domain\FirstRole as Role;
    function first() {}
}
namespace Second {
    use Illuminate\Validation\Rule as Checks;
    function second() {}
}
namespace {
    use Illuminate\Validation\Rule as Checks;
    function global_call() {}
}
";
    for (site, expected) in [
        ("function first", "Domain\\FirstRole"),
        ("function second", "Second\\Role"),
        ("function global_call", "Role"),
    ] {
        let rules = rules_from_array_text_in_context(
            "['role' => Checks::enum(Role::class)]",
            content,
            content.find(site).unwrap() as u32,
        )
        .unwrap();
        assert_eq!(rules.entries[0].enum_class.as_deref(), Some(expected));
    }
    let global = r"<?php use Illuminate\Validation\Rule as Checks;";
    let rules = rules_from_array_text_in_context(
        "['role' => Checks::enum(Role::class)]",
        global,
        global.len() as u32,
    )
    .unwrap();
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Role"));
    let content = r"<?php namespace First; use Illuminate\Validation\Rule as Checks; namespace Second; use Illuminate\Validation\Rule as Checks;";
    let rules = rules_from_array_text_in_context(
        "['role' => Checks::enum(Role::class)]",
        content,
        content.len() as u32,
    )
    .unwrap();
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Second\\Role"));
}

#[test]
fn source_range_rules_accept_legacy_arrays_and_reject_nonarrays() {
    let array =
        r"array('role' => (new \Illuminate\Validation\Rules\Enum(\Domain\Role::class))?->only([]))";
    let content = format!("<?php $rules = {array};");
    let start = content.find(array).unwrap();
    let rules = rules_from_source_range(&content, start, start + array.len()).unwrap();
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Domain\\Role"));
    assert!(rules_from_source_range(&content, start, start).is_none());
    assert!(rules_from_source_range(&content, 0, content.len() + 1).is_none());
    assert!(rules_from_source_range(&content, 0, content.len()).is_none());
    assert!(rules_from_source_range(&content, start + 1, start + array.len()).is_none());
}

#[test]
fn source_range_rules_keep_composed_and_parenthesized_enum_declarations() {
    for expression in [
        "array_merge(['role' => Checks::enum(Role::class)], ['name' => 'required'])",
        "(array_merge(['role' => Checks::enum(Role::class)], ['name' => 'required']))",
        "(['role' => Checks::enum(Role::class), 'name' => 'required'])",
    ] {
        let content = format!(
            r"<?php namespace App;
use Illuminate\Validation\Rule as Checks;
use Domain\Role;
$rules = {expression};
"
        );
        let start = content.find(expression).unwrap();
        let rules = rules_from_source_range(&content, start, start + expression.len()).unwrap();
        assert_eq!(keys(&rules), ["role", "name"]);
        assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Domain\\Role"));
        assert_eq!(
            &content[rules.entries[0].key_start..rules.entries[0].key_start + 4],
            "role"
        );
    }
}

#[test]
fn request_rules_select_the_exact_declaration_namespace() {
    let source = r"<?php
namespace First {
    use Illuminate\Validation\Rule;
    class Request { public function rules() { return ['role' => Rule::enum(Role::class)]; } }
}
namespace Second {
    use Illuminate\Validation\Rule;
    class Request { public function rules() { return ['role' => Rule::enum(Role::class)]; } }
}
namespace {
    class Request { public function rules() { return ['name' => 'string']; } }
}
";
    let first = rules_from_class_source(source, "First\\Request");
    let second = rules_from_class_source(source, "Second\\Request");
    assert_eq!(first.entries[0].enum_class.as_deref(), Some("First\\Role"));
    assert_eq!(
        second.entries[0].enum_class.as_deref(),
        Some("Second\\Role")
    );
    assert_eq!(keys(&rules_from_class_source(source, "Request")), ["name"]);
}

#[test]
fn inline_rules_resolve_aliased_enum_factories_in_their_own_file() {
    let source = r"<?php
namespace App;
use Illuminate\Validation\Rule as Checks;
use Domain\Role;
function validate_input($request) {
    $request->validate(['role' => Checks::enum(Role::class)]);
    $request->input('role');
}
";
    let rules = inline_validate_rules(source, source.find("$request->input").unwrap()).unwrap();
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("Domain\\Role"));
}

#[test]
fn key_offsets_point_inside_the_quotes() {
    let content = "<?php
class StoreUserRequest extends FormRequest {
    public function rules(): array { return ['name' => 'required']; }
}
";
    let rules = rules_from_class_source(content, "StoreUserRequest");
    assert_eq!(
        &content[rules.entries[0].key_start..rules.entries[0].key_start + 4],
        "name"
    );
}

#[test]
fn finds_inline_validate_call() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->validate([
            'title' => 'required|string',
            'body' => 'required',
        ]);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).expect("should find validate() rules");
    assert_eq!(keys(&rules), vec!["title", "body"]);
}

#[test]
fn finds_validates_requests_trait_form() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $this->validate($request, ['title' => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).expect("should find $this->validate rules");
    assert_eq!(keys(&rules), vec!["title"]);
}

#[test]
fn finds_validator_make_rules() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $validator = Validator::make($request->all(), ['title' => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).expect("should find Validator::make rules");
    assert_eq!(keys(&rules), vec!["title"]);
}

#[test]
fn validator_make_ignores_the_data_argument() {
    let content = "<?php
class UserController {
    public function store() {
        $validator = Validator::make(['title' => 'Hi'], ['headline' => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).expect("should find rules");
    assert_eq!(keys(&rules), vec!["headline"]);
}

#[test]
fn ignores_validate_calls_after_the_cursor() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->input('');
        $request->validate(['title' => 'required']);
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    assert!(inline_validate_rules(content, cursor).is_none());
}

#[test]
fn ignores_validate_calls_in_a_sibling_method() {
    let content = "<?php
class UserController {
    public function update(Request $request) {
        $request->validate(['title' => 'required']);
    }
    public function store(Request $request) {
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    assert!(inline_validate_rules(content, cursor).is_none());
}

#[test]
fn rules_reach_into_a_closure_nested_in_the_same_method() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->validate(['title' => 'required']);
        DB::transaction(function () use ($request) {
            $request->input('');
        });
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).expect("closure is still the same method");
    assert_eq!(keys(&rules), vec!["title"]);
}

#[test]
fn prefers_the_nearest_preceding_validate_call() {
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->validate(['first' => 'required']);
        $request->validate(['second' => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).unwrap();
    assert_eq!(keys(&rules), vec!["second"]);
}

#[test]
fn a_validate_call_in_each_branch_contributes_its_keys() {
    // Either arm may have run, so both describe the request at the cursor.
    let content = "<?php
class UserController {
    public function store(Request $request) {
        if ($request->has('draft')) {
            $request->validate(['first' => 'required']);
        } else {
            $request->validate(['second' => 'required']);
        }
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).unwrap();
    assert_eq!(keys(&rules), vec!["first", "second"]);
    assert!(rules.keys_complete);
}

#[test]
fn an_unconditional_call_after_a_branch_takes_over() {
    // The last call cannot have been skipped, so it alone is in force.
    let content = "<?php
class UserController {
    public function store(Request $request) {
        if ($request->has('draft')) {
            $request->validate(['first' => 'required']);
        }
        $request->validate(['second' => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).unwrap();
    assert_eq!(keys(&rules), vec!["second"]);
}

#[test]
fn a_call_in_a_loop_does_not_displace_an_earlier_one() {
    // The loop body may never run, so the earlier call can still be the one
    // in force.
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->validate(['first' => 'required']);
        foreach ($items as $item) {
            $request->validate(['second' => 'required']);
        }
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).unwrap();
    assert_eq!(keys(&rules), vec!["first", "second"]);
}

#[test]
fn an_unreadable_nearer_call_does_not_hand_back_the_earlier_one() {
    // The second call is the one in force.  Its keys cannot be read, so the
    // answer is "an incomplete set", never the first call's keys — those
    // would describe a different request and, as a shape, claim to be
    // complete.
    let content = "<?php
class UserController {
    public function store(Request $request) {
        $request->validate(['first' => 'required']);
        $request->validate([$dynamic => 'required']);
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    let rules = inline_validate_rules(content, cursor).unwrap();
    assert!(keys(&rules).is_empty());
    assert!(!rules.keys_complete);
}

#[test]
fn non_literal_keys_are_skipped() {
    let content = "<?php
class StoreUserRequest extends FormRequest {
    public function rules(): array {
        return [
            'name' => 'required',
            $dynamic => 'required',
        ];
    }
}
";
    assert_eq!(
        keys(&rules_from_class_source(content, "StoreUserRequest")),
        vec!["name"]
    );
}

// ─── Field expansion ────────────────────────────────────────────────────────

fn rule(key: &str) -> ValidationRule {
    ValidationRule {
        key: key.to_string(),
        rules: "required".to_string(),
        key_start: 0,
        key_end: key.len(),
        origin: None,
        enum_class: None,
    }
}

#[test]
fn wildcard_keys_collapse_to_their_root() {
    let rules = vec![rule("items"), rule("items.*.id")];
    let fields = rule_fields(&rules);
    let names: Vec<&String> = fields.iter().map(|f| &f.name).collect();
    assert_eq!(names, vec!["items"]);
}

#[test]
fn wildcard_root_is_offered_even_without_its_own_rule() {
    let rules = vec![rule("items.*.id")];
    let fields = rule_fields(&rules);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "items");
    assert!(fields[0].rules.is_empty());
}

#[test]
fn plain_dotted_keys_are_offered_whole_and_by_root() {
    let rules = vec![rule("address.city")];
    let fields = rule_fields(&rules);
    let names: Vec<&String> = fields.iter().map(|f| &f.name).collect();
    assert_eq!(names, vec!["address.city", "address"]);
    assert_eq!(fields[0].rules, "required");
    assert!(fields[1].rules.is_empty());
}

// ─── `safe()` provenance ────────────────────────────────────────────────────

#[test]
fn traces_a_safe_variable_back_to_its_request() {
    let content = "<?php
class UserController {
    public function store(StoreUserRequest $request) {
        $safe = $request->safe();
        $safe->only(['']);
    }
}
";
    let cursor = content.find("only(['").unwrap() + 7;
    assert_eq!(
        safe_source_variable(content, cursor, "$safe").as_deref(),
        Some("$request")
    );
}

#[test]
fn safe_variable_takes_the_nearest_preceding_assignment() {
    let content = "<?php
class UserController {
    public function store($first, $second) {
        $safe = $first->safe();
        $safe = $second->safe();
        $safe->only(['']);
    }
}
";
    let cursor = content.find("only(['").unwrap() + 7;
    assert_eq!(
        safe_source_variable(content, cursor, "$safe").as_deref(),
        Some("$second")
    );
}

#[test]
fn safe_assignment_after_the_cursor_is_ignored() {
    let content = "<?php
class UserController {
    public function store($request) {
        $safe->only(['']);
        $safe = $request->safe();
    }
}
";
    let cursor = content.find("only(['").unwrap() + 7;
    assert_eq!(safe_source_variable(content, cursor, "$safe"), None);
}

#[test]
fn safe_assignment_in_a_sibling_method_is_ignored() {
    let content = "<?php
class UserController {
    public function prepare($request) {
        $safe = $request->safe();
    }
    public function store() {
        $safe->only(['']);
    }
}
";
    let cursor = content.find("only(['").unwrap() + 7;
    assert_eq!(safe_source_variable(content, cursor, "$safe"), None);
}

#[test]
fn a_non_safe_assignment_is_not_traced() {
    let content = "<?php
class UserController {
    public function store($request) {
        $safe = $request->validated();
        $safe->only(['']);
    }
}
";
    let cursor = content.find("only(['").unwrap() + 7;
    assert_eq!(safe_source_variable(content, cursor, "$safe"), None);
}

// ─── Pre-filter ─────────────────────────────────────────────────────────────

#[test]
fn a_file_without_validation_is_not_parsed_for_rules() {
    let content = "<?php
class UserController {
    public function store($request) {
        $request->input('');
    }
}
";
    let cursor = content.find("input('").unwrap() + 7;
    assert!(!mentions_validation(content));
    assert!(inline_validate_rules(content, cursor).is_none());
}

#[test]
fn the_pre_filter_admits_every_recognised_spelling() {
    assert!(mentions_validation("$request->validate([]);"));
    assert!(mentions_validation("$this->validateWithBag('b', []);"));
    assert!(mentions_validation("Validator::make($data, []);"));
}

#[test]
fn legacy_rule_arrays_resolve_enum_imports_and_keep_source_offsets() {
    let content = r#"<?php
namespace App;
use Illuminate\Validation\Rule as Rules;
use App\Enums\State;
$request->validate(array('state' => array('required', Rules::enum(State::class))));
"#;
    let start = content.find("array('state'").unwrap();
    let end = content.rfind(");").unwrap();
    let rules = rules_from_source_range(content, start, end).unwrap();
    assert_eq!(keys(&rules), ["state"]);
    assert_eq!(
        rules.entries[0].enum_class.as_deref(),
        Some("App\\Enums\\State")
    );
    assert_eq!(
        rules.entries[0].key_start,
        content.find("'state'").unwrap() + 1
    );
}

#[test]
fn qualified_array_merge_keeps_enum_metadata_and_field_origins() {
    let content = r#"<?php
namespace App;
use Illuminate\Validation\Rule;
$request->validate(\array_merge(['name' => 'required'], ['state' => [Rule::enum(State::class), 'in:active']]));
"#;
    let start = content.find("\\array_merge").unwrap();
    let end = content.rfind(");").unwrap();
    let rules = rules_from_source_range(content, start, end).unwrap();
    assert!(rules.keys_complete);
    assert_eq!(keys(&rules), ["name", "state"]);
    assert_eq!(rules.entries[1].enum_class.as_deref(), Some("App\\State"));
    assert_eq!(
        rules.entries[1].key_start,
        content.find("'state'").unwrap() + 1
    );
}

#[test]
fn decoded_rule_keys_keep_source_spans_and_keyed_enum_values() {
    let content = r#"<?php
namespace App;
use Illuminate\Validation\Rule as Rules;
use Domain\Role;
$rules = ["\u{72}ole" => ['enum' => Rules::enum(Role::class), 'presence' => "\x72equired"]];
"#;
    let start = content.find("[\"").unwrap();
    let end = content.rfind(']').unwrap() + 1;
    let actual =
        crate::parser::with_parsed_program(content, "decoded_validation_rules", |program, _| {
            Some((
                rules_from_program_range(program, content, start, end)?,
                rules_from_program_range(program, content, end, start).is_none(),
                rules_from_program_range(program, content, start, content.len() + 1).is_none(),
            ))
        });
    let (rules, rejects_reversed, rejects_out_of_bounds) =
        actual.expect("rules parsed without panicking");
    assert_eq!(keys(&rules), ["role"]);
    let field = &rules.entries[0];
    assert_eq!(&content[field.key_start..field.key_end], r"\u{72}ole");
    assert_eq!(field.rules, "Rules::enum(Role::class)|required");
    assert_eq!(field.enum_class.as_deref(), Some("Domain\\Role"));
    assert!(rejects_reversed);
    assert!(rejects_out_of_bounds);
}

#[test]
fn equivalent_escaped_field_keys_keep_the_last_rules() {
    let source = r#"<?php
use Illuminate\Validation\Rule;
$rules = ['role' => Rule::enum(FirstRole::class), "\x72ole" => ['named' => Rule::enum(FinalRole::class)]];
"#;
    let start = source.find("['role'").unwrap();
    let end = source.rfind(']').unwrap() + 1;
    let rules = rules_from_source_range(source, start, end).unwrap();
    assert_eq!(keys(&rules), ["role"]);
    assert_eq!(rules.entries[0].enum_class.as_deref(), Some("FinalRole"));
    assert_eq!(
        &source[rules.entries[0].key_start..rules.entries[0].key_end],
        r"\x72ole"
    );
}
