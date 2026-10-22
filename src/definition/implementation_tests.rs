use std::fs;

use tower_lsp::lsp_types::{Position, Url};

use super::*;
use crate::config::{Config, IndexingStrategy};

#[test]
fn full_indexed_implementation_uses_gti_without_vendor_fallback() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    let vendor = dir.path().join("vendor");
    fs::create_dir_all(src.join("Contracts")).expect("src contracts dir");
    fs::create_dir_all(src.join("Impl")).expect("src impl dir");
    fs::create_dir_all(vendor.join("Pkg")).expect("vendor pkg dir");

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Service {}\n",
    );
    let user_impl_php = concat!(
        "<?php\n",
        "namespace App\\Impl;\n",
        "use App\\Contracts\\Service;\n",
        "class UserService implements Service {}\n",
    );
    let vendor_impl_php = concat!(
        "<?php\n",
        "namespace Vendor\\Pkg;\n",
        "use App\\Contracts\\Service;\n",
        "class VendorService implements Service {}\n",
    );

    let interface_path = src.join("Contracts/Service.php");
    let user_impl_path = src.join("Impl/UserService.php");
    let vendor_impl_path = vendor.join("Pkg/VendorService.php");
    fs::write(&interface_path, interface_php).expect("interface file");
    fs::write(&user_impl_path, user_impl_php).expect("user impl file");
    fs::write(&vendor_impl_path, vendor_impl_php).expect("vendor impl file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.add_vendor_dir(&vendor);
    let mut config = Config::default();
    config.indexing.strategy = Some(IndexingStrategy::Full);
    backend.set_config(config);

    backend.symbols.fqn_uri_index.write().insert(
        "Vendor\\Pkg\\VendorService".to_string(),
        Url::from_file_path(&vendor_impl_path)
            .expect("vendor uri")
            .to_string(),
    );

    let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
    backend.update_ast(interface_uri.as_str(), interface_php);

    let locations = backend
        .resolve_implementation(
            interface_uri.as_str(),
            interface_php,
            Position {
                line: 2,
                character: 12,
            },
        )
        .expect("user implementation should be found");

    assert_eq!(
        locations.len(),
        1,
        "full index should use GTI and avoid vendor fallback results: {locations:?}",
    );
    assert_eq!(
        locations[0].uri,
        Url::from_file_path(&user_impl_path).expect("user impl uri")
    );
}

#[test]
fn non_full_indexed_implementation_uses_ready_gti_without_fallback() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    let vendor = dir.path().join("vendor");
    fs::create_dir_all(src.join("Contracts")).expect("src contracts dir");
    fs::create_dir_all(src.join("Impl")).expect("src impl dir");
    fs::create_dir_all(vendor.join("Pkg")).expect("vendor pkg dir");

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface Service {}\n",
    );
    let user_impl_php = concat!(
        "<?php\n",
        "namespace App\\Impl;\n",
        "use App\\Contracts\\Service;\n",
        "class UserService implements Service {}\n",
    );
    let vendor_impl_php = concat!(
        "<?php\n",
        "namespace Vendor\\Pkg;\n",
        "use App\\Contracts\\Service;\n",
        "class VendorService implements Service {}\n",
    );

    let interface_path = src.join("Contracts/Service.php");
    let user_impl_path = src.join("Impl/UserService.php");
    let vendor_impl_path = vendor.join("Pkg/VendorService.php");
    fs::write(&interface_path, interface_php).expect("interface file");
    fs::write(&user_impl_path, user_impl_php).expect("user impl file");
    fs::write(&vendor_impl_path, vendor_impl_php).expect("vendor impl file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    backend.add_vendor_dir(&vendor);
    let mut config = Config::default();
    config.indexing.strategy = Some(IndexingStrategy::Composer);
    backend.set_config(config);
    backend
        .workspace_index
        .indexed
        .store(true, std::sync::atomic::Ordering::Release);

    backend.symbols.fqn_uri_index.write().insert(
        "Vendor\\Pkg\\VendorService".to_string(),
        Url::from_file_path(&vendor_impl_path)
            .expect("vendor uri")
            .to_string(),
    );

    let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
    let user_impl_uri = Url::from_file_path(&user_impl_path).expect("user impl uri");
    backend.update_ast(interface_uri.as_str(), interface_php);
    backend.update_ast(user_impl_uri.as_str(), user_impl_php);

    let locations = backend
        .resolve_implementation(
            interface_uri.as_str(),
            interface_php,
            Position {
                line: 2,
                character: 12,
            },
        )
        .expect("user implementation should be found from the ready GTI index");

    assert_eq!(
        locations.len(),
        1,
        "ready non-full indexing should return GTI results without falling back to vendor scans"
    );
    assert_eq!(locations[0].uri, user_impl_uri);
}

