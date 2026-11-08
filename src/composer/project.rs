//! What kind of project a `composer.json` describes: its PHP version, the
//! frameworks and packages that change how its code is resolved, and
//! where a Drupal project keeps its web root.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mago_composer::ComposerPackageExtra;

use super::{ComposerPackage, read_composer_package};
use crate::types::PhpVersion;

/// Detect the target PHP version from a project's `composer.json`.
///
/// Checks two locations in order:
///   1. `config.platform.php` — an explicit platform override
///      (e.g. `"8.3.1"` → 8.3).
///   2. `require.php` — the version constraint from the dependency
///      list (e.g. `"^8.4"` → 8.4).
///
/// Returns `None` when `composer.json` is missing, unreadable, or does
/// not contain a PHP version constraint.  The caller should fall back to
/// [`PhpVersion::default`] in that case.
pub fn detect_php_version(workspace_root: &Path) -> Option<PhpVersion> {
    let package = read_composer_package(workspace_root)?;
    detect_php_version_from_package(&package)
}

/// Extract the target PHP version from a parsed [`ComposerPackage`].
///
/// Checks two locations in order:
///   1. `config.platform.php` — an explicit platform override
///      (e.g. `"8.3.1"` → 8.3).
///   2. `require.php` — the version constraint from the dependency
///      list (e.g. `"^8.4"` → 8.4).
///
/// Returns `None` when the package does not contain a PHP version
/// constraint.  The caller should fall back to [`PhpVersion::default`]
/// in that case.
pub fn detect_php_version_from_package(package: &ComposerPackage) -> Option<PhpVersion> {
    // 1. config.platform.php — exact version string like "8.3.1"
    if let Some(platform_val) = package.config.as_ref().and_then(|c| c.platform.get("php"))
        && let mago_composer::ComposerPackageConfigPlatformValue::String(s) = platform_val
        && let Some(ver) = PhpVersion::from_composer_constraint(s)
    {
        return Some(ver);
    }

    // 2. require.php — constraint string like "^8.4", ">=8.0"
    if let Some(require_php) = package.require.get("php")
        && let Some(ver) = PhpVersion::from_composer_constraint(require_php)
    {
        return Some(ver);
    }

    None
}

/// Check whether a package name appears in `composer.json`'s `require-dev`.
///
/// Matches the package name exactly (e.g.
/// `"friendsofphp/php-cs-fixer"`, `"squizlabs/php_codesniffer"`).
pub(crate) fn has_require_dev(package: &ComposerPackage, dep: &str) -> bool {
    package.require_dev.contains_key(dep)
}

/// Check whether a package name appears in `composer.json`'s `require` or
/// `require-dev`.
///
/// Matches the package name exactly (e.g. `"phpstan/phpstan"`).
pub(crate) fn has_dependency(package: &ComposerPackage, dep: &str) -> bool {
    package.require.contains_key(dep) || package.require_dev.contains_key(dep)
}

/// Package-name suffixes of the PHPStan extensions that teach it Laravel.
///
/// Suffixes rather than exact names, because either is commonly installed
/// from a fork published under a different vendor prefix (`calebdw/larastan`
/// forked `larastan/larastan` when the latter was slow to pick up
/// compatibility fixes) and a fork supplies the same knowledge as what it
/// forked.
const LARAVEL_PHPSTAN_EXTENSIONS: [&str; 2] = ["/larastan", "/phpstan-laravel"];

/// Check whether the project depends on a PHPStan extension that makes
/// PHPStan understand Laravel.
///
/// The question is what the project has installed that can explain Eloquent
/// magic, the facades, and the container to PHPStan — not which package
/// does the explaining. Both `larastan/larastan` and
/// `calebdw/phpstan-laravel` answer it, as does a fork of either; see
/// [`LARAVEL_PHPSTAN_EXTENSIONS`].
pub(crate) fn has_laravel_aware_phpstan(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            let name = name.to_ascii_lowercase();
            LARAVEL_PHPSTAN_EXTENSIONS
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

/// Detect whether the project depends on Laravel or a standalone
/// Illuminate component.
///
/// Returns `true` when `require` or `require-dev` lists `laravel/framework`
/// (a full Laravel application) or any `illuminate/*` package (a library or
/// standalone use of Eloquent, the query builder, collections, etc.).
///
/// Laravel-specific analysis (Eloquent model members, the query-builder
/// forwarding, config/view/route/translation key resolution, the contract
/// to concrete-class bindings) is only useful when one of these packages is
/// present.  Gating that work on this check means non-Laravel projects skip
/// the parent-chain walks and post-resolution patches entirely.
pub(crate) fn is_laravel_project(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            name.eq_ignore_ascii_case("laravel/framework")
                || name.to_ascii_lowercase().starts_with("illuminate/")
        })
}

