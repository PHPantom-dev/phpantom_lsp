//! Unit tests for the Laravel alias-table parsers.

use super::*;

#[test]
fn parses_core_container_aliases() {
    let src = r#"<?php
namespace Illuminate\Foundation;

class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'app' => [self::class, \Illuminate\Contracts\Container\Container::class],
            'blade.compiler' => [\Illuminate\View\Compilers\BladeCompiler::class],
            'cache' => [\Illuminate\Cache\CacheManager::class, \Illuminate\Contracts\Cache\Factory::class],
            'db.connection' => [\Illuminate\Database\Connection::class, \Illuminate\Database\ConnectionInterface::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }
}
"#;

    let aliases = parse_container_aliases(src).expect("container aliases parsed");

    // Concrete class (first entry) mapped by the string key.
    assert_eq!(
        aliases.get("blade.compiler").map(String::as_str),
        Some("Illuminate\\View\\Compilers\\BladeCompiler")
    );
    assert_eq!(
        aliases.get("cache").map(String::as_str),
        Some("Illuminate\\Cache\\CacheManager")
    );
    assert_eq!(
        aliases.get("db.connection").map(String::as_str),
        Some("Illuminate\\Database\\Connection")
    );
    // `self::class` carries no concrete; the `app` entry is skipped.
    assert!(!aliases.contains_key("app"));
}

#[test]
fn parses_facade_default_aliases_resolving_use_statements() {
    let src = r#"<?php
namespace Illuminate\Support\Facades;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Support\Arr;
use Illuminate\Support\Str;

abstract class Facade
{
    public static function defaultAliases()
    {
        return new Collection([
            'App' => App::class,
            'Arr' => Arr::class,
            'DB' => DB::class,
            'Eloquent' => Model::class,
            'Str' => Str::class,
        ]);
    }
}
"#;

    let aliases = parse_facade_default_aliases(src).expect("facade aliases parsed");

    // Same-namespace facades.
    assert_eq!(
        aliases.get("App").map(String::as_str),
        Some("Illuminate\\Support\\Facades\\App")
    );
    assert_eq!(
        aliases.get("DB").map(String::as_str),
        Some("Illuminate\\Support\\Facades\\DB")
    );
    // Imported classes resolve through their `use` statements.
    assert_eq!(
        aliases.get("Eloquent").map(String::as_str),
        Some("Illuminate\\Database\\Eloquent\\Model")
    );
    assert_eq!(
        aliases.get("Arr").map(String::as_str),
        Some("Illuminate\\Support\\Arr")
    );
}

#[test]
fn parses_config_aliases_plain_array_shape() {
    let src = r#"<?php

return [
    'name' => 'Example',
    'providers' => [
        App\Providers\AppServiceProvider::class,
    ],
    'aliases' => [
        'App'      => Illuminate\Support\Facades\App::class,
        'DB'       => Illuminate\Support\Facades\DB::class,
        'Eloquent' => Illuminate\Database\Eloquent\Model::class,
        'PDF'      => Barryvdh\DomPDF\Facade\Pdf::class,
    ],
];
"#;

    let aliases = parse_config_facade_aliases(src);

    assert_eq!(
        aliases.get("App").map(String::as_str),
        Some("Illuminate\\Support\\Facades\\App")
    );
    assert_eq!(
        aliases.get("PDF").map(String::as_str),
        Some("Barryvdh\\DomPDF\\Facade\\Pdf")
    );
    // The `providers` list (value-only entries) is not mistaken for aliases.
    assert!(!aliases.values().any(|v| v.ends_with("AppServiceProvider")));
}

#[test]
fn parses_config_aliases_merge_shape() {
    let src = r#"<?php

use Illuminate\Support\Facades\Facade;

return [
    'aliases' => Facade::defaultAliases()->merge([
        'Custom' => App\Support\CustomFacade::class,
    ])->toArray(),
];
"#;

    let aliases = parse_config_facade_aliases(src);

    // The custom entry inside the `merge([...])` call is picked up; the
    // `defaultAliases()` base set is parsed separately from the framework.
    assert_eq!(
        aliases.get("Custom").map(String::as_str),
        Some("App\\Support\\CustomFacade")
    );
}

#[test]
fn non_matching_source_yields_nothing() {
    let src = r#"<?php
class Nothing
{
    public function noAliasesHere()
    {
        return ['a', 'b', 'c'];
    }
}
"#;
    assert!(parse_container_aliases(src).is_none());
    assert!(parse_facade_default_aliases(src).is_none());
    assert!(parse_config_facade_aliases(src).is_empty());
}
