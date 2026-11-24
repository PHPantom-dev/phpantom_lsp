use super::common::*;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"require":{"laravel/framework":"^13.0"},"autoload":{"psr-4":{"App\\":"app/","Illuminate\\Database\\Eloquent\\":"framework/"}}}"#;
const MODEL: &str = r"<?php namespace Illuminate\Database\Eloquent;
class Model {
    protected $guarded = ['*'];
    protected $fillable = [];
    public function fill(array $attributes) {}
    public function update(array $attributes, array $options = []) {}
    public static function unguard(bool $state = true) {}
    /** @return Builder<static> */ public static function query() {}
}";
const BUILDER: &str = r"<?php namespace Illuminate\Database\Eloquent;
/** @template T of Model */
class Builder {
    /** @return T */ public function create(array $attributes) {}
    public function update(array $values) {}
}";
const MODELS: &str = r"<?php namespace App;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Attributes\{Fillable as Allows, Guarded, Unguarded};
class Draft extends Model {}
class Article extends Model { protected $fillable = []; }
class Published extends Article {}
class OpenModel extends Model { protected $guarded = []; }
#[Allows(['name'])] class Tagged extends Model {}
#[Guarded(['id'])] class ProtectedModel extends Model {}
#[Unguarded] class Allowed extends Model {}
trait AllowsNames { protected $fillable = ['name']; }
class WithTrait extends Model { use AllowsNames; }
class Other { public static function create($attributes) {} public static function unguard() {} }
class Overridden extends Model { public static function create($attributes) {} }
";

async fn workspace(
    source: &str,
    bootstrap: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Uri) {
    create_initialized_psr4_workspace(
        COMPOSER,
        &[
            ("framework/Model.php", MODEL),
            ("framework/Builder.php", BUILDER),
            ("app/Draft.php", MODELS),
            (
                "framework/Relations/Relation.php",
                r"<?php namespace Illuminate\Database\Eloquent\Relations;
/** @template TRelated of \Illuminate\Database\Eloquent\Model
 * @template TParent of \Illuminate\Database\Eloquent\Model */
class Relation { public function create(array $attributes) {} }",
            ),
            ("app/Bootstrap.php", bootstrap),
            ("app/test.php", source),
        ],
        "app/test.php",
    )
    .await
}

fn warnings(backend: &phpantom_lsp::Backend, uri: &Uri, content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    with_code(diagnostics, "laravel_mass_assignment")
}

#[tokio::test]
async fn laravel_mass_assignment_matches_dynamic_arguments_and_receivers() {
    let source = r"<?php namespace App;
use Illuminate\Database\Eloquent\Builder;
/** @param Builder<Draft> $query */
function save(Draft $draft, array $data, $query, Draft|Article $either) {
    Draft::create($data);
    $draft?->FiLl(attributes: $data);
    $draft->update($data);
    Draft::updateOrCreate(['id' => 1], $data);
    Draft::firstOrCreate(values: ['id' => 1], attributes: $data);
    Draft::firstOrNew($data);
    $query->create($data);
    Draft::create(array('id' => 1));
    $draft->fill((['id' => 1]));
    $draft->update([], options: $data);
    Draft::firstOrNew();
    $query->update($data);
    Other::create($data);
    Overridden::create($data);
    Article::create($data);
    Published::create($data);
    OpenModel::create($data);
    Tagged::create($data);
    ProtectedModel::create($data);
    Allowed::create($data);
    WithTrait::create($data);
    $either->fill($data);
    Draft::create(...$data);
}";
    let (backend, _dir, uri) = workspace(
        source,
        r"<?php namespace App;
        use Illuminate\Database\Eloquent\Model as Base;
        Other::unguard(); Base::unguard(state: (/* keep guarded */ false),);",
    )
    .await;
    let diagnostics = warnings(&backend, &uri, source);
    let lines: Vec<_> = diagnostics.iter().map(|d| d.range.start.line).collect();
    assert_eq!(lines, (4..=10).collect::<Vec<_>>(), "{diagnostics:?}");
    for diagnostic in diagnostics {
        let line = source
            .lines()
            .nth(diagnostic.range.start.line as usize)
            .unwrap();
        assert_eq!(
            &line[diagnostic.range.start.character as usize
                ..diagnostic.range.end.character as usize],
            "$data"
        );
        assert!(message_text(&diagnostic.message).contains("App\\Draft"));
    }
}