/// Detect whether the project uses Doctrine's entity repositories, through
/// the ORM, the MongoDB ODM, or the Symfony bundle or pack that installs
/// them.
///
/// The Doctrine entity and repository lenses are only computed when this
/// holds, so other projects do not pay for the per-class repository lookups.
pub(crate) fn is_doctrine_project(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            [
                "doctrine/orm",
                "doctrine/mongodb-odm",
                "doctrine/persistence",
                "doctrine/doctrine-bundle",
                "doctrine/mongodb-odm-bundle",
                "symfony/orm-pack",
            ]
            .iter()
            .any(|package| name.eq_ignore_ascii_case(package))
        })
}

/// Detect whether the project builds a Symfony service container: the full
/// framework, its bundle, or the dependency-injection component on its own.
///
/// Service IDs and container parameters are only read in projects where this
/// holds.  The other Symfony components (console, HTTP foundation, …) are
/// shared with Laravel and say nothing about a container.
pub(crate) fn is_symfony_project(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            [
                "symfony/symfony",
                "symfony/framework-bundle",
                "symfony/dependency-injection",
            ]
            .iter()
            .any(|package| name.eq_ignore_ascii_case(package))
        })
}

/// The packages that bring Twig into a project: Twig itself, its Symfony
/// bundle and bridge, and the frameworks whose templates are Twig.
const TWIG_PACKAGES: [&str; 7] = [
    "twig/twig",
    "symfony/twig-bundle",
    "symfony/twig-bridge",
    "slim/twig-view",
    "drupal/core",
    "drupal/core-recommended",
    "craftcms/cms",
];

/// Detect whether the project uses Twig, which template names are read in:
/// it requires Twig or a package whose templates are Twig, or is a Symfony
/// project.
pub(crate) fn is_twig_project(package: &ComposerPackage) -> bool {
    is_symfony_project(package)
        || package
            .require
            .keys()
            .chain(package.require_dev.keys())
            .any(|name| {
                TWIG_PACKAGES
                    .iter()
                    .any(|package| name.eq_ignore_ascii_case(package))
            })
}

/// The packages that make a project a Yii application: the view layer
/// itself and the renderer an application's actions use, under its current
/// and its former name, the router, the translator, the package that
/// attaches event listeners from configuration, and Active Record.
const YII_PACKAGES: [&str; 7] = [
    "yiisoft/view",
    "yiisoft/yii-view-renderer",
    "yiisoft/yii-view",
    "yiisoft/router",
    "yiisoft/translator",
    "yiisoft/yii-event",
    "yiisoft/active-record",
];

/// Detect whether the project is a Yii application, which view names, route
/// names, route arguments, message IDs, event listeners, and Active Record
/// attributes and relations are read in: it renders Yii views, routes
/// requests with Yii's router, translates with Yii's translator, attaches
/// listeners through `yiisoft/yii-event`, or keeps its data in Yii's Active
/// Record.
pub(crate) fn is_yii_project(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            YII_PACKAGES
                .iter()
                .any(|package| name.eq_ignore_ascii_case(package))
        })
}

/// The packages that render Tempest views: the standalone view renderer and
/// the framework that bundles it.
const TEMPEST_PACKAGES: [&str; 2] = ["tempest/view", "tempest/framework"];

/// Detect whether the project renders Tempest views, whose `.view.php`
/// files are templates rather than plain PHP.
pub(crate) fn is_tempest_project(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            TEMPEST_PACKAGES
                .iter()
                .any(|package| name.eq_ignore_ascii_case(package))
        })
}