#[test]
fn same_short_name_interface_and_implementation_found() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("Contracts")).expect("contracts dir");
    fs::create_dir_all(src.join("Foo")).expect("foo dir");

    let interface_php = concat!(
        "<?php\n",
        "namespace App\\Contracts;\n",
        "interface HttpClient {}\n",
    );
    let impl_php = concat!(
        "<?php\n",
        "namespace App\\Foo;\n",
        "use App\\Contracts\\HttpClient as HttpClientInterface;\n",
        "class HttpClient implements HttpClientInterface {}\n",
    );

    let interface_path = src.join("Contracts/HttpClient.php");
    let impl_path = src.join("Foo/HttpClient.php");
    fs::write(&interface_path, interface_php).expect("interface file");
    fs::write(&impl_path, impl_php).expect("impl file");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), Vec::new());
    let mut config = Config::default();
    config.indexing.strategy = Some(IndexingStrategy::Full);
    backend.set_config(config);

    let interface_uri = Url::from_file_path(&interface_path).expect("interface uri");
    let impl_uri = Url::from_file_path(&impl_path).expect("impl uri");
    backend.update_ast(interface_uri.as_str(), interface_php);
    backend.update_ast(impl_uri.as_str(), impl_php);

    let locations = backend
        .resolve_implementation(
            interface_uri.as_str(),
            interface_php,
            Position {
                line: 2,
                character: 12,
            },
        )
        .expect("implementation with same short name should be found");

    assert_eq!(
        locations.len(),
        1,
        "should find exactly one implementation: {locations:?}",
    );
    assert_eq!(locations[0].uri, impl_uri);
}

// ─── Method implementations across vendor and inheritance ───────────

const VENDOR_IFACE: &str = "vendor/symfony/http-kernel/HttpKernelInterface.php";
const VENDOR_IMPL: &str = "vendor/symfony/http-kernel/HttpKernel.php";
const VENDOR_SUB_IFACE: &str = "vendor/symfony/http-kernel/KernelInterface.php";
const VENDOR_ABSTRACT: &str = "vendor/symfony/http-kernel/Kernel.php";
const PROJECT_KERNEL: &str = "src/Kernel.php";

/// Build a backend over a temp workspace holding `files` (relative
/// path, content), with `vendor/` registered as a vendor directory and
/// every `(fqn, relative path)` in `indexed` present in the FQN → URI
/// index the way the Composer classmap scan leaves it.
fn scenario_workspace(
    files: &[(&str, &str)],
    indexed: &[(&str, &str)],
) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("composer.json");
    for (rel, content) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().expect("file parent")).expect("file dirs");
        fs::write(&path, content).expect("php file");
    }

    let (mappings, _vendor_dir) = crate::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    backend.add_vendor_dir(&dir.path().join("vendor"));
    let mut config = Config::default();
    config.indexing.strategy = Some(IndexingStrategy::Full);
    backend.set_config(config);

    {
        let mut idx = backend.symbols.fqn_uri_index.write();
        for (fqn, rel) in indexed {
            idx.insert(
                (*fqn).to_string(),
                Url::from_file_path(dir.path().join(rel))
                    .expect("indexed uri")
                    .to_string(),
            );
        }
    }

    (backend, dir)
}

/// Parse `rel` into the backend the way opening it in the editor does,
/// returning its URI and content.
fn open_file(backend: &Backend, dir: &std::path::Path, rel: &str) -> (Url, String) {
    let path = dir.join(rel);
    let content = fs::read_to_string(&path).expect("read php file");
    let uri = Url::from_file_path(&path).expect("file uri");
    backend.update_ast(uri.as_str(), &content);
    (uri, content)
}

