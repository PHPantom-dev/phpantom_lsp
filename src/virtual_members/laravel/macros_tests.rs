//! Unit tests for the Laravel macro registration extractor.

use super::*;

#[test]
fn extracts_date_factory_class_from_provider() {
    let content = r#"<?php
namespace App\Providers;

use Carbon\CarbonImmutable;
use Illuminate\Support\DateFactory;

class AppServiceProvider {
    public function register(): void {
        DateFactory::use(CarbonImmutable::class);
    }
}
"#;

    assert_eq!(
        extract_date_factory_class(content).as_deref(),
        Some("Carbon\\CarbonImmutable")
    );
}

#[test]
fn extracts_date_factory_class_from_use_class() {
    let content = r#"<?php
use Carbon\CarbonImmutable;
use Illuminate\Support\DateFactory;

DateFactory::useClass(CarbonImmutable::class);
"#;

    assert_eq!(
        extract_date_factory_class(content).as_deref(),
        Some("Carbon\\CarbonImmutable")
    );
}

#[test]
fn extracts_date_factory_class_through_facade() {
    let content = r#"<?php
use Carbon\CarbonImmutable;
use Illuminate\Support\Facades\Date;

Date::use(CarbonImmutable::class);
"#;

    assert_eq!(
        extract_date_factory_class(content).as_deref(),
        Some("Carbon\\CarbonImmutable")
    );
}

#[test]
fn ignores_unrelated_use_calls() {
    assert_eq!(
        extract_date_factory_class("<?php Foo::use(Bar::class);"),
        None
    );
}

#[test]
fn extracts_closure_macro_with_signature() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Support\Collection;
class AppServiceProvider {
    public function boot(): void {
        Collection::macro('sumPrices', function (string $field): float {
            return 0.0;
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    let reg = &regs[0];
    assert_eq!(reg.target, "Illuminate\\Support\\Collection");
    assert_eq!(reg.method.name.as_str(), "sumPrices");
    assert_eq!(reg.method.parameters.len(), 1);
    assert_eq!(reg.method.parameters[0].name.as_str(), "$field");
    assert_eq!(
        reg.method.return_type.as_ref().map(|t| t.to_string()),
        Some("float".to_string())
    );
}

#[test]
fn extracts_arrow_function_macro() {
    let content = r#"<?php
use Illuminate\Support\Str;
Str::macro('shout', fn (string $s): string => strtoupper($s));
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Str");
    assert_eq!(regs[0].method.name.as_str(), "shout");
    assert_eq!(regs[0].method.parameters.len(), 1);
}

#[test]
fn resolves_target_through_use_statement() {
    // `Response` is imported, so the bare name resolves to the FQN.
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Support\Facades\Response;
Response::macro('caps', function () { return 1; });
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Facades\\Response");
}

#[test]
fn skips_non_literal_name() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro($dynamicName, function () {});
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn skips_non_closure_second_argument() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('viaCallable', 'someFunction');
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn skips_relative_self_target() {
    let content = r#"<?php
class Widget {
    public static function register(): void {
        self::macro('x', function () {});
    }
}
"#;
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn no_macro_substring_is_cheap_empty() {
    let content = "<?php class Foo { public function bar() {} }";
    assert!(extract_macro_registrations(content, None).is_empty());
}

#[test]
fn index_stores_static_and_instance_variants() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('doubled', function (): int { return 2; });
"#;
    let regs = extract_macro_registrations(content, None);
    let mut index = LaravelMacroIndex::default();
    index.set_file("file:///provider.php".to_string(), regs);
    index.rebuild();

    let methods = index
        .get("Illuminate\\Support\\Collection")
        .expect("target should be indexed");
    assert_eq!(methods.len(), 2, "should store static + instance variants");
    assert!(methods.iter().any(|m| m.is_static));
    assert!(methods.iter().any(|m| !m.is_static));
    assert!(methods.iter().all(|m| m.name.as_str() == "doubled"));
}

#[test]
fn index_records_registration_source_location() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('sumPrices', function (): float { return 0.0; });
"#;
    let regs = extract_macro_registrations(content, None);
    let mut index = LaravelMacroIndex::default();
    index.set_file(
        "file:///app/Providers/AppServiceProvider.php".to_string(),
        regs,
    );
    index.rebuild();

    let (uri, offset) = index
        .definition("Illuminate\\Support\\Collection", "sumPrices")
        .expect("macro definition location should be recorded");
    assert_eq!(uri, "file:///app/Providers/AppServiceProvider.php");
    // The offset points at the `'sumPrices'` string literal.
    assert_eq!(
        &content[offset as usize..offset as usize + 11],
        "'sumPrices'"
    );
}

#[test]
fn parse_installed_providers_reads_extra_laravel_providers() {
    let installed = r#"{
        "packages": [
            {
                "name": "livewire/livewire",
                "extra": { "laravel": { "providers": ["Livewire\\LivewireServiceProvider"] } }
            },
            {
                "name": "some/plain-package"
            },
            {
                "name": "spatie/laravel-permission",
                "extra": {
                    "laravel": {
                        "providers": [
                            "\\Spatie\\Permission\\PermissionServiceProvider"
                        ]
                    }
                }
            }
        ]
    }"#;
    let providers = parse_installed_providers(installed);
    assert_eq!(
        providers,
        vec![
            "Livewire\\LivewireServiceProvider".to_string(),
            "Spatie\\Permission\\PermissionServiceProvider".to_string(),
        ]
    );
}

#[test]
fn parse_installed_providers_handles_composer_1_top_level_array() {
    let installed = r#"[
        {
            "name": "inertiajs/inertia-laravel",
            "extra": { "laravel": { "providers": ["Inertia\\ServiceProvider"] } }
        }
    ]"#;
    assert_eq!(
        parse_installed_providers(installed),
        vec!["Inertia\\ServiceProvider".to_string()]
    );
}