/// Whether the package is a Drupal project: one that requires Drupal core.
pub(crate) fn is_drupal_project(package: &ComposerPackage) -> bool {
    const DRUPAL_PACKAGES: &[&str] = &["drupal/core", "drupal/core-recommended", "drupal/core-dev"];
    DRUPAL_PACKAGES
        .iter()
        .any(|pkg| package.require.contains_key(*pkg) || package.require_dev.contains_key(*pkg))
}

/// Detect whether the project is a Laravel *application* rather than a
/// library that uses an Illuminate component.
///
/// The distinction matters wherever the question is "does this project need
/// a Laravel-aware analyser?": plain PHPStan misreads an application's
/// Eloquent models, facades, and container bindings, but a library that
/// requires `illuminate/support` for its collections has no framework for
/// an extension to explain. [`is_laravel_project`] deliberately answers the
/// wider question of whether Laravel-aware *resolution* is worth doing at
/// all, which such a library does benefit from.
pub(crate) fn is_laravel_application(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            [
                "laravel/framework",
                "laravel/laravel",
                "illuminate/foundation",
            ]
            .iter()
            .any(|app| name.eq_ignore_ascii_case(app))
        })
}

/// Detect whether `package` describes an application rather than a library
/// that something else installs.
///
/// An application owns its configuration: the `config/` files it ships, plus
/// the framework defaults they merge with, are the whole of what exists, so a
/// key nothing declares is a typo.  A library's configuration belongs to
/// whatever application installs it, and that application is a file we never
/// see, so every key the library reads is unjudgeable.
///
/// Composer's `type` says which it is outright when it is set (`project` is
/// what the Laravel skeleton ships).  A `composer.json` that leaves the type
/// at its `library` default is read from what it requires: the framework
/// itself in `require` is an application, while a library depends on the
/// `illuminate/*` components it uses and keeps its copy of the framework in
/// `require-dev` for its test suite.  This is why it cannot share
/// [`is_laravel_application`], which counts a dev-only dependency too.
pub(crate) fn is_application_project(package: &ComposerPackage) -> bool {
    if let Some(kind) = &package.r#type {
        return kind.0.eq_ignore_ascii_case("project");
    }
    package.require.keys().any(|name| {
        ["laravel/framework", "laravel/laravel"]
            .iter()
            .any(|app| name.eq_ignore_ascii_case(app))
    })
}

/// Packages that answer authorization checks from a runtime permission
/// table rather than from `Gate::define()` calls or policy classes.
///
/// Each of these seeds its permissions in the database and hands the gate a
/// catch-all callback (`$gate->before(…)`) that resolves any ability name
/// against those rows, so the abilities an application checks appear nowhere
/// in its source.  A project depending on one has an open ability space.
const RUNTIME_PERMISSION_PACKAGES: &[&str] = &[
    "spatie/laravel-permission",
    "silber/bouncer",
    "santigarcor/laratrust",
    "zizaco/entrust",
    "kodeine/laravel-acl",
    "ultraware/roles",
    "jeremykenedy/laravel-roles",
];

/// Detect whether the project depends on a package that authorizes through a
/// runtime permission table.
///
/// Static scanning cannot enumerate what such a package accepts, so a
/// diagnostic that judges an ability name against the abilities found in
/// source has nothing to judge against and must stand down.  See
/// [`RUNTIME_PERMISSION_PACKAGES`].
pub(crate) fn has_runtime_permission_package(package: &ComposerPackage) -> bool {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .any(|name| {
            RUNTIME_PERMISSION_PACKAGES
                .iter()
                .any(|known| name.eq_ignore_ascii_case(known))
        })
}

/// The major version of PHPUnit the project's `composer.lock` installs,
/// `None` when there is no lock file or it does not list PHPUnit.
pub(crate) fn locked_phpunit_major(project_root: &Path) -> Option<u8> {
    let content = super::read_composer_file(&project_root.join("composer.lock"))?;
    phpunit_major_from_lock(&content)
}

