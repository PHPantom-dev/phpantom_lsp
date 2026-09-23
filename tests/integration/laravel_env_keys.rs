//! Environment variables: `env('KEY')` against the project's dotenv files.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.
//!
//! A variable is declared the way `vlucas/phpdotenv` (which Laravel loads
//! `.env` through) reads a line: `KEY=value`, optionally indented or
//! prefixed with `export`, where a `#` line is a comment and a bare name
//! with no `=` sets nothing.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_labels_at_opened, create_initialized_psr4_workspace,
    definition_locations, goto_definition_at, markup_hover_at, position_after, position_of,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// A class in `app/` whose one method runs `body`.
fn caller(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass Demo {{\n    public function go(): void {{\n        {body}\n    }}\n}}\n"
    )
}

/// A workspace holding `files` plus `app/Demo.php` running `body`, scanned
/// and with the demo open.
async fn workspace(
    files: &[(&str, &str)],
    body: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url, String) {
    let content = caller(body);
    let mut all: Vec<(&str, &str)> = files.to_vec();
    all.push(("app/Demo.php", content.as_str()));
    let (backend, dir, uri) =
        create_initialized_psr4_workspace(LARAVEL_APP_COMPOSER, &all, "app/Demo.php").await;
    drop(all);
    (backend, dir, uri, content)
}

/// The go-to-definition targets a couple of characters into the first
/// occurrence of `needle` in `content`.
async fn definitions_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> Vec<Location> {
    let position = position_of(content, needle);
    definition_locations(
        goto_definition_at(backend, uri, position.line, position.character + 2).await,
    )
}

/// The hover a couple of characters into the first occurrence of `needle`.
async fn hover_on(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> String {
    let position = position_of(content, needle);
    markup_hover_at(backend, uri, position.line, position.character + 2).await
}

/// The location among `found` in the workspace-root file `name`.
fn location_in<'a>(found: &'a [Location], name: &str) -> Option<&'a Location> {
    let suffix = format!("/{name}");
    found
        .iter()
        .find(|location| location.uri.as_str().ends_with(&suffix))
}

/// The completion labels offered inside `env('')` in `content`.
async fn env_completion_labels(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
) -> Vec<String> {
    let position = position_after(content, "env('");
    complete_labels_at_opened(backend, uri, position.line, position.character).await
}

// ─── Which lines declare a variable ─────────────────────────────────────────

