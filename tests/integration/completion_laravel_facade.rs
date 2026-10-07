//! Tests that listing members on a Laravel facade offers the concrete
//! class's methods, not just the facade's own declarations.
//!
//! `Facade::__callStatic()` forwards every static call to the container
//! instance named by `getFacadeAccessor()`. Laravel's own facades spell
//! that out in a generated `@method static` docblock, but an app-defined
//! facade that never ran `facade-documenter` has nothing to list, so the
//! members have to come from the class the accessor names, whether it names
//! it directly (`Container::class`) or through a container binding key
//! (`'thing.container'`) the alias table resolves.

use crate::common::{complete_labels_at_opened, create_psr4_workspace, open_php_at};

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Foundation\\": "vendor/illuminate/Foundation/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/"
        }
    }
}"#;

const FACADE_PHP: &str = r#"<?php
namespace Illuminate\Support\Facades;
abstract class Facade
{
    public static function __callStatic($method, $args)
    {
        return static::resolveFacadeInstance()->$method(...$args);
    }
}
"#;

const CONTAINER_PHP: &str = r#"<?php
namespace App\Services;
class Container
{
    public function resolveThing(string $id): object { return new \stdClass(); }
    public function configure(array $options): static { return $this; }
    protected function internals(): void {}
    public static function boot(): void {}
}
"#;

/// An app-defined facade with no generated `@method static` docblock.
const MY_FACADE_PHP: &str = r#"<?php
namespace App\Facades;
use App\Services\Container;
use Illuminate\Support\Facades\Facade;
class MyFacade extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return Container::class;
    }
}
"#;

/// A facade whose generated docblock already documents the forwarded
/// method, with the flattened return Laravel's generator produces.
const DOCUMENTED_FACADE_PHP: &str = r#"<?php
namespace App\Facades;
use App\Services\Container;
use Illuminate\Support\Facades\Facade;

/**
 * @method static mixed resolveThing(string $id)
 */
class DocumentedFacade extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return Container::class;
    }
}
"#;

/// `registerCoreContainerAliases()` in the shape Laravel declares it: the
/// string key a facade's accessor returns, mapped to the concrete class the
/// container binds it to.
const APPLICATION_PHP: &str = r#"<?php
namespace Illuminate\Foundation;
class Application
{
    public function registerCoreContainerAliases()
    {
        foreach ([
            'thing.container' => [\App\Services\Container::class],
        ] as $key => $aliases) {
            foreach ($aliases as $alias) {
                $this->alias($key, $alias);
            }
        }
    }
}
"#;

/// An app-defined facade that names a container binding rather than a class,
/// which is the shape Laravel's own facades use.
const BOUND_FACADE_PHP: &str = r#"<?php
namespace App\Facades;
use Illuminate\Support\Facades\Facade;
class BoundFacade extends Facade
{
    protected static function getFacadeAccessor()
    {
        return 'thing.container';
    }
}
"#;

/// A facade naming a key nothing binds statically (registered at runtime).
const UNBOUND_FACADE_PHP: &str = r#"<?php
namespace App\Facades;
use Illuminate\Support\Facades\Facade;
class UnboundFacade extends Facade
{
    protected static function getFacadeAccessor()
    {
        return 'nothing.binds.this';
    }
}
"#;

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Support/Facades/Facade.php", FACADE_PHP),
        (
            "vendor/illuminate/Foundation/Application.php",
            APPLICATION_PHP,
        ),
        ("src/Services/Container.php", CONTAINER_PHP),
        ("src/Facades/MyFacade.php", MY_FACADE_PHP),
        ("src/Facades/DocumentedFacade.php", DOCUMENTED_FACADE_PHP),
        ("src/Facades/BoundFacade.php", BOUND_FACADE_PHP),
        ("src/Facades/UnboundFacade.php", UNBOUND_FACADE_PHP),
    ]
}

async fn complete_labels(consumer: &str, line: u32, character: u32) -> Vec<String> {
    let mut files = base_files();
    files.push(("src/Consumer.php", consumer));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    let uri = open_php_at(&backend, &dir, "src/Consumer.php", consumer).await;
    complete_labels_at_opened(&backend, &uri, line, character).await
}

const UNDOCUMENTED_CONSUMER: &str = "\
<?php
namespace App;
use App\\Facades\\MyFacade;
class Consumer {
    public function go(): void {
        MyFacade::
    }
}
";

#[tokio::test]
async fn undocumented_facade_offers_the_concrete_class_methods() {
    let labels = complete_labels(UNDOCUMENTED_CONSUMER, 5, 18).await;
    assert!(
        labels.iter().any(|l| l.starts_with("resolveThing")),
        "expected Container::resolveThing on the facade, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("configure")),
        "expected Container::configure on the facade, got: {labels:?}"
    );
}

#[tokio::test]
async fn non_public_and_static_concrete_methods_stay_off_the_facade() {
    let labels = complete_labels(UNDOCUMENTED_CONSUMER, 5, 18).await;
    assert!(
        !labels.iter().any(|l| l.starts_with("internals")),
        "a protected method is not reachable through __callStatic, got: {labels:?}"
    );
    // `__callStatic` forwards to an instance, so a static method on the
    // concrete class is never reached through it.
    assert!(
        !labels.iter().any(|l| l.starts_with("boot")),
        "a static concrete method is not forwarded, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_generated_method_tag_keeps_precedence_over_the_forwarded_method() {
    let consumer = "\
<?php
namespace App;
use App\\Facades\\DocumentedFacade;
class Consumer {
    public function go(): void {
        DocumentedFacade::
    }
}
";
    let labels = complete_labels(consumer, 5, 26).await;
    let matches: Vec<&String> = labels
        .iter()
        .filter(|l| l.starts_with("resolveThing"))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "the `@method static` tag should be the only resolveThing, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_forwarded_call_types_to_the_concrete_return() {
    let consumer = "\
<?php
namespace App;
use App\\Facades\\MyFacade;
class Consumer {
    public function go(): void {
        $x = MyFacade::configure([]);
        $x->
    }
}
";
    let labels = complete_labels(consumer, 6, 12).await;
    assert!(
        labels.iter().any(|l| l.starts_with("resolveThing")),
        "a `static` return should chain on the concrete class, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_container_binding_accessor_offers_the_bound_class_methods() {
    let consumer = "\
<?php
namespace App;
use App\\Facades\\BoundFacade;
class Consumer {
    public function go(): void {
        BoundFacade::
    }
}
";
    let labels = complete_labels(consumer, 5, 21).await;
    assert!(
        labels.iter().any(|l| l.starts_with("resolveThing")),
        "expected the bound Container::resolveThing on the facade, got: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("internals")),
        "a protected method is not reachable through __callStatic, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_call_through_a_container_binding_accessor_types_to_the_concrete_return() {
    let consumer = "\
<?php
namespace App;
use App\\Facades\\BoundFacade;
class Consumer {
    public function go(): void {
        $x = BoundFacade::configure([]);
        $x->
    }
}
";
    let labels = complete_labels(consumer, 6, 12).await;
    assert!(
        labels.iter().any(|l| l.starts_with("resolveThing")),
        "a `static` return should chain on the bound class, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_runtime_only_binding_key_offers_nothing_to_forward() {
    let consumer = "\
<?php
namespace App;
use App\\Facades\\UnboundFacade;
class Consumer {
    public function go(): void {
        UnboundFacade::
    }
}
";
    let labels = complete_labels(consumer, 5, 23).await;
    assert!(
        !labels.iter().any(|l| l.starts_with("resolveThing")),
        "an unbound key must not forward another class's members, got: {labels:?}"
    );
}