#[tokio::test]
async fn laravel_mass_assignment_observes_unguard_across_file_edits() {
    let source = r"<?php namespace App; function save(array $data) { Draft::create($data); }";
    let (backend, dir, uri) = workspace(
        source,
        r"<?php namespace App; use Illuminate\Database\Eloquent\Model as Foo; Foo::UnGuard();",
    )
    .await;
    assert!(warnings(&backend, &uri, source).is_empty());
    open_php_at(
        &backend,
        &dir,
        "app/Bootstrap.php",
        "<?php // Model::unguard();",
    )
    .await;
    assert_eq!(warnings(&backend, &uri, source).len(), 1);
    open_php_at(
        &backend,
        &dir,
        "app/Bootstrap.php",
        r"<?php namespace App; Draft::unguard();",
    )
    .await;
    assert!(warnings(&backend, &uri, source).is_empty());
}

#[tokio::test]
async fn laravel_mass_assignment_follows_the_method_that_handles_the_attributes() {
    let source = r"<?php namespace App;
use Illuminate\Database\Eloquent\{Model, Builder, Relations\Relation};
/** @template T of Model */ class SpecialBuilder extends Builder { public function create(array $attributes) {} }
/** @template T of Model */ class ChildBuilder extends SpecialBuilder {}
/** @template T of Model */ class PlainBuilder extends Builder {}
/** @property array $fillable */ class Documented extends Model {}
class LocalDraft extends Model {
    public function apply(array $data) { self::fill($data); } // warn
    public static function broken(array $data) { self::fill($data); }
}
class CustomFill extends Model { public function fill(array $attributes) {} }
/** @template T */ class Box { public function create(array $attributes) {} }
/**
 * @param Relation<Draft, Article> $drafts
 * @param Relation<Article, Draft> $articles
 * @param Relation<mixed, Draft> $unknown
 * @param Builder<Overridden> $overridden
 * @param SpecialBuilder<Draft> $special
 * @param ChildBuilder<Draft> $child
 * @param PlainBuilder<Draft> $plain
 * @param Box<Draft> $box
 */
function save($drafts, $articles, $unknown, $overridden, $special, $child, $plain, $box, array $data) {
    $drafts->create($data); // warn
    $articles->create($data);
    $unknown->create($data);
    $overridden->create($data); // warn
    $special->create($data);
    $child->create($data);
    $plain->create($data); // warn
    Draft::query()->create($data); // warn
    Draft::query()->update($data);
    $box->create($data);
    Draft::fill($data);
    CustomFill::create($data);
    Documented::create($data); // warn
    Draft::updateOrCreate($data, ...$data); // warn
    Draft::create(null);
    Draft::firstOrCreate($data, $data); // warn
    Draft::create($data['attributes']); // warn
    $drafts->update($data);
    call_user_func('example');
    $drafts->{$data['method']}($data);
}";
    let (backend, _dir, uri) = workspace(source, "<?php").await;
    let diagnostics = warnings(&backend, &uri, source);
    let expected: Vec<_> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.ends_with("// warn"))
        .map(|(i, _)| i as u32)
        .collect();
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d.range.start.line)
            .collect::<Vec<_>>(),
        expected,
        "{diagnostics:?}"
    );
}

#[tokio::test]
async fn laravel_mass_assignment_guard_setting_belongs_to_the_application() {
    let source = r"<?php namespace App; Draft::create($request->all());";
    let unguard = r"<?php \Illuminate\Database\Eloquent\Model::unguard();";
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        COMPOSER,
        &[
            ("app/Draft.php", MODELS),
            ("app/Save.php", source),
            ("framework/Model.php", MODEL),
            ("vendor/composer/installed.json", r#"{"packages":[]}"#),
            ("vendor/package/Example.php", unguard),
            ("other/composer.json", "{}"),
            ("other/vendor/composer/installed.json", r#"{"packages":[]}"#),
            ("other/Bootstrap.php", unguard),
        ],
        "app/Save.php",
    )
    .await;
    open_php_file(&backend, "other/Bootstrap.php").await;
    open_php_file(&backend, "vendor/package/Example.php").await;
    assert_eq!(warnings(&backend, &uri, source).len(), 1);
}
