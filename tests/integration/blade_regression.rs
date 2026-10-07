#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, hover_text_at, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    /// Open a Blade file and collect syntax-error diagnostics for it.
    fn blade_syntax_errors(uri: &str, blade_text: &str) -> Vec<Diagnostic> {
        let backend = phpantom_lsp::Backend::new_test();
        backend.update_ast(uri, blade_text);
        let mut out = Vec::new();
        backend.collect_syntax_error_diagnostics(uri, blade_text, &mut out);
        out
    }

    /// An `@` before a Blade echo leaves the expression for the frontend
    /// template engine, so JavaScript-only syntax must never reach PHP's
    /// parser as an echo expression.
    #[test]
    fn at_escaped_echo_has_no_php_syntax_diagnostic() {
        let blade_text = "@{{.Image}}\n";
        let diags = blade_syntax_errors("file:///escaped-echo.blade.php", blade_text);
        assert!(
            diags.is_empty(),
            "an escaped frontend interpolation is literal Blade text: {diags:?}"
        );
    }

    /// Escaped raw echoes are frontend text too, even when their contents
    /// use syntax that PHP cannot parse.
    #[test]
    fn at_escaped_raw_echo_has_no_php_syntax_diagnostic() {
        let blade_text = "@{!! .Image !!}\n";
        let diags = blade_syntax_errors("file:///escaped-raw-echo.blade.php", blade_text);
        assert!(
            diags.is_empty(),
            "an escaped raw interpolation is literal Blade text: {diags:?}"
        );
    }

    #[tokio::test]
    async fn test_blade_regression_sentry() {
        let backend = create_test_backend();
        let blade_uri = Url::parse("file:///sentry.blade.php").unwrap();
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_1.blade.php").unwrap();

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let _ = backend.goto_definition(params).await.unwrap();

        let (virtual_php, _) = phpantom_lsp::blade::preprocessor::preprocess(&blade_text);
        println!("VIRTUAL PHP SENTRY:\n{}", virtual_php);
    }

    #[tokio::test]
    async fn test_blade_regression_sitemap() {
        let backend = create_test_backend();
        let blade_uri = Url::parse("file:///sitemap.blade.php").unwrap();
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_2.blade.php").unwrap();

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let _ = backend.goto_definition(params).await.unwrap();

        // Print the preprocessed PHP
        let (virtual_php, _) = phpantom_lsp::blade::preprocessor::preprocess(&blade_text);
        println!("VIRTUAL PHP:\n{}", virtual_php);
    }

    /// A raw `<?php ... ?>` block embedded directly in a Blade template
    /// (not wrapped in `@php`/`@endphp`) must be passed through verbatim.
    /// A string literal that happens to start with `@` (e.g. a JSON-LD
    /// `'@context'` array key) must not be misread as a Blade directive.
    #[tokio::test]
    async fn test_blade_regression_raw_php_tag_with_at_prefixed_string() {
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_3.blade.php").unwrap();

        let diags = blade_syntax_errors("file:///schema.blade.php", &blade_text);
        assert!(
            diags.is_empty(),
            "Raw <?php ?> block should not produce syntax errors: {:?}",
            diags
        );
    }

    /// `@switch`/`@case`/`@break`/`@endswitch` must translate to a valid
    /// alternative-syntax `switch`, including when a `@case` argument is a
    /// fully-qualified class constant.
    #[tokio::test]
    async fn test_blade_regression_switch_case_with_class_constant() {
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_4.blade.php").unwrap();

        let diags = blade_syntax_errors("file:///membership.blade.php", &blade_text);
        assert!(
            diags.is_empty(),
            "@switch/@case with a class-constant argument should not produce syntax errors: {:?}",
            diags
        );
    }

    /// Inline attribute directives (`@class`, `@style`, `@checked`,
    /// `@selected`, `@disabled`, `@readonly`, `@required`) used as HTML
    /// attributes must consume their own argument list and return to HTML
    /// mode, not swallow the rest of the template as PHP.
    #[tokio::test]
    async fn test_blade_attribute_directives_do_not_corrupt_rest_of_template() {
        let blade_text = r#"<div @class(['collapse', 'in' => $errors->has('cover_image')])
    id="collapse-cover-image">
    <input type="checkbox" @checked($page->show_in_app) />
    <input @disabled(!$canChange) @readonly($locked) @required($mandatory) />
    <select><option @selected($lang === $current)>{{ $lang }}</option></select>
    <span @style(['color: red' => $hasError])>Text</span>
</div>
"#;

        let diags = blade_syntax_errors("file:///attributes.blade.php", blade_text);
        assert!(
            diags.is_empty(),
            "attribute directives should not produce syntax errors: {:?}",
            diags
        );
    }

    /// `@use` and `@inject` previously left the parser in PHP mode for the
    /// rest of the template, corrupting everything after them. They must now
    /// consume their argument lists and translate to real PHP without
    /// producing cascading syntax errors.
    #[tokio::test]
    async fn test_blade_use_and_inject_do_not_corrupt_rest_of_template() {
        let blade_text = r#"@use('App\Models\Post')
@use('App\Models\Comment as Reply')
@inject('metrics', 'App\Services\Metrics')
<div class="post">
    <h1>{{ $post->title }}</h1>
    <p>{{ $metrics->views() }}</p>
</div>
"#;

        let diags = blade_syntax_errors("file:///use-inject.blade.php", blade_text);
        assert!(
            diags.is_empty(),
            "@use/@inject should not produce syntax errors: {:?}",
            diags
        );

        let (virtual_php, _) = phpantom_lsp::blade::preprocessor::preprocess(blade_text);
        assert!(
            virtual_php.contains("use App\\Models\\Post;"),
            "@use should emit a real import: {}",
            virtual_php
        );
        assert!(
            virtual_php.contains("use App\\Models\\Comment as Reply;"),
            "aliased @use should emit an aliased import: {}",
            virtual_php
        );
        assert!(
            virtual_php.contains("$metrics = app('App\\Services\\Metrics');"),
            "@inject should emit an app() assignment: {}",
            virtual_php
        );
    }

    /// A directive's keyword is replaced by generated PHP, and the
    /// expression it takes starts right behind that, so hovering the keyword
    /// must not describe the expression.
    #[tokio::test]
    async fn hovering_a_directive_keyword_does_not_describe_the_expression_behind_it() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///directive-hover.blade.php").unwrap();
        let template = "@php($cond = true)\n\
                        @if($cond)\n\
                        @foreach([$cond] as $row)\n\
                        @include('partials.card', ['on' => $cond])\n\
                        @endforeach\n\
                        @endif\n";
        open_document(&backend, &uri, "blade", template).await;

        for (line, keyword) in [(1, "@if"), (2, "@foreach"), (3, "@include")] {
            for column in 0..keyword.len() {
                let hover = hover_text_at(&backend, &uri, line, column as u32).await;
                assert_eq!(
                    hover, None,
                    "column {column} of {keyword} on line {line} is the keyword, which has nothing to hover"
                );
            }
        }
    }

    /// A raw echo's delimiters lower to a bare `echo` and a `;`, and with no
    /// space between the opener and the expression the generated `echo` ends
    /// right where the expression starts. Hovering a delimiter must describe
    /// the echo, never the expression behind it, with or without the space.
    #[tokio::test]
    async fn hovering_a_raw_echo_delimiter_describes_the_echo_not_the_expression() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///raw-echo-hover.blade.php").unwrap();
        let template = "@php($html = '<b>bold</b>')\n\
                        {!!$html!!}\n\
                        {!! $html !!}\n\
                        <p>{!!$html!!}</p>\n\
                        <p>æøå {!!$html!!}</p>\n";
        open_document(&backend, &uri, "blade", template).await;

        // `(line, column of the opener, column of the closer)`, in UTF-16
        // units like the positions an editor sends.
        for (line, opener, closer) in [(1, 0, 8), (2, 0, 10), (3, 3, 11), (4, 7, 15)] {
            for start in [opener, closer] {
                let expected = Range {
                    start: Position {
                        line,
                        character: start,
                    },
                    end: Position {
                        line,
                        character: start + 3,
                    },
                };
                for column in start..start + 3 {
                    let hover = backend
                        .hover(HoverParams {
                            text_document_position_params: TextDocumentPositionParams {
                                text_document: TextDocumentIdentifier { uri: uri.clone() },
                                position: Position {
                                    line,
                                    character: column,
                                },
                            },
                            work_done_progress_params: WorkDoneProgressParams::default(),
                        })
                        .await
                        .unwrap()
                        .unwrap_or_else(|| panic!("expected a hover at {line}:{column}"));
                    let HoverContents::Markup(markup) = hover.contents else {
                        panic!("expected markup at {line}:{column}");
                    };
                    assert!(
                        markup.value.contains("raw echo") && !markup.value.contains("$html"),
                        "{line}:{column} is a raw echo delimiter, which describes the echo: {}",
                        markup.value
                    );
                    assert_eq!(
                        hover.range,
                        Some(expected),
                        "{line}:{column} must highlight the whole delimiter"
                    );
                }
            }
        }
    }

    /// `{{!!$html!!}}` is a literal `{`, a raw echo, and a literal `}`, since
    /// Blade matches its echo tags longest-opening-first. The raw echo's
    /// delimiters describe a raw echo, and the braces around it are text, not
    /// the `e()` an escaped echo would compile to.
    #[tokio::test]
    async fn a_raw_echo_inside_literal_braces_hovers_as_a_raw_echo() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///raw-echo-in-braces-hover.blade.php").unwrap();
        let template = "@php($html = '<b>bold</b>')\n\
                        {{!!$html!!}}\n\
                        {{!! $html !!}}\n\
                        <p>{{!!$html!!}}</p>\n\
                        @{{!!$html!!}}\n";
        open_document(&backend, &uri, "blade", template).await;

        // `(line, column of the opener, column of the closer)`, in UTF-16
        // units like the positions an editor sends.
        for (line, opener, closer) in [(1, 1, 9), (2, 1, 11), (3, 4, 12), (4, 2, 10)] {
            for start in [opener, closer] {
                let expected = Range {
                    start: Position {
                        line,
                        character: start,
                    },
                    end: Position {
                        line,
                        character: start + 3,
                    },
                };
                for column in start..start + 3 {
                    let hover = backend
                        .hover(HoverParams {
                            text_document_position_params: TextDocumentPositionParams {
                                text_document: TextDocumentIdentifier { uri: uri.clone() },
                                position: Position {
                                    line,
                                    character: column,
                                },
                            },
                            work_done_progress_params: WorkDoneProgressParams::default(),
                        })
                        .await
                        .unwrap()
                        .unwrap_or_else(|| panic!("expected a hover at {line}:{column}"));
                    let HoverContents::Markup(markup) = hover.contents else {
                        panic!("expected markup at {line}:{column}");
                    };
                    assert!(
                        markup.value.contains("raw echo") && !markup.value.contains("$html"),
                        "{line}:{column} is a raw echo delimiter, which describes the echo: {}",
                        markup.value
                    );
                    assert_eq!(
                        hover.range,
                        Some(expected),
                        "{line}:{column} must highlight the whole delimiter"
                    );
                }
            }

            // The expression keeps its own hover.
            let hover = hover_text_at(&backend, &uri, line, opener + 4).await;
            assert!(
                hover.is_some_and(|text| text.contains("$html")),
                "{line}:{} is on `$html`, which describes the variable",
                opener + 4
            );
        }

        // The braces around the echo are text.
        for (line, column) in [
            (1, 0),
            (1, 12),
            (2, 0),
            (2, 14),
            (3, 3),
            (3, 15),
            (4, 1),
            (4, 13),
        ] {
            assert_eq!(
                hover_text_at(&backend, &uri, line, column).await,
                None,
                "{line}:{column} is a literal brace, which has nothing to describe"
            );
        }
    }

    /// The expression between the delimiters keeps its own hover: only the
    /// delimiters themselves describe the echo.
    #[tokio::test]
    async fn hovering_the_expression_of_a_raw_echo_still_describes_it() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///raw-echo-expression-hover.blade.php").unwrap();
        let template = "@php($html = '<b>bold</b>')\n{!!$html!!}\n{!! $html !!}\n";
        open_document(&backend, &uri, "blade", template).await;

        for (line, columns) in [(1, 3..8), (2, 4..9)] {
            for column in columns {
                let hover = hover_text_at(&backend, &uri, line, column).await;
                assert!(
                    hover.is_some_and(|text| text.contains("$html")),
                    "{line}:{column} is on `$html`, which describes the variable"
                );
            }
        }
    }
}