/// [`locked_phpunit_major`] for the text of a `composer.lock`.
fn phpunit_major_from_lock(content: &str) -> Option<u8> {
    #[derive(serde::Deserialize)]
    struct Lock {
        #[serde(default)]
        packages: Vec<LockedPackage>,
        #[serde(default, rename = "packages-dev")]
        packages_dev: Vec<LockedPackage>,
    }
    #[derive(serde::Deserialize)]
    struct LockedPackage {
        name: String,
        #[serde(default)]
        version: String,
    }

    let lock: Lock = serde_json::from_str(content).ok()?;
    let package = lock
        .packages_dev
        .iter()
        .chain(&lock.packages)
        .find(|package| package.name.eq_ignore_ascii_case("phpunit/phpunit"))?;
    let version = package.version.trim_start_matches(['v', 'V']);
    version
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

pub(crate) fn explicit_dependency_names(package: &ComposerPackage) -> HashSet<String> {
    package
        .require
        .keys()
        .chain(package.require_dev.keys())
        .filter(|name| {
            !name.eq_ignore_ascii_case("php")
                && !name.starts_with("ext-")
                && !name.starts_with("lib-")
        })
        .cloned()
        .collect()
}

/// Detect whether the project is a Drupal project and resolve the web root.
///
/// Returns `Some(web_root)` if one of the canonical Drupal core packages is
/// listed in `require` or `require-dev`, and we can determine the web root.
///
/// **Web-root resolution order:**
/// 1. `extra.drupal-scaffold.locations.web-root` in `composer.json`.
/// 2. Filesystem fallback: [`drupal_web_root_on_disk`].
pub(crate) fn detect_drupal_web_root(
    workspace_root: &Path,
    package: &ComposerPackage,
) -> Option<PathBuf> {
    if !is_drupal_project(package) {
        return None;
    }

    // 1. Read from extra.drupal-scaffold.locations.web-root
    if let Some(ComposerPackageExtra::Object(extra_map)) = &package.extra
        && let Some(scaffold) = extra_map.get("drupal-scaffold")
        && let Some(web_root_str) = scaffold
            .get("locations")
            .and_then(|l| l.get("web-root"))
            .and_then(|v| v.as_str())
    {
        // `drupal/legacy-project` names the project root as `./`, which
        // must not leave a `.` in every path walked under it.
        let relative = web_root_str.trim_end_matches('/');
        let relative = relative.strip_prefix("./").unwrap_or(relative);
        if relative.is_empty() || relative == "." {
            return Some(workspace_root.to_path_buf());
        }
        return Some(workspace_root.join(relative));
    }

    // 2. Filesystem fallback
    drupal_web_root_on_disk(workspace_root)
}

/// The web root of the Drupal tree at `root`, found on disk: the first of
/// `web/`, `docroot/`, `public/` and `html/` that holds
/// `core/lib/Drupal.php`, else `root` itself when it does, as in a legacy
/// layout that keeps `core/` beside `composer.json`.
pub(crate) fn drupal_web_root_on_disk(root: &Path) -> Option<PathBuf> {
    ["web", "docroot", "public", "html"]
        .iter()
        .map(|candidate| root.join(candidate))
        .chain(std::iter::once(root.to_path_buf()))
        .find(|path| path.join("core/lib/Drupal.php").exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse a JSON string into a [`ComposerPackage`].
    fn pkg(json: &str) -> ComposerPackage {
        json.parse::<ComposerPackage>().unwrap()
    }

    // ── locked_phpunit_major ────────────────────────────────────────

    #[test]
    fn reads_the_locked_phpunit_major() {
        let lock = r#"{
            "packages": [{"name": "symfony/console", "version": "v7.1.0"}],
            "packages-dev": [{"name": "phpunit/phpunit", "version": "10.5.38"}]
        }"#;
        assert_eq!(phpunit_major_from_lock(lock), Some(10));
        let lock = r#"{"packages": [{"name": "phpunit/phpunit", "version": "v9.6.0"}]}"#;
        assert_eq!(phpunit_major_from_lock(lock), Some(9));
    }

    #[test]
    fn a_lock_without_phpunit_has_no_phpunit_major() {
        let lock = r#"{"packages": [], "packages-dev": [{"name": "mockery/mockery", "version": "1.6.0"}]}"#;
        assert_eq!(phpunit_major_from_lock(lock), None);
        assert_eq!(phpunit_major_from_lock("not json"), None);
        let lock = r#"{"packages-dev": [{"name": "phpunit/phpunit", "version": "dev-main"}]}"#;
        assert_eq!(phpunit_major_from_lock(lock), None);
    }

    // ── is_doctrine_project ─────────────────────────────────────────

    #[test]
    fn detects_doctrine_through_the_orm_or_the_symfony_bundle() {
        assert!(is_doctrine_project(&pkg(
            r#"{"require": {"doctrine/orm": "^3.0"}}"#
        )));
        assert!(is_doctrine_project(&pkg(
            r#"{"require": {"doctrine/doctrine-bundle": "^2.10"}}"#
        )));
    }

    #[test]
    fn dbal_alone_is_not_doctrine_repositories() {
        assert!(!is_doctrine_project(&pkg(
            r#"{"require": {"doctrine/dbal": "^4.0", "laravel/framework": "^11.0"}}"#
        )));
    }

    // ── is_symfony_project ──────────────────────────────────────────

    #[test]
    fn detects_symfony_through_the_bundle_or_the_container_component() {
        assert!(is_symfony_project(&pkg(
            r#"{"require": {"symfony/framework-bundle": "^7.3"}}"#
        )));
        assert!(is_symfony_project(&pkg(
            r#"{"require": {"symfony/dependency-injection": "^7.3"}}"#
        )));
    }

    #[test]
    fn laravel_and_its_symfony_components_are_not_symfony() {
        assert!(!is_symfony_project(&pkg(
            r#"{"require": {"laravel/framework": "^11.0", "symfony/console": "^7.0"}}"#
        )));
    }

    // ── is_twig_project ─────────────────────────────────────────────

    #[test]
    fn detects_twig_on_its_own_and_in_symfony() {
        assert!(is_twig_project(&pkg(
            r#"{"require": {"twig/twig": "^3.4"}}"#
        )));
        assert!(is_twig_project(&pkg(
            r#"{"require": {"symfony/framework-bundle": "^7.3"}}"#
        )));
        assert!(is_twig_project(&pkg(
            r#"{"require": {"drupal/core-recommended": "^11.0"}}"#
        )));
        assert!(!is_twig_project(&pkg(
            r#"{"require": {"laravel/framework": "^11.0", "symfony/console": "^7.0"}}"#
        )));
    }

    // ── is_yii_project ──────────────────────────────────────────────

    #[test]
    fn detects_yii_through_its_router() {
        assert!(is_yii_project(&pkg(
            r#"{"require": {"yiisoft/router": "^4.0"}}"#
        )));
        assert!(!is_yii_project(&pkg(
            r#"{"require": {"yiisoft/arrays": "^3.0"}}"#
        )));
    }

    // ── is_tempest_project ──────────────────────────────────────────

    #[test]
    fn detects_tempest_through_its_view_renderer_or_framework() {
        assert!(is_tempest_project(&pkg(
            r#"{"require": {"tempest/view": "^3.0"}}"#
        )));
        assert!(is_tempest_project(&pkg(
            r#"{"require": {"tempest/framework": "^3.0"}}"#
        )));
        assert!(!is_tempest_project(&pkg(
            r#"{"require": {"laravel/framework": "^11.0"}}"#
        )));
    }

    // ── is_laravel_project ──────────────────────────────────────────

    #[test]
    fn detects_laravel_framework() {
        let p = pkg(r#"{"require": {"php": "^8.2", "laravel/framework": "^11.0"}}"#);
        assert!(is_laravel_project(&p));
    }

    #[test]
    fn detects_standalone_illuminate_component() {
        let p = pkg(r#"{"require": {"illuminate/database": "^11.0"}}"#);
        assert!(is_laravel_project(&p));
    }

    #[test]
    fn detects_laravel_in_require_dev() {
        let p = pkg(r#"{"require-dev": {"laravel/framework": "^11.0"}}"#);
        assert!(is_laravel_project(&p));
    }

    #[test]
    fn non_laravel_project_is_not_detected() {
        let p = pkg(r#"{"require": {"php": "^8.2", "symfony/console": "^7.0"}}"#);
        assert!(!is_laravel_project(&p));
    }

    // ── is_laravel_application ──────────────────────────────────────

    /// An application is the framework, or the skeleton, or the component
    /// that boots one.  A library reaching for a single Illuminate
    /// component is not one, even though Laravel-aware resolution still
    /// helps it.
    #[test]
    fn an_illuminate_component_alone_is_not_an_application() {
        for json in [
            r#"{"require": {"laravel/framework": "^11.0"}}"#,
            r#"{"require": {"laravel/laravel": "^11.0"}}"#,
            r#"{"require": {"illuminate/foundation": "^11.0"}}"#,
        ] {
            assert!(is_laravel_application(&pkg(json)), "for {json}");
        }

        let library = pkg(r#"{"require": {"illuminate/support": "^11.0"}}"#);
        assert!(is_laravel_project(&library));
        assert!(!is_laravel_application(&library));
    }

    // ── is_application_project ──────────────────────────────────────

    /// The declared type settles it on its own, whichever way it points.
    #[test]
    fn a_declared_type_decides_whether_a_project_is_an_application() {
        assert!(is_application_project(&pkg(
            r#"{"type": "project", "require": {"illuminate/support": "^11.0"}}"#
        )));
        assert!(!is_application_project(&pkg(
            r#"{"type": "library", "require": {"laravel/framework": "^11.0"}}"#
        )));
    }

    /// Without one, requiring the framework itself is what tells an
    /// application apart from a package that keeps its copy for tests.
    #[test]
    fn an_untyped_package_is_read_from_what_it_requires() {
        assert!(is_application_project(&pkg(
            r#"{"require": {"laravel/framework": "^11.0"}}"#
        )));
        assert!(!is_application_project(&pkg(
            r#"{"require": {"illuminate/support": "^11.0"},
                "require-dev": {"laravel/framework": "^11.0"}}"#
        )));
        assert!(!is_application_project(&pkg(
            r#"{"require": {"symfony/console": "^7.0"}}"#
        )));
    }

    // ── has_runtime_permission_package ──────────────────────────────

    #[test]
    fn detects_a_runtime_permission_package() {
        let p = pkg(
            r#"{"require": {"laravel/framework": "^11.0", "spatie/laravel-permission": "^6.0"}}"#,
        );
        assert!(has_runtime_permission_package(&p));
    }

    #[test]
    fn detects_a_runtime_permission_package_in_require_dev() {
        let p = pkg(r#"{"require-dev": {"silber/bouncer": "^1.0"}}"#);
        assert!(has_runtime_permission_package(&p));
    }

    #[test]
    fn a_plain_laravel_project_has_no_runtime_permission_package() {
        let p =
            pkg(r#"{"require": {"laravel/framework": "^11.0", "spatie/laravel-data": "^4.0"}}"#);
        assert!(!has_runtime_permission_package(&p));
    }

    // ── detect_drupal_web_root ──────────────────────────────────────

    #[test]
    fn drupal_not_detected_without_drupal_packages() {
        let dir = tempfile::tempdir().unwrap();
        let p = pkg(r#"{"require": {"php": "^8.1", "some/package": "^1.0"}}"#);
        assert!(detect_drupal_web_root(dir.path(), &p).is_none());
    }

    #[test]
    fn drupal_detected_via_drupal_core() {
        let dir = tempfile::tempdir().unwrap();
        // Create web/core/lib/Drupal.php for the filesystem fallback
        let web = dir.path().join("web").join("core").join("lib");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("Drupal.php"), "<?php\nclass Drupal {}").unwrap();

        let p = pkg(r#"{"require": {"drupal/core": "^10.0"}}"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        assert_eq!(result, Some(dir.path().join("web")));
    }

    #[test]
    fn drupal_detected_via_core_recommended_in_require_dev() {
        let dir = tempfile::tempdir().unwrap();
        let docroot = dir.path().join("docroot").join("core").join("lib");
        std::fs::create_dir_all(&docroot).unwrap();
        std::fs::write(docroot.join("Drupal.php"), "<?php").unwrap();

        let p = pkg(r#"{"require-dev": {"drupal/core-recommended": "^10.0"}}"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        assert_eq!(result, Some(dir.path().join("docroot")));
    }

    #[test]
    fn drupal_detected_via_core_dev() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web").join("core").join("lib");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("Drupal.php"), "<?php").unwrap();

        let p = pkg(r#"{"require-dev": {"drupal/core-dev": "^10.0"}}"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        assert_eq!(result, Some(dir.path().join("web")));
    }

    #[test]
    fn drupal_web_root_from_scaffold_config() {
        let dir = tempfile::tempdir().unwrap();
        // No filesystem dirs needed — scaffold config takes priority
        let p = pkg(r#"{
            "require": {"drupal/core": "^10.0"},
            "extra": {
                "drupal-scaffold": {
                    "locations": {
                        "web-root": "docroot/"
                    }
                }
            }
        }"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        // Trailing slash should be stripped
        assert_eq!(result, Some(dir.path().join("docroot")));
    }

    #[test]
    fn drupal_scaffold_config_takes_priority_over_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        // Create web/core/lib/Drupal.php so filesystem fallback would find "web"
        let web = dir.path().join("web").join("core").join("lib");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("Drupal.php"), "<?php").unwrap();

        let p = pkg(r#"{
            "require": {"drupal/core": "^10.0"},
            "extra": {
                "drupal-scaffold": {
                    "locations": {
                        "web-root": "custom_root"
                    }
                }
            }
        }"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        // Scaffold config wins over filesystem fallback
        assert_eq!(result, Some(dir.path().join("custom_root")));
    }

    #[test]
    fn drupal_filesystem_fallback_tries_candidates_in_order() {
        let dir = tempfile::tempdir().unwrap();
        // Only create public/core/lib/Drupal.php (third candidate)
        let public = dir.path().join("public").join("core").join("lib");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(public.join("Drupal.php"), "<?php").unwrap();

        let p = pkg(r#"{"require": {"drupal/core": "^10.0"}}"#);
        let result = detect_drupal_web_root(dir.path(), &p);
        assert_eq!(result, Some(dir.path().join("public")));
    }

    /// Write `<dir>/core/lib/Drupal.php`.
    fn write_core(dir: &Path) {
        let lib = dir.join("core").join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Drupal.php"), "<?php").unwrap();
    }

    #[test]
    fn drupal_core_beside_composer_json_makes_the_project_root_the_web_root() {
        let dir = tempfile::tempdir().unwrap();
        write_core(dir.path());

        let p = pkg(r#"{"require": {"drupal/core": "^10.0"}}"#);
        assert_eq!(
            detect_drupal_web_root(dir.path(), &p),
            Some(dir.path().to_path_buf())
        );
    }

    #[test]
    fn a_scaffold_entry_or_a_web_folder_wins_over_core_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        write_core(dir.path());
        write_core(&dir.path().join("html"));

        let p = pkg(r#"{"require": {"drupal/core": "^10.0"}}"#);
        assert_eq!(
            detect_drupal_web_root(dir.path(), &p),
            Some(dir.path().join("html"))
        );

        let p = pkg(r#"{
            "require": {"drupal/core": "^10.0"},
            "extra": {"drupal-scaffold": {"locations": {"web-root": "docroot/"}}}
        }"#);
        assert_eq!(
            detect_drupal_web_root(dir.path(), &p),
            Some(dir.path().join("docroot"))
        );
    }

    #[test]
    fn a_scaffold_entry_naming_the_project_root_gives_the_root_as_written() {
        let dir = tempfile::tempdir().unwrap();
        for web_root in ["./", ".", ""] {
            let p = pkg(&format!(
                r#"{{
                    "require": {{"drupal/core": "^10.0"}},
                    "extra": {{"drupal-scaffold": {{"locations": {{"web-root": "{web_root}"}}}}}}
                }}"#
            ));
            // Compared as text: `Path` equality ignores a trailing `.`.
            assert_eq!(
                detect_drupal_web_root(dir.path(), &p).map(PathBuf::into_os_string),
                Some(dir.path().as_os_str().to_owned()),
                "{web_root:?}"
            );
        }
    }

    #[test]
    fn drupal_returns_none_when_no_web_root_found() {
        let dir = tempfile::tempdir().unwrap();
        // Drupal package present but no scaffold config and no matching dirs
        let p = pkg(r#"{"require": {"drupal/core": "^10.0"}}"#);
        assert!(detect_drupal_web_root(dir.path(), &p).is_none());
    }
}
