use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn declaration_at(
    backend: &phpantom_lsp::Backend,
    uri: &Uri,
    line: u32,
    character: u32,
) -> Vec<Location> {
    let params = DeclarationParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    match backend.goto_declaration(params).await.unwrap() {
        Some(DeclarationResponse::Declaration(Declaration::Location(loc))) => vec![loc],
        Some(DeclarationResponse::Declaration(Declaration::LocationList(locs))) => locs,
        Some(DeclarationResponse::DeclarationLinkList(links)) => links
            .into_iter()
            .map(|l| Location {
                uri: l.target_uri,
                range: l.target_selection_range,
            })
            .collect(),
        None => vec![],
    }
}

fn lines(locations: &[Location]) -> Vec<u32> {
    locations.iter().map(|l| l.range.start.line).collect()
}

const RENDERERS: &str = concat!(
    "<?php\n",                                                   // 0
    "interface Renderable {\n",                                  // 1
    "    public function render(): string;\n",                   // 2
    "}\n",                                                       // 3
    "abstract class BaseView {\n",                               // 4
    "    abstract public function title(): string;\n",           // 5
    "}\n",                                                       // 6
    "class HtmlView extends BaseView implements Renderable {\n", // 7
    "    public function render(): string { return ''; }\n",     // 8
    "    public function title(): string { return ''; }\n",      // 9
    "    public function extra(): string { return ''; }\n",      // 10
    "}\n",                                                       // 11
    "class Page extends HtmlView {}\n",                          // 12
    "function demo(HtmlView $view, Page $page): void {\n",       // 13
    "    $view->render();\n",                                    // 14
    "    $view->title();\n",                                     // 15
    "    $view->extra();\n",                                     // 16
    "    $page->render();\n",                                    // 17
    "    $x = new HtmlView();\n",                                // 18
    "}\n",                                                       // 19
);

// ─── Member calls ───────────────────────────────────────────────────────────

/// A call on a concrete class lands on the interface method it implements,
/// where go-to-definition lands on the concrete method.
#[tokio::test]
async fn test_declaration_method_call_jumps_to_interface() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_call_iface.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    let locations = declaration_at(&backend, &uri, 14, 12).await;
    assert_eq!(lines(&locations), vec![2]);
}

/// A call lands on the abstract method of a parent class.
#[tokio::test]
async fn test_declaration_method_call_jumps_to_abstract_parent() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_call_abstract.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    let locations = declaration_at(&backend, &uri, 15, 12).await;
    assert_eq!(lines(&locations), vec![5]);
}

/// A method inherited by a subclass still reaches the prototype of the
/// class that defines it.
#[tokio::test]
async fn test_declaration_inherited_method_call_jumps_to_interface() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_call_inherited.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    let locations = declaration_at(&backend, &uri, 17, 12).await;
    assert_eq!(lines(&locations), vec![2]);
}

/// A method with no prototype answers the same as go-to-definition.
#[tokio::test]
async fn test_declaration_without_prototype_falls_back_to_definition() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_call_no_proto.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    let locations = declaration_at(&backend, &uri, 16, 12).await;
    assert_eq!(lines(&locations), vec![10]);
}

// ─── Member declarations ────────────────────────────────────────────────────

/// On a method's own declaration, the jump goes to its prototype.
#[tokio::test]
async fn test_declaration_on_member_declaration_jumps_to_prototype() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_member_decl.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    assert_eq!(lines(&declaration_at(&backend, &uri, 8, 22).await), vec![2]);
    assert_eq!(lines(&declaration_at(&backend, &uri, 9, 22).await), vec![5]);
}

/// A declaration without a prototype answers with itself, as
/// go-to-definition does.
#[tokio::test]
async fn test_declaration_on_member_declaration_without_prototype() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_member_decl_none.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    assert_eq!(
        lines(&declaration_at(&backend, &uri, 10, 22).await),
        vec![10]
    );
}

/// An interface method that re-declares one from a parent interface jumps
/// to the parent's.
#[tokio::test]
async fn test_declaration_interface_method_jumps_to_parent_interface() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_iface_parent.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "interface Shape {\n",                  // 1
        "    public function area(): float;\n", // 2
        "}\n",                                  // 3
        "interface Polygon extends Shape {\n",  // 4
        "    public function area(): float;\n", // 5
        "}\n",                                  // 6
    );
    open_php(&backend, &uri, text).await;

    assert_eq!(lines(&declaration_at(&backend, &uri, 5, 22).await), vec![2]);
}

// ─── Non-member symbols ─────────────────────────────────────────────────────

/// A class reference answers the same as go-to-definition.
#[tokio::test]
async fn test_declaration_class_reference_matches_definition() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///decl_class_ref.php").unwrap();
    open_php(&backend, &uri, RENDERERS).await;

    let locations = declaration_at(&backend, &uri, 18, 15).await;
    assert_eq!(lines(&locations), vec![7]);
}

// ─── Cross-file ─────────────────────────────────────────────────────────────

/// The prototype is found in a file that was never opened, and a global
/// interface sharing the short name does not take its place.
#[tokio::test]
async fn test_declaration_cross_file_namespaced() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let interface_php = concat!(
        "<?php\n",                                       // 0
        "namespace App\\Contracts;\n",                   // 1
        "interface Logger {\n",                          // 2
        "    public function log(string $msg): void;\n", // 3
        "}\n",                                           // 4
    );
    let file_logger_php = concat!(
        "<?php\n",
        "namespace App\\Logging;\n",
        "use App\\Contracts\\Logger;\n",
        "class FileLogger implements Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
    );
    let service_php = concat!(
        "<?php\n",                                               // 0
        "namespace App\\Services;\n",                            // 1
        "use App\\Logging\\FileLogger;\n",                       // 2
        "class AppService {\n",                                  // 3
        "    public function run(FileLogger $logger): void {\n", // 4
        "        $logger->log('hello');\n",                      // 5
        "    }\n",                                               // 6
        "}\n",                                                   // 7
    );
    let global_php = concat!(
        "<?php\n",
        "interface Logger {\n",
        "    public function log(string $msg): void;\n",
        "}\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Contracts/Logger.php", interface_php),
            ("src/Logging/FileLogger.php", file_logger_php),
            ("src/Services/AppService.php", service_php),
        ],
    );

    let global_uri = Uri::parse("file:///global_logger.php").unwrap();
    open_php(&backend, &global_uri, global_php).await;

    let service_path = dir.path().join("src/Services/AppService.php");
    let service_uri = Uri::from_file_path(&service_path).unwrap();
    open_php(&backend, &service_uri, service_php).await;

    let locations = declaration_at(&backend, &service_uri, 5, 18).await;
    assert_eq!(locations.len(), 1, "got {locations:?}");
    assert!(
        locations[0]
            .uri
            .as_str()
            .ends_with("src/Contracts/Logger.php"),
        "should land in the namespaced interface, got {:?}",
        locations[0].uri
    );
    assert_eq!(locations[0].range.start.line, 3);
}

// ─── Capability ─────────────────────────────────────────────────────────────

/// The server advertises `declarationProvider`.
#[tokio::test]
async fn test_server_advertises_declaration_capability() {
    let backend = create_test_backend();
    let result = backend
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    assert!(result.capabilities.declaration_provider.is_some());
}
