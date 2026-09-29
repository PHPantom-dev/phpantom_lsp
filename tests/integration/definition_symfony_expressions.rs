use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const CONFIG: &str = r#"
[indexing]
strategy = "none"

[[symfony.expression-language.attributes]]
attribute = 'Acme\Expression\Cache'
argument = "tags"
position = 3
method-parameters = true

[[symfony.expression-language.attributes]]
attribute = 'Acme\Expression\Security'
argument = "expression"
position = 0
method-parameters = true

[[symfony.expression-language.constructors]]
class = 'Acme\Expression\Value'
position = 0
inside-attribute-prefixes = ['Acme\Track\']
bindings = { request = "parameter:0", response = "return", subject = 'class:App\Model\Course' }
"#;

async fn open_php(backend: &Backend, uri: Url, content: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;
}

fn uri_for(dir: &tempfile::TempDir, relative: &str) -> Url {
    Url::from_file_path(dir.path().join(relative)).unwrap()
}

fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    let offset = content.find(needle).expect("needle should exist") + inside;
    let prefix = &content[..offset];
    Position::new(
        prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, line)| line.len()) as u32,
    )
}

async fn definition_at(backend: &Backend, uri: Url, position: Position) -> Location {
    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("expression symbol should resolve");
    match response {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) if locations.len() == 1 => {
            locations.pop().unwrap()
        }
        other => panic!("expected one definition, got {other:?}"),
    }
}

#[tokio::test]
async fn configured_attribute_arguments_navigate_and_report_the_first_missing_member() {
    let request_php = r#"<?php
namespace App\Request;

use App\Model\Course;

final readonly class CourseRequest
{
    public function __construct(public Course $course) {}
}
"#;
    let course_php = r#"<?php
namespace App\Model;

final class Course
{
    public int $id = 1;
    public function owner(): self { return $this; }
}
"#;
    let controller_php = r#"<?php
namespace App\Controller;

use Acme\Expression\{Cache, Security};
use App\Request\CourseRequest;

final class CourseController
{
    #[Cache(tags: [
        'request.course.id',
        'request.missing.afterMissing',
    ])]
    #[Security('request.course.owner().id > 0 and request.missingMethod()')]
    public function show(CourseRequest $request): void {}

    #[Cache(null, [], null, ['request.positionalMissing'])]
    public function other(CourseRequest $request): void {}
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (".phpantom.toml", CONFIG),
            ("src/Request/CourseRequest.php", request_php),
            ("src/Model/Course.php", course_php),
            ("src/Controller/CourseController.php", controller_php),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let request_uri = uri_for(&dir, "src/Request/CourseRequest.php");
    let course_uri = uri_for(&dir, "src/Model/Course.php");
    let controller_uri = uri_for(&dir, "src/Controller/CourseController.php");
    open_php(&backend, request_uri.clone(), request_php).await;
    open_php(&backend, course_uri.clone(), course_php).await;
    open_php(&backend, controller_uri.clone(), controller_php).await;

    let root = definition_at(
        &backend,
        controller_uri.clone(),
        position_in(controller_php, "request.course.id", 2),
    )
    .await;
    assert_eq!(root.uri, controller_uri);
    assert_eq!(root.range.start.line, 13);

    let course = definition_at(
        &backend,
        controller_uri.clone(),
        position_in(controller_php, "request.course.id", "request.".len() + 2),
    )
    .await;
    assert_eq!(course.uri, request_uri);

    let owner = definition_at(
        &backend,
        controller_uri.clone(),
        position_in(
            controller_php,
            "request.course.owner()",
            "request.course.".len() + 2,
        ),
    )
    .await;
    assert_eq!(owner.uri, course_uri);
    assert_eq!(owner.range.start.line, 6);

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(controller_uri.as_str(), controller_php, &mut diagnostics);
    diagnostics.retain(|diagnostic| {
        diagnostic.code.as_ref().is_some_and(
            |code| matches!(code, NumberOrString::String(value) if value == "unknown_member"),
        )
    });
    assert_eq!(
        diagnostics.len(),
        3,
        "unexpected diagnostics: {diagnostics:#?}"
    );
    for missing in ["missing", "missingMethod", "positionalMissing"] {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(missing)),
            "missing diagnostic for {missing}: {diagnostics:#?}"
        );
    }
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("afterMissing"))
    );
}