#[test]
fn parse_provider_class_list_bootstrap_providers() {
    // Laravel 11+ bootstrap/providers.php: a bare `return [...]` of providers.
    let content = r#"<?php
return [
    App\Providers\AppServiceProvider::class,
    App\Providers\RouteServiceProvider::class,
];
"#;
    assert_eq!(
        parse_provider_class_list(content),
        vec![
            "App\\Providers\\AppServiceProvider".to_string(),
            "App\\Providers\\RouteServiceProvider".to_string(),
        ]
    );
}

#[test]
fn parse_provider_class_list_config_app_providers_key() {
    // Laravel ≤10 config/app.php: only the `providers` array is collected,
    // not the `aliases` array.
    let content = r#"<?php
return [
    'name' => 'Laravel',
    'providers' => [
        Illuminate\Auth\AuthServiceProvider::class,
        App\Providers\AppServiceProvider::class,
    ],
    'aliases' => [
        'App' => Illuminate\Support\Facades\App::class,
    ],
];
"#;
    let providers = parse_provider_class_list(content);
    assert!(providers.contains(&"Illuminate\\Auth\\AuthServiceProvider".to_string()));
    assert!(providers.contains(&"App\\Providers\\AppServiceProvider".to_string()));
    assert!(
        !providers.contains(&"Illuminate\\Support\\Facades\\App".to_string()),
        "aliases entries must not be treated as providers"
    );
}

#[test]
fn parse_provider_class_list_empty_without_class_const() {
    assert!(parse_provider_class_list("<?php return [];").is_empty());
}

#[test]
fn parse_provider_referenced_classes_collects_method_body_refs() {
    let content = r#"<?php
namespace App\Providers;

use App\Macros\CollectionMacros;
    class MacroServiceProvider {
        public function boot(): void {
            CollectionMacros::boot();
            LocalMacros::register();
        }

        private function registerResponse(): void {
            \App\Macros\ResponseMacros::boot();
        }
}
"#;
    assert_eq!(
        parse_provider_referenced_classes(content),
        vec![
            "App\\Macros\\CollectionMacros".to_string(),
            "App\\Providers\\LocalMacros".to_string(),
            "App\\Macros\\ResponseMacros".to_string(),
        ]
    );
}

#[test]
fn parse_provider_referenced_classes_collects_instantiations_and_call_arguments() {
    let content = r#"<?php
namespace App\Providers;

use App\Macros\CollectionMacros;
use App\Macros\ResponseMacros;
use App\Macros\StrMacros;
class MacroServiceProvider {
    public function boot(): void {
        (new CollectionMacros())->register();
        $this->call(ResponseMacros::boot(...));
        Registrar::run(StrMacros::class);
    }
}
"#;
    let refs = parse_provider_referenced_classes(content);
    assert!(
        refs.contains(&"App\\Macros\\CollectionMacros".to_string()),
        "instantiated helper should be collected, got: {refs:?}"
    );
    assert!(
        refs.contains(&"App\\Macros\\StrMacros".to_string()),
        "::class argument of a static call should be collected, got: {refs:?}"
    );
}

