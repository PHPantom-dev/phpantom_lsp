//! How a Blade template's directives, loops, `@props` lists and embedded PHP
//! regions lower to the PHP the type engine reads, observed through hover,
//! completion, go-to-definition and the undefined-variable check. Several
//! cases are adapted from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    BLADE_COMPONENT_COMPOSER, ILLUMINATE_COMPONENT_STUB, LIVEWIRE_COMPONENT_STUB,
    blade_undefined_variables, complete_at_opened_with_trigger,
    complete_labels_at_opened_with_trigger, create_psr4_workspace, create_test_backend,
    definition_locations, goto_definition_at, hover_text_at, labels, open_document, open_php,
    position_after, position_of, workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const CLASSES_URI: &str = "file:///Classes.php";

/// The classes every template in this file renders.
const CLASSES: &str = r#"<?php
class User {
    public string $name = '';
}
class Item {
    public string $label = '';
}
class Category {
    public string $title = '';
    /** @var list<Item> */
    public array $items = [];
}
class Roster {
    /** @return list<User> */
    public function active(string $flag, bool $on): array { return []; }
}
class Query {
    public function where(string $column, bool $value): static { return $this; }
    public function orderBy(string $column): static { return $this; }
    /** @return list<User> */
    public function get(): array { return []; }
}
class Logger {
    public function info(): void {}
    public function warn(): void {}
}
"#;

/// The signature every template in this file opens with.
const PRELUDE: &str = "@php
/**
 * @var list<User> $users
 * @var array<string, User> $byName
 * @var list<array{int, User}> $pairs
 * @var array<string, array{int, User}> $rows
 * @var list<Category> $categories
 * @var Roster $roster
 * @var Query $query
 * @var Logger $logger
 */
@endphp
";

/// Open [`CLASSES`] and a template of [`PRELUDE`] followed by `body`,
/// handing back the template's URI and full text.
async fn template(body: &str) -> (Backend, Url, String) {
    let backend = create_test_backend();
    open_php(&backend, &Url::parse(CLASSES_URI).unwrap(), CLASSES).await;
    let text = format!("{PRELUDE}{body}");
    let uri = Url::parse("file:///view.blade.php").unwrap();
    open_document(&backend, &uri, "blade", &text).await;
    (backend, uri, text)
}

/// The hover `offset` UTF-16 columns into the first `needle` of `text`,
/// empty when nothing hovers.
async fn hover_in(backend: &Backend, uri: &Url, text: &str, needle: &str, offset: u32) -> String {
    let at = position_of(text, needle);
    hover_text_at(backend, uri, at.line, at.character + offset)
        .await
        .unwrap_or_default()
}

/// The line of [`CLASSES`] a definition request `offset` columns into the
/// first `needle` of `text` lands on, if it lands in [`CLASSES`] at all.
async fn definition_line_in(
    backend: &Backend,
    uri: &Url,
    text: &str,
    needle: &str,
    offset: u32,
) -> Option<u32> {
    let at = position_of(text, needle);
    let response = goto_definition_at(backend, uri, at.line, at.character + offset).await;
    definition_locations(response)
        .into_iter()
        .find(|location| location.uri.as_str() == CLASSES_URI)
        .map(|location| location.range.start.line)
}

/// The line of [`CLASSES`] declaring `needle`.
fn classes_line(needle: &str) -> u32 {
    position_of(CLASSES, needle).line
}

// ─── Loop bindings ──────────────────────────────────────────────────────────

#[tokio::test]
async fn a_foreach_value_is_typed_from_the_iterable() {
    let body = "@foreach($users as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_foreach_key_and_value_are_both_typed() {
    let body =
        "@foreach($byName as $key => $value)\n    {{ $key }}: {{ $value->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let key = hover_in(&backend, &uri, &text, "{{ $key }}", 4).await;
    assert!(
        key.contains("string"),
        "the key should be a string, got: {key}"
    );
    let value = hover_in(&backend, &uri, &text, "{{ $value->name }}", 4).await;
    assert!(
        value.contains("User"),
        "the value should be a User, got: {value}"
    );
}