#[tokio::test]
async fn configured_constructor_bindings_are_scoped_to_matching_attributes() {
    let request_php = r#"<?php
namespace App\Request;

final readonly class CourseRequest
{
    public function __construct(public int $courseId) {}
}
"#;
    let course_php = r#"<?php
namespace App\Model;

final class Course
{
    public int $id = 1;
}
"#;
    let response_php = r#"<?php
namespace App\Response;

final readonly class CourseResponse
{
    public function __construct(public int $candidateId) {}
}
"#;
    let use_case_php = r#"<?php
namespace App\UseCase;

use Acme\Expression\Value;
use Acme\Security\IsGranted;
use Acme\Track\Analytics;
use App\Request\CourseRequest;
use App\Response\CourseResponse;

final class PublishCourse
{
    #[Analytics(name: new Value('request.courseId ~ response.candidateId ~ subject.id ~ response.missing'))]
    #[IsGranted(new Value('request.notPartOfThisContract'))]
    public function execute(CourseRequest $input): CourseResponse
    {
        return new CourseResponse(1);
    }
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (".phpantom.toml", CONFIG),
            ("src/Request/CourseRequest.php", request_php),
            ("src/Model/Course.php", course_php),
            ("src/Response/CourseResponse.php", response_php),
            ("src/UseCase/PublishCourse.php", use_case_php),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let request_uri = uri_for(&dir, "src/Request/CourseRequest.php");
    let course_uri = uri_for(&dir, "src/Model/Course.php");
    let response_uri = uri_for(&dir, "src/Response/CourseResponse.php");
    let use_case_uri = uri_for(&dir, "src/UseCase/PublishCourse.php");
    open_php(&backend, request_uri.clone(), request_php).await;
    open_php(&backend, course_uri.clone(), course_php).await;
    open_php(&backend, response_uri.clone(), response_php).await;
    open_php(&backend, use_case_uri.clone(), use_case_php).await;

    let request = definition_at(
        &backend,
        use_case_uri.clone(),
        position_in(use_case_php, "request.courseId", "request.".len() + 2),
    )
    .await;
    assert_eq!(request.uri, request_uri);

    let response = definition_at(
        &backend,
        use_case_uri.clone(),
        position_in(use_case_php, "response.candidateId", "response.".len() + 2),
    )
    .await;
    assert_eq!(response.uri, response_uri);

    let subject = definition_at(
        &backend,
        use_case_uri.clone(),
        position_in(use_case_php, "subject.id", "subject.".len() + 1),
    )
    .await;
    assert_eq!(subject.uri, course_uri);

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(use_case_uri.as_str(), use_case_php, &mut diagnostics);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("missing"))
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("notPartOfThisContract")),
        "constructor rule leaked outside its attribute prefix: {diagnostics:#?}"
    );
}

#[tokio::test]
async fn expression_chains_follow_backed_enum_value() {
    let interaction_php = r#"<?php
namespace App\Model;

enum Interaction: string
{
    case Viewed = 'viewed';
}
"#;
    let request_php = r#"<?php
namespace App\Request;

use App\Model\Interaction;

final readonly class TrackRequest
{
    public function __construct(public Interaction $interaction) {}
}
"#;
    let controller_php = r#"<?php
namespace App\Controller;

use Acme\Expression\Security;
use App\Request\TrackRequest;

final class TrackController
{
    #[Security('request.interaction.value')]
    public function track(TrackRequest $request): void {}
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (".phpantom.toml", CONFIG),
            ("src/Model/Interaction.php", interaction_php),
            ("src/Request/TrackRequest.php", request_php),
            ("src/Controller/TrackController.php", controller_php),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let interaction_uri = uri_for(&dir, "src/Model/Interaction.php");
    let request_uri = uri_for(&dir, "src/Request/TrackRequest.php");
    let controller_uri = uri_for(&dir, "src/Controller/TrackController.php");
    open_php(&backend, interaction_uri.clone(), interaction_php).await;
    open_php(&backend, request_uri, request_php).await;
    open_php(&backend, controller_uri.clone(), controller_php).await;

    let value = definition_at(
        &backend,
        controller_uri.clone(),
        position_in(
            controller_php,
            "request.interaction.value",
            "request.interaction.".len() + 2,
        ),
    )
    .await;
    assert_eq!(value.uri, interaction_uri);
    assert_eq!(value.range.start.line, 3);

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(controller_uri.as_str(), controller_php, &mut diagnostics);
    assert!(diagnostics.iter().all(|diagnostic| {
        !diagnostic.message.contains("value")
            || !diagnostic.code.as_ref().is_some_and(
                |code| matches!(code, NumberOrString::String(value) if value == "unknown_member"),
            )
    }));
}

#[tokio::test]
async fn unconfigured_attributes_do_not_gain_expression_semantics() {
    let request_php = "<?php\nnamespace App\\Request;\nfinal class CourseRequest {}\n";
    let controller_php = r#"<?php
namespace App\Controller;

use App\Request\CourseRequest;
use Legacy\Vendor\Attribute\Cache;

final class CourseController
{
    #[Cache(tags: ['request.missingField'])]
    public function show(CourseRequest $request): void {}
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (".phpantom.toml", CONFIG),
            ("src/Request/CourseRequest.php", request_php),
            ("src/Controller/CourseController.php", controller_php),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let request_uri = uri_for(&dir, "src/Request/CourseRequest.php");
    let controller_uri = uri_for(&dir, "src/Controller/CourseController.php");
    open_php(&backend, request_uri, request_php).await;
    open_php(&backend, controller_uri.clone(), controller_php).await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(controller_uri.as_str(), controller_php, &mut diagnostics);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("missingField")),
        "unconfigured package name should not be special: {diagnostics:#?}"
    );
}