/// The workspace-relative files the returned locations point at.
fn located_files(locations: &[Location], dir: &std::path::Path) -> Vec<String> {
    locations
        .iter()
        .map(|loc| {
            let path = loc.uri.to_file_path().expect("location path");
            path.strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// A Symfony-like `vendor/` tree: an interface, a concrete direct
/// implementor, a sub-interface, an abstract class whose `handle()` is
/// concrete, and a project subclass that only inherits `handle()`.
fn symfony_like_workspace() -> (Backend, tempfile::TempDir) {
    scenario_workspace(
        &[
            (
                VENDOR_IFACE,
                concat!(
                    "<?php\n",
                    "namespace Symfony\\Component\\HttpKernel;\n",
                    "interface HttpKernelInterface\n",
                    "{\n",
                    "    public function handle(string $request): string;\n",
                    "}\n",
                ),
            ),
            (
                VENDOR_IMPL,
                concat!(
                    "<?php\n",
                    "namespace Symfony\\Component\\HttpKernel;\n",
                    "class HttpKernel implements HttpKernelInterface\n",
                    "{\n",
                    "    public function handle(string $request): string\n",
                    "    {\n",
                    "        return 'response';\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                VENDOR_SUB_IFACE,
                concat!(
                    "<?php\n",
                    "namespace Symfony\\Component\\HttpKernel;\n",
                    "interface KernelInterface extends HttpKernelInterface\n",
                    "{\n",
                    "}\n",
                ),
            ),
            (
                VENDOR_ABSTRACT,
                concat!(
                    "<?php\n",
                    "namespace Symfony\\Component\\HttpKernel;\n",
                    "abstract class Kernel implements KernelInterface\n",
                    "{\n",
                    "    public function handle(string $request): string\n",
                    "    {\n",
                    "        return 'kernel';\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                PROJECT_KERNEL,
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "use Symfony\\Component\\HttpKernel\\Kernel as BaseKernel;\n",
                    "class Kernel extends BaseKernel\n",
                    "{\n",
                    "}\n",
                ),
            ),
        ],
        &[
            (
                "Symfony\\Component\\HttpKernel\\HttpKernelInterface",
                VENDOR_IFACE,
            ),
            ("Symfony\\Component\\HttpKernel\\HttpKernel", VENDOR_IMPL),
            (
                "Symfony\\Component\\HttpKernel\\KernelInterface",
                VENDOR_SUB_IFACE,
            ),
            ("Symfony\\Component\\HttpKernel\\Kernel", VENDOR_ABSTRACT),
            ("App\\Kernel", PROJECT_KERNEL),
        ],
    )
}

/// A method declared on a vendor interface resolves to the concrete
/// implementations that vendor package ships.  The workspace index
/// covers project files only, so the vendor implementors have to come
/// from the class-index scans.
#[test]
fn vendor_interface_method_finds_vendor_implementations() {
    let (backend, dir) = symfony_like_workspace();
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

    // Cursor on `handle` in `public function handle(...)` (line 4).
    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 4,
                character: 22,
            },
        )
        .expect("vendor implementations of HttpKernelInterface::handle should be found");

    let files = located_files(&locations, dir.path());
    assert!(
        files.contains(&VENDOR_IMPL.to_string()),
        "the concrete direct implementor HttpKernel::handle should be returned: {files:?}"
    );
    assert!(
        !files.contains(&VENDOR_IFACE.to_string()),
        "the queried interface itself is not an implementation: {files:?}"
    );
}

/// The class-level request on a vendor interface returns the vendor
/// classes that implement it, not an empty list.
#[test]
fn vendor_interface_class_finds_vendor_implementors() {
    let (backend, dir) = symfony_like_workspace();
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

    // Cursor on `HttpKernelInterface` in the interface declaration.
    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 2,
                character: 12,
            },
        )
        .expect("vendor implementors of HttpKernelInterface should be found");

    let files = located_files(&locations, dir.path());
    assert!(
        files.contains(&VENDOR_IMPL.to_string()),
        "HttpKernel implements the interface directly: {files:?}"
    );
    assert!(
        files.contains(&PROJECT_KERNEL.to_string()),
        "App\\Kernel implements it through the vendor base kernel: {files:?}"
    );
}