/// A commented-out assignment documents a variable; the live line below it
/// is the declaration.
#[tokio::test]
async fn a_commented_out_assignment_is_not_the_declaration() {
    let (backend, _dir, uri, content) = workspace(
        &[(
            ".env",
            "# APP_NAME=this is a comment, not a declaration\nAPP_NAME=Laravel\n",
        )],
        "env('APP_NAME');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "APP_NAME").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].range.start.line, 1);
}

/// A bare name with no `=` sets nothing, so the assignment further down is
/// the declaration.
#[tokio::test]
async fn a_bare_name_without_an_equals_sign_is_not_the_declaration() {
    let (backend, _dir, uri, content) = workspace(
        &[(".env", "APP_NAME\nOTHER=foo\nAPP_NAME=Laravel\n")],
        "env('APP_NAME');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "APP_NAME").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].range.start.line, 2);
}

/// Leading whitespace is trimmed off the name, so an indented line still
/// declares the variable and gives it its value.
#[tokio::test]
async fn an_indented_assignment_is_a_declaration() {
    let (backend, _dir, uri, content) = workspace(
        &[(".env", "APP_ENV=local\n  APP_NAME=Laravel\n")],
        "env('APP_NAME');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "APP_NAME").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].range.start.line, 1);

    let text = hover_on(&backend, &uri, &content, "APP_NAME").await;
    assert!(text.contains("`Laravel`"), "got {text}");
}

/// An `export ` prefix is part of the shell syntax, not of the name.
#[tokio::test]
async fn an_exported_assignment_is_a_declaration() {
    let (backend, _dir, uri, content) = workspace(
        &[(".env", "APP_ENV=local\nexport MAIL_MAILER=log\n")],
        "env('MAIL_MAILER');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "MAIL_MAILER").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].range.start.line, 1);

    let text = hover_on(&backend, &uri, &content, "MAIL_MAILER").await;
    assert!(text.contains("`log`"), "got {text}");
}

/// A line is split on its first `=`, so the value keeps any that follow.
#[tokio::test]
async fn a_value_may_contain_an_equals_sign() {
    let (backend, _dir, uri, content) = workspace(
        &[(".env", "DATABASE_URL=postgres://user:pass=word@host/db\n")],
        "env('DATABASE_URL');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "DATABASE_URL").await;
    assert!(
        text.contains("`postgres://user:pass=word@host/db`"),
        "got {text}"
    );
}

/// phpdotenv's immutable repository only refuses to overwrite a variable
/// the process environment already had; one the file itself set is
/// overwritten by a later line, so a redeclared variable ends up with the
/// last value.
#[tokio::test]
async fn a_redeclared_variable_takes_its_last_value() {
    let (backend, _dir, uri, content) = workspace(
        &[(".env", "MAIL_MAILER=smtp\nMAIL_MAILER=log\n")],
        "env('MAIL_MAILER');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "MAIL_MAILER").await;
    assert!(
        text.contains("`log`") && !text.contains("`smtp`"),
        "the later line wins, got {text}"
    );
}

/// A name nothing declares hovers as such.
#[tokio::test]
async fn an_undeclared_variable_hovers_as_not_declared() {
    let (backend, _dir, uri, content) =
        workspace(&[(".env", "APP_NAME=Acme\n")], "env('MAIL_MAILER');").await;

    let text = hover_on(&backend, &uri, &content, "MAIL_MAILER").await;
    assert!(text.contains("Not declared in `.env`"), "got {text}");
}

// ─── Which files declare a variable ─────────────────────────────────────────

/// A variable both `.env` and `.env.example` declare is defined in both.
#[tokio::test]
async fn a_variable_declared_in_both_dotenv_files_resolves_to_both() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\nMAIL_MAILER=log\n"),
            (".env.example", "MAIL_MAILER=smtp\n"),
        ],
        "env('MAIL_MAILER');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "MAIL_MAILER").await;
    assert_eq!(found.len(), 2, "got {found:?}");
    assert_eq!(
        location_in(&found, ".env").map(|l| l.range.start.line),
        Some(1),
        "got {found:?}"
    );
    assert_eq!(
        location_in(&found, ".env.example").map(|l| l.range.start.line),
        Some(0),
        "got {found:?}"
    );
}

/// When only the example file declares a variable, that is where it
/// resolves.
#[tokio::test]
async fn a_variable_only_the_example_declares_resolves_there() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".env.example", "APP_NAME=\nMAIL_HOST=\n"),
        ],
        "env('MAIL_HOST');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "MAIL_HOST").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(
        found[0].uri.as_str().ends_with("/.env.example"),
        "got {found:?}"
    );
    assert_eq!(found[0].range.start.line, 1);
}

/// `.env` is the file Laravel loads, so its value is the one hover shows.
#[tokio::test]
async fn hover_prefers_the_dotenv_value_over_the_example() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".env.example", "APP_NAME=Example\n"),
        ],
        "env('APP_NAME');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "APP_NAME").await;
    assert!(
        text.contains("`Acme`") && text.contains("Declared in `.env`"),
        "got {text}"
    );
}

/// With no `.env` declaration, the example file's value is what there is.
#[tokio::test]
async fn hover_falls_back_to_the_example_file() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".env.example", "MAIL_MAILER=log\n"),
        ],
        "env('MAIL_MAILER');",
    )
    .await;

    let text = hover_on(&backend, &uri, &content, "MAIL_MAILER").await;
    assert!(
        text.contains("`log`") && text.contains("Declared in `.env.example`"),
        "got {text}"
    );
}