#[tokio::test]
async fn a_foreach_header_with_padding_inside_its_parens_still_binds() {
    let body = "@foreach( $users as $user )\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn short_list_destructuring_binds_every_name() {
    let body = "@foreach($pairs as [$position, $member])\n    {{ $position }} {{ $member->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let position = hover_in(&backend, &uri, &text, "{{ $position }}", 4).await;
    assert!(position.contains("int"), "got: {position}");
    let member = hover_in(&backend, &uri, &text, "{{ $member->name }}", 4).await;
    assert!(member.contains("User"), "got: {member}");
}

#[tokio::test]
async fn long_list_destructuring_binds_every_name() {
    let body =
        "@foreach($pairs as list($position, $member))\n    {{ $member->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let member = hover_in(&backend, &uri, &text, "{{ $member->name }}", 4).await;
    assert!(member.contains("User"), "got: {member}");
    assert!(
        blade_undefined_variables(&backend, &uri).is_empty(),
        "list() binds both names"
    );
}

#[tokio::test]
async fn a_by_reference_binding_is_typed_like_a_plain_one() {
    let body = "@foreach($users as &$user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_key_beside_a_destructured_value_binds_all_three_names() {
    let body = "@foreach($rows as $slug => [$rank, $member])\n    {{ $slug }} {{ $rank }} {{ $member->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let slug = hover_in(&backend, &uri, &text, "{{ $slug }}", 4).await;
    assert!(slug.contains("string"), "got: {slug}");
    let member = hover_in(&backend, &uri, &text, "{{ $member->name }}", 4).await;
    assert!(member.contains("User"), "got: {member}");
    assert!(
        blade_undefined_variables(&backend, &uri).is_empty(),
        "every name the header binds is defined"
    );
}

#[tokio::test]
async fn a_forelse_value_is_typed_from_the_iterable() {
    let body = "@forelse($users as $user)\n    {{ $user->name }}\n@empty\n    none\n@endforelse\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_for_loop_counter_is_an_int() {
    let body = "@for($i = 0; $i < 10; $i++)\n    {{ $i }}\n@endfor\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $i }}", 4).await;
    assert!(hover.contains("int"), "got: {hover}");
}

#[tokio::test]
async fn a_for_loop_header_without_spaces_still_binds_its_counter() {
    let body = "@for($j=0;$j<10;$j++)\n    {{ $j }}\n@endfor\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $j }}", 4).await;
    assert!(hover.contains("int"), "got: {hover}");
    assert!(blade_undefined_variables(&backend, &uri).is_empty());
}

// ─── Loop headers that are hard to delimit ──────────────────────────────────

#[tokio::test]
async fn a_method_call_iterable_with_a_paren_in_a_string_keeps_the_binding() {
    let body =
        "@foreach($roster->active('a)b', true) as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn an_iterable_with_nested_calls_keeps_the_binding() {
    let body = "@foreach($roster->active(strtoupper('x'), in_array(1, [1, 2])) as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_header_wrapped_before_as_keeps_the_binding() {
    let body =
        "@foreach($roster->active('x', true)\n    as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_header_with_keyword_binding_and_paren_on_their_own_lines_keeps_the_binding() {
    let body = "@foreach(\n    $byName as $key => $value\n)\n    {{ $key }}: {{ $value->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let value = hover_in(&backend, &uri, &text, "{{ $value->name }}", 4).await;
    assert!(value.contains("User"), "got: {value}");
    assert!(blade_undefined_variables(&backend, &uri).is_empty());
}

