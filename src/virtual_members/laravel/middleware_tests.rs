use super::*;

fn alias<'a>(scan: &'a MiddlewareScan, name: &str) -> Option<&'a MiddlewareAliasEntry> {
    scan.aliases.iter().find(|alias| alias.name == name)
}

fn group<'a>(scan: &'a MiddlewareScan, name: &str) -> Option<&'a MiddlewareGroupEntry> {
    scan.groups.iter().find(|group| group.name == name)
}

#[test]
fn reads_the_framework_defaults() {
    let content = r#"<?php
namespace Illuminate\Foundation\Configuration;

class Middleware
{
    public function alias(array $aliases) { $this->customAliases = $aliases; }

    public function getMiddlewareGroups()
    {
        $middleware = [
            'web' => array_values(array_filter([
                \Illuminate\Cookie\Middleware\EncryptCookies::class,
                $this->authenticatedSessions ? 'auth.session' : null,
            ])),
            'api' => array_values(array_filter([
                \Illuminate\Routing\Middleware\SubstituteBindings::class,
            ])),
        ];
        $middleware = array_merge($middleware, $this->groups);
        return $middleware;
    }

    protected function defaultAliases()
    {
        $aliases = [
            'auth' => \Illuminate\Auth\Middleware\Authenticate::class,
            'throttle' => $this->throttleWithRedis
                ? \Illuminate\Routing\Middleware\ThrottleRequestsWithRedis::class
                : \Illuminate\Routing\Middleware\ThrottleRequests::class,
        ];
        return $aliases;
    }
}
"#;
    let scan = scan_middleware(content);
    assert!(scan.framework);
    assert!(!scan.incomplete);
    assert_eq!(
        alias(&scan, "auth").and_then(|a| a.class.as_deref()),
        Some("Illuminate\\Auth\\Middleware\\Authenticate")
    );
    assert_eq!(
        alias(&scan, "throttle").and_then(|a| a.class.as_deref()),
        Some("Illuminate\\Routing\\Middleware\\ThrottleRequestsWithRedis")
    );
    assert_eq!(
        group(&scan, "web").map(|g| g.members.clone()),
        Some(vec![
            "Illuminate\\Cookie\\Middleware\\EncryptCookies".to_string(),
            "auth.session".to_string(),
        ])
    );
    assert!(group(&scan, "api").is_some_and(|g| g.defines));
}

#[test]
fn reads_the_configurator_a_bootstrap_closure_receives() {
    let content = r#"<?php
use App\Http\Middleware\EnsureAdmin;
use App\Http\Middleware\TrackVisit;
use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Middleware;

return Application::configure(basePath: dirname(__DIR__))
    ->withMiddleware(function (Middleware $mw) {
        $mw->alias(['admin' => EnsureAdmin::class]);
        $mw->group('panel', ['web', 'admin']);
        $mw->appendToGroup('web', TrackVisit::class);
        $mw->web(append: [TrackVisit::class]);
    })->create();
"#;
    let scan = scan_middleware(content);
    assert!(!scan.framework);
    let admin = alias(&scan, "admin").expect("admin alias");
    assert_eq!(
        admin.class.as_deref(),
        Some("App\\Http\\Middleware\\EnsureAdmin")
    );
    assert_eq!(&content[admin.offset as usize..][..5], "admin");
    let panel = group(&scan, "panel").expect("panel group");
    assert!(panel.defines);
    assert_eq!(panel.members, vec!["web".to_string(), "admin".to_string()]);
    let additions: Vec<_> = scan
        .groups
        .iter()
        .filter(|g| g.name == "web")
        .map(|g| (g.defines, g.members.clone()))
        .collect();
    assert_eq!(
        additions,
        vec![
            (false, vec!["App\\Http\\Middleware\\TrackVisit".to_string()]),
            (false, vec!["App\\Http\\Middleware\\TrackVisit".to_string()]),
        ]
    );
}