/// The vendor scan parses only files that name the target or a subtype
/// found so far.  A class whose file never names the target is still
/// found through the intermediate class it extends, including an
/// already-open one whose parent nothing had parsed yet.
#[test]
fn vendor_scan_follows_subtypes_without_parsing_unrelated_files() {
    const TARGET: &str = "vendor/acme/queue/src/ShouldQueue.php";
    const BASE: &str = "vendor/acme/queue/src/QueuedJob.php";
    const LEAF: &str = "vendor/acme/queue/src/RetryingJob.php";
    const UNRELATED: &str = "vendor/acme/queue/src/Clock.php";
    const PROJECT_JOB: &str = "src/SendMail.php";
    let (backend, dir) = scenario_workspace(
        &[
            (
                TARGET,
                "<?php\nnamespace Acme\\Queue;\ninterface ShouldQueue\n{\n}\n",
            ),
            (
                BASE,
                "<?php\nnamespace Acme\\Queue;\nabstract class QueuedJob implements ShouldQueue\n{\n}\n",
            ),
            (
                LEAF,
                "<?php\nnamespace Acme\\Queue;\nclass RetryingJob extends QueuedJob\n{\n}\n",
            ),
            (
                UNRELATED,
                "<?php\nnamespace Acme\\Queue;\nclass Clock\n{\n}\n",
            ),
            (
                PROJECT_JOB,
                "<?php\nnamespace App;\nuse Acme\\Queue\\RetryingJob;\nclass SendMail extends RetryingJob\n{\n}\n",
            ),
        ],
        &[
            ("Acme\\Queue\\ShouldQueue", TARGET),
            ("Acme\\Queue\\QueuedJob", BASE),
            ("Acme\\Queue\\RetryingJob", LEAF),
            ("Acme\\Queue\\Clock", UNRELATED),
            ("App\\SendMail", PROJECT_JOB),
        ],
    );
    let (target_uri, target_text) = open_file(&backend, dir.path(), TARGET);
    open_file(&backend, dir.path(), PROJECT_JOB);

    // Cursor on `ShouldQueue` in the interface declaration.
    let locations = backend
        .resolve_implementation(
            target_uri.as_str(),
            &target_text,
            Position {
                line: 2,
                character: 12,
            },
        )
        .expect("implementors of ShouldQueue should be found");

    let mut files = located_files(&locations, dir.path());
    files.sort();
    assert_eq!(files, vec![PROJECT_JOB.to_string(), LEAF.to_string()]);

    let unrelated_uri = crate::util::path_to_uri(&dir.path().join(UNRELATED));
    assert!(
        !backend.parsed_uris.read().contains(&unrelated_uri),
        "a vendor file that names no subtype should not be parsed"
    );
}

/// An abstract class whose method body is concrete is a valid method
/// implementation: `Symfony\Component\HttpKernel\Kernel` is abstract but
/// owns a concrete `handle()`.
#[test]
fn abstract_class_with_concrete_method_is_an_implementation() {
    let (backend, dir) = symfony_like_workspace();
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), VENDOR_IFACE);

    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 4,
                character: 22,
            },
        )
        .expect("implementations of HttpKernelInterface::handle should be found");

    let files = located_files(&locations, dir.path());
    assert!(
        files.contains(&VENDOR_ABSTRACT.to_string()),
        "the abstract Kernel owns the concrete handle() body: {files:?}"
    );
}