#[tokio::test]
async fn a_wrapped_forelse_header_keeps_the_binding() {
    let body = "@forelse($roster->active('x', true)\n    as $person)\n    {{ $person->name }}\n@empty\n    none\n@endforelse\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $person->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_header_wrapped_across_many_continuation_lines_keeps_the_binding() {
    let body = "@foreach ($query\n    ->where('active', true)\n    ->where('verified', true)\n    ->orderBy('name')\n    ->get()\n    as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

/// Blade finds the closing paren with PHP's own tokenizer, so a `)` inside a
/// comment in the header does not end the argument list.
#[tokio::test]
#[ignore = "known gap: a `)` inside a PHP comment closes a Blade directive's arguments"]
async fn a_paren_inside_a_block_comment_does_not_close_the_header() {
    let body = "@foreach ($users /* :) */ as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
    assert!(
        blade_undefined_variables(&backend, &uri).is_empty(),
        "the binding after the comment must still be seen"
    );
}

#[tokio::test]
async fn a_line_comment_on_the_first_line_of_a_wrapped_header_ends_at_the_line_break() {
    let body =
        "@foreach ($users // active only\n    as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
async fn a_hash_comment_on_the_first_line_of_a_wrapped_header_ends_at_the_line_break() {
    let body =
        "@foreach ($users # active only\n    as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

#[tokio::test]
#[ignore = "known gap: a `)` inside a PHP comment closes a Blade directive's arguments"]
async fn a_multi_line_block_comment_holding_a_paren_does_not_close_the_header() {
    let body = "@foreach ($users /* keep only\n    the :) active ones */\n    as $user)\n    {{ $user->name }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $user->name }}", 4).await;
    assert!(hover.contains("User"), "got: {hover}");
}

/// A header whose paren never closes must not take down any request made
/// against the template, wherever the cursor sits.
#[tokio::test]
async fn an_unclosed_loop_header_does_not_break_requests_anywhere_in_the_template() {
    let body =
        "@foreach ($users as $user\n    {{ $user->name }}\n@endforeach\n{{ $logger->info() }}\n";
    let (backend, uri, text) = template(body).await;

    for (line, content) in text.lines().enumerate() {
        let width = content.encode_utf16().count() as u32;
        for character in 0..=width + 1 {
            let _ = hover_text_at(&backend, &uri, line as u32, character).await;
            let _ = goto_definition_at(&backend, &uri, line as u32, character).await;
        }
    }
    let end = position_after(&text, "{{ $logger->");
    let _ = complete_at_opened_with_trigger(&backend, &uri, end.line, end.character, ">").await;
}

// ─── Nested loops ───────────────────────────────────────────────────────────

const NESTED: &str = "@foreach($categories as $category)
    @foreach($category->items as $item)
        {{ $item->label }}
    @endforeach
    {{ $category->title }}
@endforeach
";

#[tokio::test]
async fn an_inner_loop_iterates_a_member_of_the_outer_binding() {
    let (backend, uri, text) = template(NESTED).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $item->label }}", 4).await;
    assert!(hover.contains("Item"), "got: {hover}");
}

#[tokio::test]
async fn the_outer_binding_survives_the_inner_loop_closing() {
    let (backend, uri, text) = template(NESTED).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $category->title }}", 4).await;
    assert!(hover.contains("Category"), "got: {hover}");
    assert_eq!(
        definition_line_in(&backend, &uri, &text, "title }}", 0).await,
        Some(classes_line("$title")),
    );
}

// ─── The $loop variable ─────────────────────────────────────────────────────