#[test]
fn extracts_instance_macro_from_typed_parameter() {
    let content = r#"<?php
use Illuminate\Database\Eloquent\Builder;

class ConfidentialScope {
    public function extend(Builder $query): void {
        $query->macro('withConfidential', function (bool $flag = true): Builder {
            return $this;
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Database\\Eloquent\\Builder");
    assert_eq!(regs[0].method.name.as_str(), "withConfidential");
}

#[test]
fn extracts_instance_macro_from_closure_typed_parameter() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Database\Eloquent\Builder;

class AppServiceProvider {
    public function boot(): void {
        $this->app->resolving(Builder::class, function (Builder $builder): void {
            $builder->macro('withConfidential', function (bool $flag = true): Builder {
                return $this;
            });
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Database\\Eloquent\\Builder");
    assert_eq!(regs[0].method.name.as_str(), "withConfidential");
}

#[test]
fn closure_parameter_shadows_outer_typed_variable() {
    // The inner `$query` is a Collection, not the method's Builder; the
    // macro must attach to the shadowing parameter's type.
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Support\Collection;

class ShadowScope {
    public function extend(Builder $query): void {
        $this->each(function (Collection $query): void {
            $query->macro('shadowed', function (): int {
                return 1;
            });
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Collection");
    assert_eq!(regs[0].method.name.as_str(), "shadowed");
}

#[test]
fn closure_without_capture_does_not_see_outer_typed_variable() {
    // A plain closure only sees `use (...)` captures; `$query` inside the
    // uncaptured closure is a different (undefined) variable.
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Database\Eloquent\Builder;

class NoCaptureScope {
    public function extend(Builder $query): void {
        $this->later(function (): void {
            $query->macro('invisible', function (): int {
                return 1;
            });
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert!(
        regs.is_empty(),
        "uncaptured outer variable must not resolve, got: {:?}",
        regs.iter()
            .map(|r| r.method.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn closure_use_capture_keeps_outer_typed_variable() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Database\Eloquent\Builder;

class CaptureScope {
    public function extend(Builder $query): void {
        $this->later(function () use ($query): void {
            $query->macro('captured', function (): int {
                return 1;
            });
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Database\\Eloquent\\Builder");
    assert_eq!(regs[0].method.name.as_str(), "captured");
}

#[test]
fn arrow_function_captures_outer_typed_variable() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Database\Eloquent\Builder;

class ArrowScope {
    public function extend(Builder $query): callable {
        return fn (): mixed => $query->macro('viaArrow', function (): int {
            return 1;
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Database\\Eloquent\\Builder");
    assert_eq!(regs[0].method.name.as_str(), "viaArrow");
}

#[test]
fn index_removes_file_contributions_when_emptied() {
    let content = r#"<?php
use Illuminate\Support\Collection;
Collection::macro('temp', function () {});
"#;
    let uri = "file:///provider.php".to_string();
    let mut index = LaravelMacroIndex::default();
    index.set_file(uri.clone(), extract_macro_registrations(content, None));
    index.rebuild();
    assert!(!index.is_empty());

    // File edited to remove the macro.
    index.set_file(uri, Vec::new());
    index.rebuild();
    assert!(index.is_empty());
}

// ─── Macroable::mixin() ─────────────────────────────────────────────────────

#[test]
fn extracts_mixin_registration_from_new_instance() {
    let content = r#"<?php
namespace App\Providers;
use Illuminate\Support\Str;
use App\Mixins\StrMixin;
class AppServiceProvider {
    public function boot(): void {
        Str::mixin(new StrMixin());
    }
}
"#;
    let regs = extract_mixin_registrations(content);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Str");
    assert_eq!(regs[0].mixin_fqn, "App\\Mixins\\StrMixin");
}

#[test]
fn extracts_mixin_registration_from_class_constant() {
    let content = r#"<?php
use Illuminate\Support\Collection;
use App\Mixins\CollectionMixin;
Collection::mixin(CollectionMixin::class);
"#;
    let regs = extract_mixin_registrations(content);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Illuminate\\Support\\Collection");
    assert_eq!(regs[0].mixin_fqn, "App\\Mixins\\CollectionMixin");
}

#[test]
fn skips_mixin_with_non_literal_argument() {
    let content = r#"<?php
use Illuminate\Support\Str;
Str::mixin($someMixin);
"#;
    assert!(extract_mixin_registrations(content).is_empty());
}

#[test]
fn skips_relative_self_mixin_target() {
    let content = r#"<?php
use App\Mixins\StrMixin;
class Widget {
    public static function register(): void {
        self::mixin(new StrMixin());
    }
}
"#;
    assert!(extract_mixin_registrations(content).is_empty());
}

#[test]
fn no_mixin_substring_is_cheap_empty() {
    let content = "<?php class Foo { public function bar() {} }";
    assert!(extract_mixin_registrations(content).is_empty());
}

#[test]
fn synthesizes_macros_from_mixin_methods() {
    let mixin = r#"<?php
namespace App\Mixins;
use Closure;
class StrMixin {
    public function shout(): Closure {
        return function (string $value): string {
            return strtoupper($value);
        };
    }

    public function repeat(): Closure {
        return fn (string $value, int $times): string => str_repeat($value, $times);
    }
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\StrMixin",
        "file:///app/Mixins/StrMixin.php",
        "Illuminate\\Support\\Str",
        None,
    );
    assert_eq!(regs.len(), 2);

    let shout = regs
        .iter()
        .find(|r| r.method.name.as_str() == "shout")
        .unwrap();
    assert_eq!(shout.target, "Illuminate\\Support\\Str");
    assert_eq!(shout.method.parameters.len(), 1);
    assert_eq!(shout.method.parameters[0].name.as_str(), "$value");
    assert_eq!(
        shout.method.return_type.as_ref().map(|t| t.to_string()),
        Some("string".to_string())
    );
    assert!(shout.method.is_macro);
    // Go-to-definition points at the mixin method's own file.
    assert_eq!(
        shout.definition_uri.as_deref(),
        Some("file:///app/Mixins/StrMixin.php")
    );

    let repeat = regs
        .iter()
        .find(|r| r.method.name.as_str() == "repeat")
        .unwrap();
    assert_eq!(repeat.method.parameters.len(), 2);
}

#[test]
fn skips_mixin_methods_without_a_returned_closure() {
    let mixin = r#"<?php
namespace App\Mixins;
use Closure;
class StrMixin {
    public function ok(): Closure {
        return fn () => 1;
    }
    // No returned closure — not a macro factory.
    public function notAFactory(): int {
        return 42;
    }
    // Private, static and magic methods are never mixed in.
    private function secret(): Closure {
        return fn () => 1;
    }
    public static function boot(): Closure {
        return fn () => 1;
    }
    public function __construct() {}
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\StrMixin",
        "file:///m.php",
        "Illuminate\\Support\\Str",
        None,
    );
    let names: Vec<_> = regs
        .iter()
        .map(|r| r.method.name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["ok".to_string()]);
}

#[test]
fn synthesize_mixin_ignores_other_classes_in_file() {
    let mixin = r#"<?php
namespace App\Mixins;
use Closure;
class Other {
    public function nope(): Closure { return fn () => 1; }
}
class StrMixin {
    public function yes(): Closure { return fn () => 1; }
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\StrMixin",
        "file:///m.php",
        "Illuminate\\Support\\Str",
        None,
    );
    let names: Vec<_> = regs
        .iter()
        .map(|r| r.method.name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["yes".to_string()]);
}

// ─── Trait-based mixin() (Carbon pattern) ───────────────────────────────────

#[test]
fn synthesizes_macros_from_trait_mixin_direct_methods() {
    let mixin = r#"<?php
namespace App\Mixins;

use Carbon\CarbonInterface;

trait TimezoneMixin {
    public function toTz(string $tz, bool $shift = false): CarbonInterface
    {
        return $shift
            ? $this->shiftTimezone($tz)
            : $this->timezone($tz);
    }

    public function toAppTz(bool $shift = false): CarbonInterface
    {
        return $this->toTz(config('app.timezone'), $shift);
    }
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\TimezoneMixin",
        "file:///app/Mixins/TimezoneMixin.php",
        "Carbon\\CarbonImmutable",
        None,
    );
    assert_eq!(regs.len(), 2);

    let to_tz = regs
        .iter()
        .find(|r| r.method.name.as_str() == "toTz")
        .unwrap();
    assert_eq!(to_tz.target, "Carbon\\CarbonImmutable");
    assert_eq!(to_tz.method.parameters.len(), 2);
    assert_eq!(to_tz.method.parameters[0].name.as_str(), "$tz");
    assert_eq!(to_tz.method.parameters[1].name.as_str(), "$shift");
    assert!(to_tz.method.is_macro);
    assert_eq!(
        to_tz.definition_uri.as_deref(),
        Some("file:///app/Mixins/TimezoneMixin.php")
    );

    let to_app = regs
        .iter()
        .find(|r| r.method.name.as_str() == "toAppTz")
        .unwrap();
    assert_eq!(to_app.method.parameters.len(), 1);
}

#[test]
fn trait_mixin_uses_direct_signature_even_when_returning_closure() {
    let mixin = r#"<?php
namespace App\Mixins;
use Closure;

trait HybridMixin {
    public function returnsClosure(): Closure {
        return function (string $value): string {
            return strtoupper($value);
        };
    }
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\HybridMixin",
        "file:///m.php",
        "Carbon\\CarbonImmutable",
        None,
    );
    assert_eq!(regs.len(), 1);
    let reg = &regs[0];
    assert_eq!(reg.method.name.as_str(), "returnsClosure");
    assert_eq!(reg.method.parameters.len(), 0);
    assert_eq!(
        reg.method.return_type.as_ref().map(|t| t.to_string()),
        Some("Closure".to_string()),
        "trait mixins should use the trait method's own return type, not a returned closure"
    );
}

#[test]
fn trait_mixin_skips_private_static_abstract_magic_methods() {
    let mixin = r#"<?php
namespace App\Mixins;

trait FilteredMixin {
    public function ok(): int { return 1; }
    private function secret(): int { return 2; }
    public static function boot(): void {}
    public function __construct() {}
    abstract public function nope(): int;
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\FilteredMixin",
        "file:///m.php",
        "Carbon\\CarbonImmutable",
        None,
    );
    let names: Vec<_> = regs
        .iter()
        .map(|r| r.method.name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["ok".to_string()]);
}

#[test]
fn trait_mixin_ignores_other_traits_in_file() {
    let mixin = r#"<?php
namespace App\Mixins;

trait Other {
    public function nope(): int { return 1; }
}
trait TargetMixin {
    public function yes(): int { return 1; }
}
"#;
    let regs = synthesize_mixin_macros(
        mixin,
        "App\\Mixins\\TargetMixin",
        "file:///m.php",
        "Carbon\\CarbonImmutable",
        None,
    );
    let names: Vec<_> = regs
        .iter()
        .map(|r| r.method.name.as_str().to_string())
        .collect();
    assert_eq!(names, vec!["yes".to_string()]);
}

#[test]
fn carbon_macro_registration_works() {
    let content = r#"<?php
namespace App\Providers;
use Carbon\CarbonImmutable;
class AppServiceProvider {
    public function boot(): void {
        CarbonImmutable::macro('diffFromYear', function (int $year, bool $absolute = false): string {
            return '';
        });
    }
}
"#;
    let regs = extract_macro_registrations(content, None);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Carbon\\CarbonImmutable");
    assert_eq!(regs[0].method.name.as_str(), "diffFromYear");
    assert_eq!(regs[0].method.parameters.len(), 2);
    assert_eq!(regs[0].method.parameters[0].name.as_str(), "$year");
    assert_eq!(regs[0].method.parameters[1].name.as_str(), "$absolute");
}

#[test]
fn carbon_mixin_registration_with_trait() {
    let content = r#"<?php
use Carbon\CarbonImmutable;
use App\Mixins\TimezoneMixin;
CarbonImmutable::mixin(TimezoneMixin::class);
"#;
    let regs = extract_mixin_registrations(content);
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].target, "Carbon\\CarbonImmutable");
    assert_eq!(regs[0].mixin_fqn, "App\\Mixins\\TimezoneMixin");
}