/// direnv's `.envrc`, and files that merely have `env` in their name, are
/// not dotenv files.
#[tokio::test]
async fn files_that_only_resemble_a_dotenv_file_are_not_read() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".envrc", "export MAIL_HOST=smtp.test\n"),
            ("env.txt", "MAIL_HOST=smtp.test\n"),
            ("config.env", "MAIL_HOST=smtp.test\n"),
        ],
        "env('MAIL_HOST');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "MAIL_HOST").await;
    for name in [".envrc", "env.txt", "config.env"] {
        assert!(
            location_in(&found, name).is_none(),
            "{name} is not a dotenv file, got {found:?}"
        );
    }

    let text = hover_on(&backend, &uri, &content, "MAIL_HOST").await;
    assert!(text.contains("Not declared in `.env`"), "got {text}");
}

/// Laravel loads `.env.<APP_ENV>` in place of `.env` when it exists, which
/// is how `.env.testing` supplies the test suite's database, so a variable
/// declared there is declared.
#[tokio::test]
#[ignore = "known gap: `.env.<environment>` files are not read"]
async fn an_environment_specific_dotenv_file_declares_the_variable_too() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".env.testing", "APP_NAME=Acme\nDB_DATABASE=testing\n"),
        ],
        "env('DB_DATABASE');",
    )
    .await;

    let found = definitions_at(&backend, &uri, &content, "DB_DATABASE").await;
    let location = location_in(&found, ".env.testing")
        .unwrap_or_else(|| panic!("expected a location in .env.testing, got {found:?}"));
    assert_eq!(location.range.start.line, 1);
}

/// Find-references with declarations included lists the line in every
/// dotenv file that declares the variable.
#[tokio::test]
async fn find_references_lists_every_dotenv_declaration() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\nMAIL_MAILER=log\n"),
            (".env.example", "MAIL_MAILER=smtp\n"),
        ],
        "env('MAIL_MAILER');",
    )
    .await;

    let position = position_of(&content, "MAIL_MAILER");
    let found = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(position.line, position.character + 2),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("an env key should have references");

    for name in ["app/Demo.php", ".env", ".env.example"] {
        assert!(
            location_in(&found, name).is_some(),
            "expected a reference in {name}, got {found:?}"
        );
    }
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// The example file's variables are offered too, each name once.
#[tokio::test]
async fn completion_offers_the_example_files_variables_once() {
    let (backend, _dir, uri, content) = workspace(
        &[
            (".env", "APP_NAME=Acme\n"),
            (".env.example", "APP_NAME=\nMAIL_HOST=\n"),
        ],
        "env('');",
    )
    .await;

    let labels = env_completion_labels(&backend, &uri, &content).await;
    assert!(
        labels.iter().any(|label| label == "MAIL_HOST"),
        "got {labels:?}"
    );
    assert_eq!(
        labels.iter().filter(|label| *label == "APP_NAME").count(),
        1,
        "a name both files declare is offered once, got {labels:?}"
    );
}

/// Only lines that declare something are offered: indented, exported and
/// empty-valued assignments are, comments and bare names are not.
#[tokio::test]
async fn completion_offers_only_declared_names() {
    let (backend, _dir, uri, content) = workspace(
        &[(
            ".env",
            "# COMMENTED_OUT=x\nJUST_TEXT\n  SPACED=value\nexport EXPORTED=1\nEMPTY=\n  # INDENTED_COMMENT=y\n",
        )],
        "env('');",
    )
    .await;

    let labels = env_completion_labels(&backend, &uri, &content).await;
    for expected in ["SPACED", "EXPORTED", "EMPTY"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "expected `{expected}`, got {labels:?}"
        );
    }
    for unexpected in ["COMMENTED_OUT", "JUST_TEXT", "INDENTED_COMMENT"] {
        assert!(
            !labels.iter().any(|label| label == unexpected),
            "`{unexpected}` is not declared, got {labels:?}"
        );
    }
}