#[test]
fn alias_on_another_receiver_is_not_a_registration() {
    let content = r#"<?php
use Illuminate\Foundation\Configuration\Middleware;

class AppServiceProvider
{
    public function register(): void
    {
        $this->app->alias('payments', Payments::class);
        $this->app->alias(['admin' => EnsureAdmin::class]);
    }
}
"#;
    assert!(scan_middleware(content).is_empty());
}

#[test]
fn reads_a_kernels_properties() {
    let content = r#"<?php
namespace App\Http;

use Illuminate\Foundation\Http\Kernel as HttpKernel;

class Kernel extends HttpKernel
{
    protected $middlewareGroups = [
        'web' => [\App\Http\Middleware\EncryptCookies::class],
    ];

    protected $routeMiddleware = [
        'auth' => \App\Http\Middleware\Authenticate::class,
    ];

    protected $middlewareAliases = [
        'verified' => 'Illuminate\Auth\Middleware\EnsureEmailIsVerified',
    ];
}
"#;
    let scan = scan_middleware(content);
    assert_eq!(
        alias(&scan, "auth").and_then(|a| a.class.as_deref()),
        Some("App\\Http\\Middleware\\Authenticate")
    );
    assert_eq!(
        alias(&scan, "verified").and_then(|a| a.class.as_deref()),
        Some("Illuminate\\Auth\\Middleware\\EnsureEmailIsVerified")
    );
    assert!(group(&scan, "web").is_some_and(|g| g.defines));
}

#[test]
fn router_registrations_and_unreadable_names() {
    let content = r#"<?php
class PackageServiceProvider
{
    public function boot(Router $router): void
    {
        $router->aliasMiddleware('tenant', \Acme\Tenant::class);
        $router->middlewareGroup('acme', [\Acme\Tenant::class]);
        $router->pushMiddlewareToGroup('web', \Acme\Share::class);
        foreach ($this->aliases as $name => $class) {
            $router->aliasMiddleware($name, $class);
        }
    }
}
"#;
    let scan = scan_middleware(content);
    assert!(alias(&scan, "tenant").is_some());
    assert!(group(&scan, "acme").is_some_and(|g| g.defines));
    assert!(group(&scan, "web").is_some_and(|g| !g.defines));
    assert!(scan.incomplete);
}

#[test]
fn the_application_overrides_the_framework_and_adds_to_its_groups() {
    let mut index = LaravelMiddlewareIndex::default();
    index.files.set_file(
        "file:///vendor/Middleware.php".to_string(),
        MiddlewareScan {
            aliases: vec![MiddlewareAliasEntry {
                name: "auth".to_string(),
                class: Some("Framework\\Authenticate".to_string()),
                offset: 1,
            }],
            groups: vec![MiddlewareGroupEntry {
                name: "web".to_string(),
                members: vec!["Framework\\Cookies".to_string()],
                defines: true,
                offset: 2,
            }],
            incomplete: false,
            framework: true,
        },
    );
    index.files.set_file(
        "file:///app/bootstrap/app.php".to_string(),
        MiddlewareScan {
            aliases: vec![MiddlewareAliasEntry {
                name: "auth".to_string(),
                class: Some("App\\Authenticate".to_string()),
                offset: 3,
            }],
            groups: vec![MiddlewareGroupEntry {
                name: "web".to_string(),
                members: vec!["App\\Track".to_string()],
                defines: false,
                offset: 4,
            }],
            incomplete: false,
            framework: false,
        },
    );
    index.rebuild();

    let Some(MiddlewareTarget::Alias(auth)) = index.get("auth") else {
        panic!("auth should be an alias");
    };
    assert_eq!(auth.class.as_deref(), Some("App\\Authenticate"));
    let Some(MiddlewareTarget::Group(web)) = index.get("web") else {
        panic!("web should be a group");
    };
    assert_eq!(
        web.members,
        vec!["Framework\\Cookies".to_string(), "App\\Track".to_string()]
    );
    assert_eq!(web.uri, "file:///vendor/Middleware.php");
    assert_eq!(
        index.all_names(),
        vec!["auth".to_string(), "web".to_string()]
    );
    assert!(!index.is_incomplete());
}