/// A concrete subclass that inherits the method without overriding it
/// resolves to the nearest ancestor that actually declares the method.
#[test]
fn inherited_method_resolves_to_declaring_ancestor() {
    let (backend, dir) = scenario_workspace(
        &[
            (
                "src/Handler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "interface Handler\n",
                    "{\n",
                    "    public function handle(): void;\n",
                    "}\n",
                ),
            ),
            // Declares handle() but is unrelated to Handler, so it is
            // never itself an implementor of the interface.
            (
                "src/BaseHandler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "abstract class BaseHandler\n",
                    "{\n",
                    "    public function handle(): void\n",
                    "    {\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                "src/AppHandler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class AppHandler extends BaseHandler implements Handler\n",
                    "{\n",
                    "}\n",
                ),
            ),
        ],
        &[],
    );
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), "src/Handler.php");

    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 4,
                character: 22,
            },
        )
        .expect("the inherited implementation should resolve to its declaring class");

    let files = located_files(&locations, dir.path());
    assert_eq!(
        files,
        vec!["src/BaseHandler.php".to_string()],
        "only the class that declares handle() has a body to jump to: {files:?}"
    );
    assert_eq!(
        locations[0].range.start.line, 4,
        "should point at BaseHandler::handle: {locations:?}"
    );
}

/// An abstract re-declaration is another declaration, not an
/// implementation, so only the class with the body is returned.
#[test]
fn abstract_method_redeclaration_is_not_an_implementation() {
    let (backend, dir) = scenario_workspace(
        &[
            (
                "src/Handler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "interface Handler\n",
                    "{\n",
                    "    public function handle(): void;\n",
                    "}\n",
                ),
            ),
            (
                "src/AbstractHandler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "abstract class AbstractHandler implements Handler\n",
                    "{\n",
                    "    abstract public function handle(): void;\n",
                    "}\n",
                ),
            ),
            (
                "src/RealHandler.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class RealHandler extends AbstractHandler\n",
                    "{\n",
                    "    public function handle(): void\n",
                    "    {\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
        &[],
    );
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), "src/Handler.php");

    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 4,
                character: 22,
            },
        )
        .expect("the concrete implementation should be found");

    let files = located_files(&locations, dir.path());
    assert_eq!(
        files,
        vec!["src/RealHandler.php".to_string()],
        "the abstract re-declaration is not an implementation: {files:?}"
    );
}

/// A vendor class implementing a *project* interface stays excluded:
/// the project-only restriction still applies when the queried symbol
/// belongs to the project.
#[test]
fn project_interface_method_still_excludes_vendor_implementors() {
    let (backend, dir) = scenario_workspace(
        &[
            (
                "src/Contracts/Cacheable.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Contracts;\n",
                    "interface Cacheable\n",
                    "{\n",
                    "    public function cacheKey(): string;\n",
                    "}\n",
                ),
            ),
            (
                "src/Cache/FileCache.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Cache;\n",
                    "use App\\Contracts\\Cacheable;\n",
                    "class FileCache implements Cacheable\n",
                    "{\n",
                    "    public function cacheKey(): string\n",
                    "    {\n",
                    "        return 'file';\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                "vendor/acme/cache/src/AcmeCache.php",
                concat!(
                    "<?php\n",
                    "namespace Acme\\Cache;\n",
                    "use App\\Contracts\\Cacheable;\n",
                    "class AcmeCache implements Cacheable\n",
                    "{\n",
                    "    public function cacheKey(): string\n",
                    "    {\n",
                    "        return 'acme';\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
        &[
            ("App\\Contracts\\Cacheable", "src/Contracts/Cacheable.php"),
            ("App\\Cache\\FileCache", "src/Cache/FileCache.php"),
            (
                "Acme\\Cache\\AcmeCache",
                "vendor/acme/cache/src/AcmeCache.php",
            ),
        ],
    );

    // The vendor implementor was parsed earlier in the session, which
    // puts it into the reverse-inheritance index.
    open_file(&backend, dir.path(), "vendor/acme/cache/src/AcmeCache.php");
    let (iface_uri, iface_text) = open_file(&backend, dir.path(), "src/Contracts/Cacheable.php");

    let locations = backend
        .resolve_implementation(
            iface_uri.as_str(),
            &iface_text,
            Position {
                line: 4,
                character: 22,
            },
        )
        .expect("the project implementation should be found");

    let files = located_files(&locations, dir.path());
    assert_eq!(
        files,
        vec!["src/Cache/FileCache.php".to_string()],
        "a vendor implementor of a project interface must stay excluded: {files:?}"
    );
}
