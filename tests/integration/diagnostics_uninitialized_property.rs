use crate::common::{
    collect_diagnostics_with, create_psr4_workspace, create_test_backend, message_text,
};
use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use tower_lsp_server::gen_lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect_on(backend: &Backend, php: &str) -> Vec<String> {
    collect_diagnostics_with(backend, php, Backend::collect_slow_diagnostics)
        .into_iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, Code::String(s) if s == "uninitialized_property"))
        })
        .map(|d| {
            assert_eq!(d.severity, Some(DiagnosticSeverity::Warning));
            message_text(&d.message).to_string()
        })
        .collect()
}

fn collect(php: &str) -> Vec<String> {
    collect_on(&create_test_backend(), php)
}

// ─── Reported ───────────────────────────────────────────────────────────────

#[test]
fn a_private_property_nothing_writes_is_reported() {
    let php = r#"<?php
namespace App;

final class User
{
    private string $name;

    public function name(): string
    {
        return $this->name;
    }
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property App\\User::$name is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

#[test]
fn the_diagnostic_covers_the_property_variable() {
    let php = "<?php\nfinal class User { private string $name; }\n";
    let diags = collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    );
    let diag = diags
        .iter()
        .find(|d| matches!(&d.code, Some(Code::String(s)) if s == "uninitialized_property"))
        .expect("diagnostic");
    assert_eq!(diag.range.start, Position::new(1, 34));
    assert_eq!(diag.range.end, Position::new(1, 39));
}

#[test]
fn a_readonly_property_says_it_must_be_assigned_in_the_constructor() {
    let php = r#"<?php
final class Point
{
    public readonly int $x;
    public readonly int $y;

    public function __construct(int $x)
    {
        $this->x = $x;
    }
}
"#;
    assert_eq!(
        collect(php),
        vec!["Readonly property Point::$y is never initialized; assign it in the constructor"]
    );
}

#[test]
fn a_property_of_a_readonly_class_is_readonly() {
    let php = "<?php\nfinal readonly class Point { public int $x; }\n";
    assert_eq!(
        collect(php),
        vec!["Readonly property Point::$x is never initialized; assign it in the constructor"]
    );
}

#[test]
fn a_protected_property_of_a_final_class_is_reported() {
    let php = "<?php\nfinal class Box { protected int $size; }\n";
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn an_asymmetric_private_set_property_is_reported() {
    let php = "<?php\nclass Box { public private(set) int $size; }\n";
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn a_private_property_of_an_abstract_class_is_reported() {
    let php = "<?php\nabstract class Base { private int $size; }\n";
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn an_anonymous_class_property_is_reported() {
    let php = "<?php\n$box = new class { protected int $size; };\n";
    assert_eq!(
        collect(php),
        vec![
            "Property $size is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

#[test]
fn a_constructor_that_skips_the_parent_constructor_does_not_save_the_property() {
    let php = r#"<?php
class Base { public function __construct() {} }

final class Child extends Base
{
    private int $size;

    public function __construct() {}
}
"#;
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn a_readonly_property_of_a_non_final_class_is_reported_before_php_8_4() {
    // Until PHP 8.4 only the declaring class may initialize a readonly
    // property, so no subclass can do it.
    let php = "<?php\nclass Point { public readonly int $x; }\n";
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 3));
    assert_eq!(collect_on(&backend, php).len(), 1);

    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 4));
    assert!(collect_on(&backend, php).is_empty());
}

// ─── Initialized ────────────────────────────────────────────────────────────

#[test]
fn a_write_from_any_method_counts() {
    let php = r#"<?php
final class Service
{
    private string $promoted;
    private string $inConstructor;
    private string $bySetter;
    private ?Service $lazy;
    private string $byFactory;
    private array $byAppend;
    private int $byIncrement;
    private string $byDestructuring;
    private string $byForeach;

    public function __construct(private string $promotedParam)
    {
        $this->inConstructor = 'a';
    }

    public function setBySetter(string $value): void { $this->bySetter = $value; }

    public function lazy(): Service { return $this->lazy ??= new Service(''); }

    public static function make(): self
    {
        $self = new self('');
        $self->byFactory = 'b';
        return $self;
    }

    public function more(array $rows): void
    {
        $this->byAppend[] = 1;
        $this->byIncrement++;
        [$this->byDestructuring] = $rows;
        foreach ($rows as $this->byForeach) {}
    }
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property Service::$promoted is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

#[test]
fn properties_without_a_type_or_with_a_default_are_not_reported() {
    let php = r#"<?php
final class Box
{
    private $untyped;
    private int $withDefault = 0;
    private ?int $nullableWithDefault = null;
    private static int $static;
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_public_property_is_left_to_outside_code() {
    let php = r#"<?php
final class Options { public string $host; }

$options = new Options();
$options->host = 'localhost';
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_protected_property_of_a_non_final_class_is_left_to_subclasses() {
    let php = r#"<?php
class Base { protected string $name; }
final class Child extends Base { public function __construct() { $this->name = 'child'; } }
"#;
    assert!(collect(php).is_empty());
}

// ─── Initialized by something the check cannot see ──────────────────────────

#[test]
fn a_dynamic_property_write_may_initialize_anything() {
    let php = r#"<?php
final class Dto
{
    private string $name;

    public function __construct(array $data)
    {
        foreach ($data as $key => $value) {
            $this->$key = $value;
        }
    }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn passing_this_to_a_call_may_initialize_anything() {
    let php = r#"<?php
function hydrate(object $target): void {}

final class Dto
{
    private string $name;

    public function __construct() { hydrate($this); }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn an_inherited_constructor_may_initialize_anything() {
    let php = r#"<?php
class Base { public function __construct() {} }

final class Implicit extends Base { private int $size; }

final class Explicit extends Base
{
    private int $size;

    public function __construct() { parent::__construct(); }
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_class_using_a_trait_is_skipped() {
    let php = r#"<?php
trait InitializesSize { public function init(): void { $this->size = 1; } }

final class Box
{
    use InitializesSize;

    private int $size;
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn an_attribute_from_a_library_marks_the_property_as_filled() {
    let php = r#"<?php
namespace App;

use DI\Attribute\Inject;

final class Controller
{
    #[Inject]
    private Logger $logger;
}
"#;
    assert!(collect(php).is_empty());
}

#[test]
fn a_php_attribute_does_not_mark_the_property_as_filled() {
    let php = r#"<?php
namespace App;

class Base { protected int $size = 0; }

final class Child extends Base
{
    #[\Override]
    private int $other;
}
"#;
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn an_injection_docblock_tag_marks_the_property_as_filled() {
    let php = r#"<?php
final class Presenter
{
    /** @inject */
    private Logger $logger;

    /** @ORM\Column(type="string") */
    private string $name;

    /** @var string */
    private string $plain;
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property Presenter::$plain is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

// ─── Doctrine mappings ──────────────────────────────────────────────────────

#[test]
fn doctrine_fills_generated_and_version_fields_only() {
    let php = r#"<?php
namespace App;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class Product
{
    #[ORM\Id, ORM\GeneratedValue, ORM\Column]
    private int $id;

    #[ORM\Version, ORM\Column]
    private int $version;

    #[ORM\Column(generated: 'ALWAYS')]
    private string $slug;

    #[ORM\Column]
    private string $name;
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property App\\Product::$name is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

#[test]
fn a_natural_doctrine_identifier_is_not_generated() {
    let php = r#"<?php
use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class Country
{
    #[ORM\Id, ORM\GeneratedValue(strategy: 'NONE'), ORM\Column]
    private string $code;
}
"#;
    assert_eq!(collect(php).len(), 1);
}

#[test]
fn doctrine_fills_a_read_only_entity_without_a_constructor() {
    let php = r#"<?php
use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity(readOnly: true)]
class Snapshot
{
    #[ORM\Id, ORM\Column]
    private int $id;
}

#[ORM\Entity(readOnly: true)]
class SnapshotWithConstructor
{
    #[ORM\Id, ORM\Column]
    private int $id;

    public function __construct() {}
}
"#;
    assert_eq!(
        collect(php),
        vec![
            "Property SnapshotWithConstructor::$id is never initialized; give it a default value or assign it in the constructor"
        ]
    );
}

// ─── Reads of a fresh object ────────────────────────────────────────────────

const OPTIONS: &str = r#"<?php
namespace App;

final class Options
{
    public string $host;
    public ?string $path;
    public array $tags;
    public int $port = 80;
}
"#;

fn read_message(property: &str) -> String {
    format!("Property App\\Options::${property} is read before it is initialized")
}

fn collect_reads(body: &str) -> Vec<String> {
    collect(&format!("{OPTIONS}\n{body}"))
}

#[test]
fn reading_a_fresh_object_property_before_any_write_is_reported() {
    assert_eq!(
        collect_reads("$options = new Options();\necho $options->host;\n"),
        vec![read_message("host")]
    );
}

#[test]
fn the_read_diagnostic_covers_the_property_access() {
    let php = format!("{OPTIONS}\n$options = new Options();\necho $options->host;\n");
    let diags = collect_diagnostics_with(
        &create_test_backend(),
        &php,
        Backend::collect_slow_diagnostics,
    );
    let diag = diags
        .iter()
        .find(|d| matches!(&d.code, Some(Code::String(s)) if s == "uninitialized_property"))
        .expect("diagnostic");
    assert_eq!(diag.range.start, Position::new(12, 5));
    assert_eq!(diag.range.end, Position::new(12, 19));
}

#[test]
fn reads_inside_a_function_and_through_nullsafe_access_are_reported() {
    let body = r#"
function connect(): string
{
    $options = new Options();
    return $options?->host;
}
"#;
    assert_eq!(collect_reads(body), vec![read_message("host")]);
}

#[test]
fn only_the_first_read_of_a_property_is_reported() {
    assert_eq!(
        collect_reads("$options = new Options();\necho $options->host;\necho $options->host;\n"),
        vec![read_message("host")]
    );
}

#[test]
fn a_compound_assignment_or_increment_reads_the_property() {
    let body = r#"
$options = new Options();
$options->host .= ':80';
$options->tags[] = 'web';
"#;
    assert_eq!(collect_reads(body), vec![read_message("host")]);
}

#[test]
fn a_write_before_the_read_initializes_the_property() {
    let body = r#"
$options = new Options();
$options->host = 'localhost';
$options->tags[] = 'web';
[$options->path] = ['/'];
echo $options->host, $options->path, count($options->tags);
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn the_assigned_value_is_read_before_the_write() {
    let body = r#"
$options = new Options();
$options->host = $options->host . ':80';
"#;
    assert_eq!(collect_reads(body), vec![read_message("host")]);
}

#[test]
fn a_write_in_a_branch_counts() {
    let body = r#"
$options = new Options();
if (rand(0, 1)) {
    $options->host = 'localhost';
}
echo $options->host;
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn a_write_later_in_a_loop_body_counts() {
    let body = r#"
$options = new Options();
foreach ([1, 2] as $i) {
    if ($i > 1) {
        echo $options->host;
    }
    $options->host = 'localhost';
}
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn isset_empty_and_coalesce_do_not_read() {
    let body = r#"
$options = new Options();
var_dump(isset($options->host), empty($options->path));
echo $options->host ?? 'localhost';
$options->path ??= '/';
echo $options->path;
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn passing_a_property_to_a_call_reads_it_unless_a_reference_could_initialize_it() {
    let body = r#"
$options = new Options();
strlen($options->host);
preg_match('/x/', 'x', $options->path);
echo $options->path;
"#;
    assert_eq!(collect_reads(body), vec![read_message("host")]);
}

#[test]
fn an_object_that_escapes_is_no_longer_followed() {
    let body = r#"
$a = new Options();
configure($a);
echo $a->host;

$b = new Options();
$b->load();
echo $b->host;

$c = new Options();
$copy = $c;
echo $c->host;

$d = new Options();
$fill = function () use ($d) { $d->host = 'x'; };
$fill();
echo $d->host;

$e = new Options();
$fill = fn () => $e->host = 'x';
$fill();
echo $e->host;

$f = new Options();
extract(['f' => null]);
echo $f->host;

$g = new Options();
$g = new \stdClass();
echo $g->host;
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn a_variable_bound_by_reference_may_be_written_through_its_alias() {
    let body = r#"
function f(): void
{
    $fill = function () use (&$options) { $options->host = 'x'; };
    $options = new Options();
    $fill();
    echo $options->host;
}
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn a_call_in_the_global_scope_may_reach_the_object() {
    let body = r#"
$options = new Options();
load_config(); // may write it through `global $options`
echo $options->host;
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn a_call_inside_a_function_cannot_reach_a_local_object() {
    let body = r#"
function f(): void
{
    $options = new Options();
    load_config();
    echo $options->host;
}
"#;
    assert_eq!(collect_reads(body), vec![read_message("host")]);
}

#[test]
fn a_read_after_the_flow_ends_is_not_reported() {
    let body = r#"
function f(): void
{
    $options = new Options();
    return;
    echo $options->host;
}
"#;
    assert!(collect_reads(body).is_empty());
}

#[test]
fn a_property_the_class_writes_anywhere_is_not_reported() {
    let php = r#"<?php
final class Lazy
{
    public string $name;
    public string $label;
    public string $title;

    public function __construct(public string $id) {}

    public function load(): void
    {
        $this->name = 'x';
    }

    public static function create(): self
    {
        $self = new self('1');
        $self->label = 'x';
        return $self;
    }
}

$lazy = new Lazy('1');
echo $lazy->name, $lazy->label, $lazy->id, $lazy->title;
"#;
    assert_eq!(
        collect(php),
        vec!["Property Lazy::$title is read before it is initialized"]
    );
}

#[test]
fn a_parent_class_that_writes_the_property_initializes_it() {
    let php = r#"<?php
class Base
{
    public string $name;
    public string $label;

    public function __construct()
    {
        $this->name = 'x';
    }
}

final class Child extends Base {}

$child = new Child();
echo $child->name, $child->label;
"#;
    assert_eq!(
        collect(php),
        vec!["Property Child::$label is read before it is initialized"]
    );
}

#[test]
fn a_class_that_may_initialize_anything_is_not_judged() {
    let php = r#"<?php
trait Fills { public function fill(): void {} }

final class WithTrait { use Fills; public string $name; }

final class WithMagicGet
{
    public string $name;
    public function __construct() { unset($this->name); }
    public function __get(string $key): mixed { return $this->$key = 'x'; }
}

final class Hydrated
{
    public string $name;
    public function __construct(array $data)
    {
        foreach ($data as $key => $value) {
            $this->$key = $value;
        }
    }
}

final class Injected
{
    #[\Some\Inject]
    public string $name;
}

$a = new WithTrait();
echo $a->name;
$b = new WithMagicGet();
echo $b->name;
$c = new Hydrated(['name' => 'x']);
echo $c->name;
$d = new Injected();
echo $d->name;
"#;
    assert!(collect(php).is_empty(), "{:?}", collect(php));
}

#[test]
fn reads_of_a_class_in_another_file_are_reported() {
    let composer_json = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/"
        }
    }
}"#;
    let (backend, _tmp) = create_psr4_workspace(
        composer_json,
        &[(
            "src/Options.php",
            r#"<?php
namespace App;

final class Options
{
    public string $host;
    public string $path;

    public function withPath(string $path): void
    {
        $this->path = $path;
    }
}
"#,
        )],
    );
    let php = r#"<?php
namespace Client;

use App\Options;

$options = new Options();
echo $options->host, $options->path;
"#;
    assert_eq!(
        collect_on(&backend, php),
        vec!["Property App\\Options::$host is read before it is initialized"]
    );
}