#[tokio::test]
async fn loop_inside_a_foreach_offers_its_members() {
    let body = "@foreach($users as $user)\n    {{ $loop-> }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let at = position_after(&text, "{{ $loop->");
    let labels =
        complete_labels_at_opened_with_trigger(&backend, &uri, at.line, at.character, ">").await;
    for member in [
        "index",
        "iteration",
        "remaining",
        "count",
        "first",
        "last",
        "depth",
        "parent",
    ] {
        assert!(
            labels.iter().any(|l| l == member),
            "$loop should offer {member}, got: {labels:?}"
        );
    }
}

#[tokio::test]
async fn loop_inside_a_forelse_offers_its_members() {
    let body = "@forelse($users as $user)\n    {{ $loop-> }}\n@empty\n    none\n@endforelse\n";
    let (backend, uri, text) = template(body).await;

    let at = position_after(&text, "{{ $loop->");
    let labels =
        complete_labels_at_opened_with_trigger(&backend, &uri, at.line, at.character, ">").await;
    assert!(labels.iter().any(|l| l == "iteration"), "got: {labels:?}");
}

#[tokio::test]
async fn loop_is_defined_inside_a_foreach() {
    let body = "@foreach($users as $user)\n    {{ $loop->index }} {{ $user->name }}\n@endforeach\n";
    let (backend, uri, _text) = template(body).await;

    let undefined = blade_undefined_variables(&backend, &uri);
    assert!(undefined.is_empty(), "got: {undefined:?}");
}

/// Laravel's `$loop->parent` is the enclosing loop's own `$loop` object, so
/// it carries the same members.
#[tokio::test]
#[ignore = "known gap: `$loop->parent` is typed `?object`, not the loop shape"]
async fn loop_parent_in_a_nested_loop_offers_the_outer_loops_members() {
    let body = "@foreach($categories as $category)\n    @foreach($category->items as $item)\n        {{ $loop->parent-> }}\n    @endforeach\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let at = position_after(&text, "{{ $loop->parent->");
    let labels =
        complete_labels_at_opened_with_trigger(&backend, &uri, at.line, at.character, ">").await;
    assert!(
        labels.iter().any(|l| l == "iteration") && labels.iter().any(|l| l == "depth"),
        "$loop->parent should be a loop object too, got: {labels:?}"
    );
}

/// Only `@foreach` and `@forelse` push a loop onto Blade's loop stack.
#[tokio::test]
async fn loop_is_undefined_outside_any_loop() {
    let body = "{{ $loop->index }}\n";
    let (backend, uri, _text) = template(body).await;

    let undefined = blade_undefined_variables(&backend, &uri);
    assert!(
        undefined.iter().any(|m| m.contains("$loop")),
        "got: {undefined:?}"
    );
}

#[tokio::test]
async fn a_while_loop_does_not_introduce_loop() {
    let body = "@while($roster)\n    {{ $loop->index }}\n@endwhile\n";
    let (backend, uri, _text) = template(body).await;

    let undefined = blade_undefined_variables(&backend, &uri);
    assert!(
        undefined.iter().any(|m| m.contains("$loop")),
        "Blade compiles @while to a bare while, got: {undefined:?}"
    );
}

#[tokio::test]
async fn a_for_loop_does_not_introduce_loop() {
    let body = "@for($i = 0; $i < 3; $i++)\n    {{ $loop->index }}\n@endfor\n";
    let (backend, uri, _text) = template(body).await;

    let undefined = blade_undefined_variables(&backend, &uri);
    assert!(
        undefined.iter().any(|m| m.contains("$loop")),
        "Blade compiles @for to a bare for, got: {undefined:?}"
    );
}

#[tokio::test]
async fn loop_assigned_in_a_php_block_carries_its_shape() {
    let body =
        "@foreach($users as $user)\n@php($meta = $loop)\n    {{ $meta->index }}\n@endforeach\n";
    let (backend, uri, text) = template(body).await;

    let hover = hover_in(&backend, &uri, &text, "{{ $meta->index }}", 4).await;
    assert!(hover.contains("iteration"), "got: {hover}");
}

// ─── Embedded PHP regions ───────────────────────────────────────────────────

#[tokio::test]
async fn a_member_inside_a_raw_echo_leads_to_its_declaration() {
    let body = "<div>\n{!! $logger->warn() !!}\n</div>\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "warn() !!}", 1).await,
        Some(classes_line("function warn")),
    );
}

#[tokio::test]
async fn the_second_echo_on_a_line_maps_to_its_own_columns() {
    let body = "<a title=\"{{ $logger->info() }}\">{{ $logger->warn() }}</a>\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "info() }}", 1).await,
        Some(classes_line("function info")),
    );
    assert_eq!(
        definition_line_in(&backend, &uri, &text, "warn() }}", 1).await,
        Some(classes_line("function warn")),
    );
}

#[tokio::test]
async fn an_echo_after_multibyte_text_maps_its_columns() {
    let body = "<p>café 🎉 naïve {{ $logger->warn() }}</p>\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "warn() }}", 1).await,
        Some(classes_line("function warn")),
    );
}

#[tokio::test]
async fn an_echo_spanning_lines_maps_its_inner_line() {
    let body = "{{\n    $logger->info()\n}}\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "info()\n}}", 1).await,
        Some(classes_line("function info")),
    );
}

#[tokio::test]
async fn a_multi_line_php_block_assignment_is_in_scope_below_it() {
    let body =
        "@php\n    $channel = new Logger();\n    $label = 'x';\n@endphp\n{{ $channel->info() }}\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "info() }}", 1).await,
        Some(classes_line("function info")),
    );
}

#[tokio::test]
async fn a_native_php_tag_assignment_is_in_scope_below_it() {
    let body = "<?php $channel = new Logger(); ?>\n{{ $channel->warn() }}\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "warn() }}", 1).await,
        Some(classes_line("function warn")),
    );
}

/// Blade anchors directives with `\B`, so the `@foreach` in an email address
/// is text, not a loop opening that swallows the rest of the template.
#[tokio::test]
async fn an_email_address_does_not_open_a_directive() {
    let body = "<p>Mail support@foreach.example</p>\n{{ $logger->info() }}\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "info() }}", 1).await,
        Some(classes_line("function info")),
    );
}

#[tokio::test]
async fn a_directive_after_multibyte_text_still_lowers() {
    let body = "<p>🎉 café</p> @if($logger)\n    {{ $logger->warn() }}\n@endif\n";
    let (backend, uri, text) = template(body).await;

    assert_eq!(
        definition_line_in(&backend, &uri, &text, "warn() }}", 1).await,
        Some(classes_line("function warn")),
    );
}

// ─── Directive-name completion ──────────────────────────────────────────────

#[tokio::test]
async fn directive_completion_fires_after_an_emoji() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    let text = "<p>🎉 @fo</p>";
    open_document(&backend, &uri, "blade", text).await;

    let at = position_after(text, "@fo");
    let items = complete_at_opened_with_trigger(&backend, &uri, at.line, at.character, "@").await;
    assert!(
        labels(&items).contains(&"@foreach"),
        "got: {:?}",
        labels(&items)
    );
}

#[tokio::test]
async fn directive_completion_fires_after_an_accented_letter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    let text = "é @for";
    open_document(&backend, &uri, "blade", text).await;

    let at = position_after(text, "@for");
    let items = complete_at_opened_with_trigger(&backend, &uri, at.line, at.character, "@").await;
    assert!(
        labels(&items).contains(&"@foreach"),
        "got: {:?}",
        labels(&items)
    );
}

#[tokio::test]
async fn an_at_sign_glued_to_a_word_is_not_a_directive_position() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    let text = "<p>foo@fo</p>";
    open_document(&backend, &uri, "blade", text).await;

    let at = position_after(text, "@fo");
    let items = complete_at_opened_with_trigger(&backend, &uri, at.line, at.character, "@").await;
    assert!(
        !labels(&items).contains(&"@foreach"),
        "Blade reads `foo@fo` as text, got: {:?}",
        labels(&items)
    );
}

// ─── @props lists, read through a component tag's attribute completion ──────

/// A project whose anonymous components exercise the shapes a `@props`
/// list can take, plus `resources/views/page.blade.php` holding `template`.
fn component_workspace(template: &str) -> (Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        BLADE_COMPONENT_COMPOSER,
        &[
            (
                "stubs/Illuminate/View/Component.php",
                ILLUMINATE_COMPONENT_STUB,
            ),
            ("stubs/Livewire/Component.php", LIVEWIRE_COMPONENT_STUB),
            (
                "resources/views/components/note.blade.php",
                "@props(['note' => 'something (parens) inside', 'count' => 0])\n<p>{{ $note }} {{ $count }}</p>\n",
            ),
            (
                "resources/views/components/tally.blade.php",
                "@props(['summary' => 'a, b, c', 'total' => 0])\n<p>{{ $summary }} {{ $total }}</p>\n",
            ),
            (
                "resources/views/components/options.blade.php",
                "@props(['opts' => ['alpha', 'beta'], 'label' => 'Hi'])\n<p>{{ $label }}</p>\n",
            ),
            (
                "resources/views/components/twice.blade.php",
                "@props(['first' => 1])\n@props(['second' => 2])\n<p>{{ $first }} {{ $second }}</p>\n",
            ),
            (
                "resources/views/components/extended.blade.php",
                "@propsExtended(['ghost' => 1])\n@props(['real' => 2])\n<p>{{ $real }}</p>\n",
            ),
            (
                "resources/views/components/bare.blade.php",
                "@props([])\n<p>{{ $slot }}</p>\n",
            ),
            (
                "resources/views/components/price.blade.php",
                "@props(['caption' => 'naïve façade €', 'size' => 'base'])\n<p>{{ $caption }} {{ $size }}</p>\n",
            ),
            (
                "resources/views/components/quote.blade.php",
                "@props(['text' => 'it\\'s', 'author' => 'anon'])\n<p>{{ $text }} {{ $author }}</p>\n",
            ),
            (
                "resources/views/components/card.blade.php",
                "<div {{ $attributes->merge(['class' => 'card']) }}>\n\
                 {{ $errors->first('email') }} {{ $component->data() }} {{ $heading }}\n\
                 </div>\n",
            ),
            ("resources/views/page.blade.php", template),
        ],
    );
    let uri = workspace_uri(&backend, "resources/views/page.blade.php");
    (backend, dir, uri)
}

/// The attribute names offered at the end of `template`, a component tag
/// being typed.
async fn attributes_offered(template: &str) -> Vec<CompletionItem> {
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;
    let at = position_after(template, template);
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: at,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::INVOKED,
            trigger_character: None,
        }),
    };
    match backend.completion(params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

#[tokio::test]
async fn a_paren_inside_a_props_default_string_does_not_end_the_list() {
    let items = attributes_offered("<x-note ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"note") && labels.contains(&"count"),
        "got: {labels:?}"
    );
}

#[tokio::test]
async fn a_comma_inside_a_props_default_string_does_not_split_an_entry() {
    let items = attributes_offered("<x-tally ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"summary") && labels.contains(&"total"),
        "got: {labels:?}"
    );
    assert!(
        !labels
            .iter()
            .any(|l| l.contains(' ') || *l == "b" || *l == "c"),
        "a piece of a default string is not a prop, got: {labels:?}"
    );
}

#[tokio::test]
async fn strings_inside_a_nested_array_default_are_not_props() {
    let items = attributes_offered("<x-options ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"opts") && labels.contains(&"label"),
        "got: {labels:?}"
    );
    assert!(
        !labels.contains(&"alpha") && !labels.contains(&"beta"),
        "the default's elements are not props, got: {labels:?}"
    );
}

/// Blade compiles every `@props` in a template, and each one lifts its own
/// names out of the attribute bag.
#[tokio::test]
async fn every_props_directive_in_a_template_declares_its_names() {
    let items = attributes_offered("<x-twice ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"first") && labels.contains(&"second"),
        "got: {labels:?}"
    );
}

#[tokio::test]
async fn a_directive_whose_name_only_starts_with_props_declares_nothing() {
    let items = attributes_offered("<x-extended ").await;
    let labels = labels(&items);
    assert!(labels.contains(&"real"), "got: {labels:?}");
    assert!(
        !labels.contains(&"ghost"),
        "@propsExtended is not @props, got: {labels:?}"
    );
}

#[tokio::test]
async fn an_empty_props_list_offers_no_props() {
    let items = attributes_offered("<x-bare ").await;
    let labels = labels(&items);
    assert!(
        !labels.contains(&"slot"),
        "$slot is the component's own, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_multibyte_props_default_is_kept_whole() {
    let items = attributes_offered("<x-price ").await;
    let caption = items
        .iter()
        .find(|item| item.label == "caption")
        .unwrap_or_else(|| panic!("expected the caption prop, got: {:?}", labels(&items)));
    assert_eq!(caption.detail.as_deref(), Some("= 'naïve façade €'"));
    assert!(
        labels(&items).contains(&"size"),
        "got: {:?}",
        labels(&items)
    );
}

#[tokio::test]
async fn an_escaped_quote_in_a_props_default_does_not_end_the_list() {
    let items = attributes_offered("<x-quote ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"text") && labels.contains(&"author"),
        "got: {labels:?}"
    );
}

#[tokio::test]
async fn framework_variables_a_component_reads_are_not_attributes() {
    let items = attributes_offered("<x-card ").await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"heading"),
        "expected the undeclared read, got: {labels:?}"
    );
    for name in ["attributes", "errors", "component"] {
        assert!(
            !labels.contains(&name),
            "{name} is supplied by Blade itself, got: {labels:?}"
        );
    }
}
