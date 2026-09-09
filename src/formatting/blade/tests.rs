//! The reindenter's specification, and the Blade strategy resolution.
//!
//! The `indentation_*` cases are ported from joelstein/blade-formatter's
//! `IndentationFormatterTest.php` (MIT), the closest thing to a
//! specification of reindent-only Blade formatting that exists. Two of
//! them (`<script>` and `<style>` bodies) have re-derived expectations:
//! that formatter reindents JavaScript and CSS by brace depth, while
//! this one shifts an embedded language as a block and leaves its own
//! layout alone.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use mago_formatter::settings::FormatSettings;

use crate::config::FormattingConfig;
use crate::types::PhpVersion;

use super::reindent::{Options, reindent};
use super::{
    BladeFormattingStrategy, format_blade_content, is_whitespace_sensitive, options_from_lsp,
    resolve_blade_strategy,
};

/// Reindent `input` and check it against `expected`, then check the two
/// properties every reindent-only formatter has: formatting is
/// idempotent, and every output line is its input line with different
/// leading whitespace.
fn check(input: &str, expected: &str) {
    check_with(input, expected, &Options::default());
}

fn check_with(input: &str, expected: &str, options: &Options) {
    let actual = reindent(input, options);
    assert_eq!(
        actual, expected,
        "\n--- input ---\n{input}\n--- actual ---\n{actual}"
    );
    assert_eq!(
        reindent(&actual, options),
        actual,
        "formatting is not idempotent"
    );
    for (before, after) in input.lines().zip(actual.lines()) {
        assert_eq!(
            before.trim_start_matches([' ', '\t']),
            after.trim_start_matches([' ', '\t']),
            "a line changed beyond its leading whitespace"
        );
    }
    assert_eq!(input.lines().count(), actual.lines().count());
}

// ── HTML tag indentation ──────────────────────────────────────────

#[test]
fn indentation_indents_children_of_opening_tags() {
    check(
        "\n<div>\n<p>Hello</p>\n</div>",
        "\n<div>\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_handles_nested_tags() {
    check(
        "\n<div>\n<section>\n<p>Hello</p>\n</section>\n</div>",
        "\n<div>\n    <section>\n        <p>Hello</p>\n    </section>\n</div>",
    );
}

#[test]
fn indentation_does_not_indent_after_void_elements() {
    check(
        "\n<div>\n<input>\n<img>\n<p>Hello</p>\n</div>",
        "\n<div>\n    <input>\n    <img>\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_indent_after_self_closing_tags() {
    check(
        "\n<div>\n<br />\n<hr />\n<p>Hello</p>\n</div>",
        "\n<div>\n    <br />\n    <hr />\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_handles_same_line_open_and_close_tags() {
    check(
        "\n<div>\n<p>Hello</p>\n<p>World</p>\n</div>",
        "\n<div>\n    <p>Hello</p>\n    <p>World</p>\n</div>",
    );
}

#[test]
fn indentation_preserves_empty_lines() {
    check(
        "\n<div>\n\n<p>Hello</p>\n\n</div>",
        "\n<div>\n\n    <p>Hello</p>\n\n</div>",
    );
}

// ── Blade component indentation ──────────────────────────────────

#[test]
fn indentation_indents_children_of_flux_components() {
    check(
        "\n<flux:modal>\n<p>Content</p>\n</flux:modal>",
        "\n<flux:modal>\n    <p>Content</p>\n</flux:modal>",
    );
}

#[test]
fn indentation_indents_children_of_x_components() {
    check(
        "\n<x-layout>\n<x-slot name=\"header\">\n<h1>Title</h1>\n</x-slot>\n<p>Body</p>\n</x-layout>",
        "\n<x-layout>\n    <x-slot name=\"header\">\n        <h1>Title</h1>\n    </x-slot>\n    <p>Body</p>\n</x-layout>",
    );
}

#[test]
fn indentation_does_not_indent_after_self_closing_components() {
    check(
        "\n<div>\n<flux:button />\n<x-icon name=\"check\" />\n<p>Hello</p>\n</div>",
        "\n<div>\n    <flux:button />\n    <x-icon name=\"check\" />\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_indent_after_self_closing_tags_with_gt_in_attribute_values() {
    check(
        "\n<flux:dropdown>\n<flux:profile :initials=\"auth()->user()->initials()\" />\n<flux:menu>\n<p>Content</p>\n</flux:menu>\n</flux:dropdown>",
        "\n<flux:dropdown>\n    <flux:profile :initials=\"auth()->user()->initials()\" />\n    <flux:menu>\n        <p>Content</p>\n    </flux:menu>\n</flux:dropdown>",
    );
}

// ── Blade directive indentation ──────────────────────────────────

#[test]
fn indentation_indents_if_endif_blocks() {
    check(
        "\n@if($show)\n<p>Visible</p>\n@endif",
        "\n@if($show)\n    <p>Visible</p>\n@endif",
    );
}

#[test]
fn indentation_handles_else_at_same_level_as_if() {
    check(
        "\n@if($show)\n<p>Yes</p>\n@else\n<p>No</p>\n@endif",
        "\n@if($show)\n    <p>Yes</p>\n@else\n    <p>No</p>\n@endif",
    );
}

#[test]
fn indentation_handles_elseif_at_same_level_as_if() {
    check(
        "\n@if($a)\n<p>A</p>\n@elseif($b)\n<p>B</p>\n@else\n<p>C</p>\n@endif",
        "\n@if($a)\n    <p>A</p>\n@elseif($b)\n    <p>B</p>\n@else\n    <p>C</p>\n@endif",
    );
}

#[test]
fn indentation_indents_foreach_endforeach_blocks() {
    check(
        "\n@foreach($items as $item)\n<li>{{ $item }}</li>\n@endforeach",
        "\n@foreach($items as $item)\n    <li>{{ $item }}</li>\n@endforeach",
    );
}

#[test]
fn indentation_indents_nested_directives() {
    check(
        "\n@foreach($items as $item)\n@if($item->active)\n<li>{{ $item->name }}</li>\n@endif\n@endforeach",
        "\n@foreach($items as $item)\n    @if($item->active)\n        <li>{{ $item->name }}</li>\n    @endif\n@endforeach",
    );
}

#[test]
fn indentation_indents_auth_endauth_blocks() {
    check(
        "\n@auth\n<p>Logged in</p>\n@endauth",
        "\n@auth\n    <p>Logged in</p>\n@endauth",
    );
}

#[test]
fn indentation_indents_error_enderror_blocks() {
    check(
        "\n<div>\n@error('email')\n<span>{{ $message }}</span>\n@enderror\n</div>",
        "\n<div>\n    @error('email')\n        <span>{{ $message }}</span>\n    @enderror\n</div>",
    );
}

/// `@feature` is Pennant's, not Blade's: the template defines the block
/// by writing its `@endfeature`.
#[test]
fn indentation_indents_feature_endfeature_blocks() {
    check(
        "\n@feature('new-layout')\n<p>New</p>\n@else\n<p>Old</p>\n@endfeature",
        "\n@feature('new-layout')\n    <p>New</p>\n@else\n    <p>Old</p>\n@endfeature",
    );
}

#[test]
fn indentation_indents_has_section_blocks_closed_by_endif() {
    check(
        "\n<div>\n@hasSection('sidebar')\n<aside></aside>\n@endif\n</div>",
        "\n<div>\n    @hasSection('sidebar')\n        <aside></aside>\n    @endif\n</div>",
    );
}

#[test]
fn indentation_indents_section_missing_blocks_closed_by_endif() {
    check(
        "\n@sectionMissing('sidebar')\n<p>No sidebar</p>\n@else\n<p>Has sidebar</p>\n@endif",
        "\n@sectionMissing('sidebar')\n    <p>No sidebar</p>\n@else\n    <p>Has sidebar</p>\n@endif",
    );
}

#[test]
fn indentation_does_not_indent_single_line_has_section_blocks() {
    check(
        "\n<div>\n@hasSection('sidebar') <aside></aside> @endif\n<p>After</p>\n</div>",
        "\n<div>\n    @hasSection('sidebar') <aside></aside> @endif\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_indents_nested_has_section_and_if_blocks() {
    check(
        "\n@if($show)\n@hasSection('sidebar')\n<aside></aside>\n@endif\n<p>After</p>\n@endif",
        "\n@if($show)\n    @hasSection('sidebar')\n        <aside></aside>\n    @endif\n    <p>After</p>\n@endif",
    );
}

#[test]
fn indentation_indents_switch_case_endswitch_blocks() {
    check(
        "\n@switch($type)\n@case('a')\n<p>A</p>\n@break\n@default\n<p>Default</p>\n@endswitch",
        "\n@switch($type)\n    @case('a')\n        <p>A</p>\n        @break\n    @default\n        <p>Default</p>\n@endswitch",
    );
}

#[test]
fn indentation_aligns_comment_before_case_with_the_case_directive() {
    check(
        "\n@switch($type)\n{{-- handles a --}}\n@case('a')\n<p>A</p>\n@break\n{{-- handles b --}}\n@case('b')\n<p>B</p>\n@break\n@endswitch",
        "\n@switch($type)\n    {{-- handles a --}}\n    @case('a')\n        <p>A</p>\n        @break\n    {{-- handles b --}}\n    @case('b')\n        <p>B</p>\n        @break\n@endswitch",
    );
}

#[test]
fn indentation_aligns_comment_before_case_when_separated_by_blank_line() {
    check(
        "\n@switch($type)\n\n{{-- handles a --}}\n@case('a')\n<p>A</p>\n@break\n@endswitch",
        "\n@switch($type)\n\n    {{-- handles a --}}\n    @case('a')\n        <p>A</p>\n        @break\n@endswitch",
    );
}

#[test]
fn indentation_aligns_comment_before_case_that_falls_through() {
    check(
        "\n@switch($type)\n@case('a')\n<p>A</p>\n{{-- handles b --}}\n@case('b')\n<p>B</p>\n@break\n@endswitch",
        "\n@switch($type)\n    @case('a')\n        <p>A</p>\n    {{-- handles b --}}\n    @case('b')\n        <p>B</p>\n        @break\n@endswitch",
    );
}

#[test]
fn indentation_aligns_comment_before_else_with_the_else_directive() {
    check(
        "\n@if($a)\n<p>A</p>\n{{-- otherwise --}}\n@else\n<p>B</p>\n@endif",
        "\n@if($a)\n    <p>A</p>\n{{-- otherwise --}}\n@else\n    <p>B</p>\n@endif",
    );
}

#[test]
fn indentation_does_not_indent_after_inline_php_endphp() {
    check(
        "\n<div>\n@php $var = 'value'; @endphp\n<p>After</p>\n</div>",
        "\n<div>\n    @php $var = 'value'; @endphp\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_indent_after_inline_php_expression() {
    check(
        "\n<div>\n@php($limit = app(Config::class)->perPage)\n<flux:input wire:model=\"name\" />\n<flux:switch wire:model=\"active\" />\n</div>",
        "\n<div>\n    @php($limit = app(Config::class)->perPage)\n    <flux:input wire:model=\"name\" />\n    <flux:switch wire:model=\"active\" />\n</div>",
    );
}

#[test]
fn indentation_still_indents_block_php_endphp() {
    check(
        "\n<div>\n@php\n$var = 'value';\n@endphp\n<p>After</p>\n</div>",
        "\n<div>\n    @php\n        $var = 'value';\n    @endphp\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_preserves_relative_whitespace_in_php_blocks() {
    check(
        "\n<div>\n@php\n$list = collect($items)\n    ->map(fn ($i) => $i->name)\n    ->join(', ');\n@endphp\n</div>",
        "\n<div>\n    @php\n        $list = collect($items)\n            ->map(fn ($i) => $i->name)\n            ->join(', ');\n    @endphp\n</div>",
    );
}

/// The shift keeps the block's own layout whatever indentation it
/// arrived with, so shifting is idempotent where prepending is not.
#[test]
fn indentation_shifts_an_already_indented_php_block() {
    check(
        "\n<div>\n@php\n            $a = 1;\n                $b = 2;\n@endphp\n</div>",
        "\n<div>\n    @php\n        $a = 1;\n            $b = 2;\n    @endphp\n</div>",
    );
}

#[test]
fn indentation_indents_placeholder_endplaceholder_blocks() {
    check(
        "\n@placeholder\n<p>Loading...</p>\n@endplaceholder",
        "\n@placeholder\n    <p>Loading...</p>\n@endplaceholder",
    );
}

/// A `Blade::if` family the project registers pairs up through its
/// `@end…` like any other block, its `@unless…` opener included.
#[test]
fn indentation_pairs_custom_conditional_directives() {
    check(
        "\n@bakeryOpen\n<p>Open</p>\n@elsebakeryOpen\n<p>Closed</p>\n@endbakeryOpen\n@unlessbakeryOpen\n<p>Shut</p>\n@endbakeryOpen",
        "\n@bakeryOpen\n    <p>Open</p>\n@elsebakeryOpen\n    <p>Closed</p>\n@endbakeryOpen\n@unlessbakeryOpen\n    <p>Shut</p>\n@endbakeryOpen",
    );
}

#[test]
fn indentation_treats_forelse_empty_as_a_separator() {
    check(
        "\n@forelse($rows as $row)\n<li>{{ $row }}</li>\n@empty\n<li>None</li>\n@endforelse",
        "\n@forelse($rows as $row)\n    <li>{{ $row }}</li>\n@empty\n    <li>None</li>\n@endforelse",
    );
}

// ── Brace handling ──────────────────────────────────────────────

#[test]
fn indentation_handles_else_brace_at_correct_indent() {
    check(
        "\n<div>\nif ($a) {\n$b = 1;\n} else {\n$b = 2;\n}\n</div>",
        "\n<div>\n    if ($a) {\n        $b = 1;\n    } else {\n        $b = 2;\n    }\n</div>",
    );
}

#[test]
fn indentation_ignores_braces_inside_quoted_strings() {
    check(
        "\n<div>\n@t('{count, plural, one {# item} other {# items}}')\n<p>After</p>\n</div>",
        "\n<div>\n    @t('{count, plural, one {# item} other {# items}}')\n    <p>After</p>\n</div>",
    );
}

// ── Multi-line tag indentation ───────────────────────────────────

#[test]
fn indentation_indents_attributes_of_multi_line_tags() {
    check(
        "\n<flux:input\nname=\"email\"\ntype=\"email\"\nrequired\n/>",
        "\n<flux:input\n    name=\"email\"\n    type=\"email\"\n    required\n/>",
    );
}

#[test]
fn indentation_aligns_closing_bracket_with_opening_tag() {
    check(
        "\n<div\nclass=\"container\"\n>\n<p>Hello</p>\n</div>",
        "\n<div\n    class=\"container\"\n>\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_handles_self_closing_multi_line_tags_without_adding_child_indent() {
    check(
        "\n<div>\n<flux:input\nname=\"email\"\ntype=\"email\"\n/>\n<p>After</p>\n</div>",
        "\n<div>\n    <flux:input\n        name=\"email\"\n        type=\"email\"\n    />\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_indents_nested_multi_line_tags() {
    check(
        "\n<div>\n<form\nmethod=\"POST\"\naction=\"/submit\"\n>\n<flux:input\nname=\"email\"\nrequired\n/>\n</form>\n</div>",
        "\n<div>\n    <form\n        method=\"POST\"\n        action=\"/submit\"\n    >\n        <flux:input\n            name=\"email\"\n            required\n        />\n    </form>\n</div>",
    );
}

#[test]
fn indentation_indents_directive_content_inside_multi_line_tags() {
    check(
        "\n<a\n@if($linked)\nhref=\"https://example.com\"\ntarget=\"_blank\"\n@endif\nclass=\"btn\"\n>\nlink\n</a>",
        "\n<a\n    @if($linked)\n        href=\"https://example.com\"\n        target=\"_blank\"\n    @endif\n    class=\"btn\"\n>\n    link\n</a>",
    );
}

#[test]
fn indentation_indents_if_else_inside_multi_line_tags() {
    check(
        "\n<a\n@if($linked)\nhref=\"https://example.com\"\n@else\nhref=\"#\"\n@endif\nclass=\"btn\"\n>\nlink\n</a>",
        "\n<a\n    @if($linked)\n        href=\"https://example.com\"\n    @else\n        href=\"#\"\n    @endif\n    class=\"btn\"\n>\n    link\n</a>",
    );
}

// ── Alpine x-data indentation ────────────────────────────────────

#[test]
fn indentation_indents_x_data_object_contents() {
    check(
        "\n<div\nx-data=\"{\nopen: false,\nname: '',\n}\"\n>\n<p>Hello</p>\n</div>",
        "\n<div\n    x-data=\"{\n        open: false,\n        name: '',\n    }\"\n>\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_indents_nested_functions_in_x_data() {
    check(
        "\n<div\nx-data=\"{\nopen: false,\ntoggle() {\nthis.open = !this.open;\n},\n}\"\n>\n<p>Content</p>\n</div>",
        "\n<div\n    x-data=\"{\n        open: false,\n        toggle() {\n            this.open = !this.open;\n        },\n    }\"\n>\n    <p>Content</p>\n</div>",
    );
}

#[test]
fn indentation_indents_x_data_brace_content_on_tag_opening_line() {
    check(
        "\n<div class=\"flex\" x-data=\"{\nopen: false,\n}\">\n<p>Hello</p>\n</div>",
        "\n<div class=\"flex\" x-data=\"{\n    open: false,\n}\">\n    <p>Hello</p>\n</div>",
    );
}

/// The attribute list of a tag whose own line left a bracket open
/// returns to one level in once the bracket closes.
#[test]
fn indentation_returns_to_attribute_level_after_a_brace_on_the_tag_line() {
    check(
        "\n<div x-data=\"{\nopen: false,\n}\"\nclass=\"x\"\n>\n<p>Hello</p>\n</div>",
        "\n<div x-data=\"{\n    open: false,\n}\"\n    class=\"x\"\n>\n    <p>Hello</p>\n</div>",
    );
}

#[test]
fn indentation_indents_multiline_alpine_attribute_values() {
    check(
        "\n<button\nx-on:click=\"\nconst a = 1;\nconst b = 2;\n\"\nclass=\"btn\"\n>\nClick\n</button>",
        "\n<button\n    x-on:click=\"\n        const a = 1;\n        const b = 2;\n    \"\n    class=\"btn\"\n>\n    Click\n</button>",
    );
}

#[test]
fn indentation_indents_ternary_operators_in_multiline_attribute_values() {
    check(
        "\n<form\nx-on:submit.prevent=\"\ncondition\n? doA()\n: doB();\n\"\n>",
        "\n<form\n    x-on:submit.prevent=\"\n        condition\n            ? doA()\n            : doB();\n    \"\n>",
    );
}

#[test]
fn indentation_indents_dot_chained_methods_in_multiline_attribute_values() {
    check(
        "\n<flux:input\nx-on:input=\"\nvalue\n.toLowerCase()\n.trim()\n\"\n/>",
        "\n<flux:input\n    x-on:input=\"\n        value\n            .toLowerCase()\n            .trim()\n    \"\n/>",
    );
}

#[test]
fn indentation_indents_ternary_operators_in_multi_line_tag_attributes() {
    check(
        "\n<button\n:class=\"condition\n? 'active'\n: 'inactive'\"\nclass=\"btn\"\n>",
        "\n<button\n    :class=\"condition\n        ? 'active'\n        : 'inactive'\"\n    class=\"btn\"\n>",
    );
}

#[test]
fn indentation_indents_else_blocks_in_multiline_attribute_values() {
    let input = "
<input
x-effect=\"
rawValue; focused;
$nextTick(() => {
if (!focused) {
const num = parseFloat(rawValue);
if (rawValue !== '' && !isNaN(num) && isFinite(num)) {
$el.value = num.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
} else {
$el.value = rawValue;
}
}
});
\"
/>";
    let expected = "
<input
    x-effect=\"
        rawValue; focused;
        $nextTick(() => {
            if (!focused) {
                const num = parseFloat(rawValue);
                if (rawValue !== '' && !isNaN(num) && isFinite(num)) {
                    $el.value = num.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
                } else {
                    $el.value = rawValue;
                }
            }
        });
    \"
/>";
    check(input, expected);
}

#[test]
fn indentation_outdents_closing_paren_of_multi_line_directive_condition() {
    check(
        "\n@if (\n! app()->isLocal()\n&& ! auth()->check()\n|| $override\n)\n<p>Content</p>\n@endif",
        "\n@if (\n    ! app()->isLocal()\n    && ! auth()->check()\n    || $override\n)\n    <p>Content</p>\n@endif",
    );
}

#[test]
fn indentation_outdents_closing_paren_of_multi_line_elseif_condition() {
    check(
        "\n@if ($a)\n<p>A</p>\n@elseif (\n$b\n)\n<p>B</p>\n@endif",
        "\n@if ($a)\n    <p>A</p>\n@elseif (\n    $b\n)\n    <p>B</p>\n@endif",
    );
}

#[test]
fn indentation_treats_inline_section_with_two_arguments_as_non_block() {
    check(
        "\n@extends('pdf')\n\n@section('title', 'Invoice')\n\n@section('content')\n<div>Content</div>\n@endsection",
        "\n@extends('pdf')\n\n@section('title', 'Invoice')\n\n@section('content')\n    <div>Content</div>\n@endsection",
    );
}

#[test]
fn indentation_indents_continuation_lines_inside_braces() {
    check(
        "\n{!! t('test', [\n'key' => 'value'\n. 'more',\n]) !!}",
        "\n{!! t('test', [\n    'key' => 'value'\n        . 'more',\n]) !!}",
    );
}

// ── @class inside tags ──────────────────────────────────────────

#[test]
fn indentation_indents_class_directive_on_tag_opening_line() {
    check(
        "\n<div @class([\n'h-3',\n'bg-green-500' => true,\n])></div>",
        "\n<div @class([\n    'h-3',\n    'bg-green-500' => true,\n])></div>",
    );
}

#[test]
fn indentation_indents_class_directive_on_own_attribute_line() {
    check(
        "\n<div\n@class([\n'h-3',\n])\n>\nContent\n</div>",
        "\n<div\n    @class([\n        'h-3',\n    ])\n>\n    Content\n</div>",
    );
}

#[test]
fn indentation_indents_children_when_tag_has_inline_class_directive_with_arrow_operators() {
    check(
        "\n<nav @class(['active' => $open, 'hidden' => ! $open])>\n@if ($show)\n<p>Hi</p>\n@endif\n</nav>",
        "\n<nav @class(['active' => $open, 'hidden' => ! $open])>\n    @if ($show)\n        <p>Hi</p>\n    @endif\n</nav>",
    );
}

#[test]
fn indentation_indents_style_directive_on_own_attribute_line() {
    check(
        "\n<div\n@style([\n'color: red' => $highlight,\n])\n>\nContent\n</div>",
        "\n<div\n    @style([\n        'color: red' => $highlight,\n    ])\n>\n    Content\n</div>",
    );
}

// ── Brace nesting outside tags ───────────────────────────────────

#[test]
fn indentation_indents_props_array_contents() {
    check(
        "\n@props([\n'on',\n'color' => 'blue',\n])",
        "\n@props([\n    'on',\n    'color' => 'blue',\n])",
    );
}

#[test]
fn indentation_indents_nested_arrays_in_props() {
    check(
        "\n@props([\n'options' => [\n'a',\n'b',\n],\n])",
        "\n@props([\n    'options' => [\n        'a',\n        'b',\n    ],\n])",
    );
}

#[test]
fn indentation_indents_multi_line_echo_arguments() {
    check(
        "\n<p>\n{{ __('messages.welcome', [\n'name' => $name,\n]) }}\n</p>",
        "\n<p>\n    {{ __('messages.welcome', [\n        'name' => $name,\n    ]) }}\n</p>",
    );
}

// ── Mixed content ────────────────────────────────────────────────

#[test]
fn indentation_handles_a_realistic_blade_template() {
    check(
        "\n<div>\n@if($users->count())\n<ul>\n@foreach($users as $user)\n<li>\n<x-avatar\n:src=\"$user->avatar\"\n:alt=\"$user->name\"\n/>\n<span>{{ $user->name }}</span>\n</li>\n@endforeach\n</ul>\n@else\n<p>No users found.</p>\n@endif\n</div>",
        "\n<div>\n    @if($users->count())\n        <ul>\n            @foreach($users as $user)\n                <li>\n                    <x-avatar\n                        :src=\"$user->avatar\"\n                        :alt=\"$user->name\"\n                    />\n                    <span>{{ $user->name }}</span>\n                </li>\n            @endforeach\n        </ul>\n    @else\n        <p>No users found.</p>\n    @endif\n</div>",
    );
}

#[test]
fn indentation_handles_custom_indent_size() {
    check_with(
        "\n<div>\n<p>Hello</p>\n</div>",
        "\n<div>\n  <p>Hello</p>\n</div>",
        &Options {
            indent: "  ".to_string(),
            ..Options::default()
        },
    );
}

#[test]
fn indentation_handles_tabs() {
    check_with(
        "\n<div>\n    <p>\n    Hello\n</p>\n</div>",
        "\n<div>\n\t<p>\n\t\tHello\n\t</p>\n</div>",
        &Options {
            indent: "\t".to_string(),
            ..Options::default()
        },
    );
}

// ── Preserved blocks ─────────────────────────────────────────────

#[test]
fn indentation_preserves_content_inside_verbatim() {
    check(
        "\n<div>\n@verbatim\n    <div>\n        {{ $unprocessed }}\n    </div>\n@endverbatim\n</div>",
        "\n<div>\n    @verbatim\n    <div>\n        {{ $unprocessed }}\n    </div>\n@endverbatim\n</div>",
    );
}

#[test]
fn indentation_preserves_content_inside_pre_tags() {
    check(
        "\n<div>\n<pre>\n  line 1\n    line 2\n      line 3\n</pre>\n</div>",
        "\n<div>\n    <pre>\n  line 1\n    line 2\n      line 3\n</pre>\n</div>",
    );
}

#[test]
fn indentation_preserves_content_inside_textarea_tags() {
    check(
        "\n<div>\n<textarea>\n  keep\n</textarea>\n<p>After</p>\n</div>",
        "\n<div>\n    <textarea>\n  keep\n</textarea>\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_inject_whitespace_before_a_closing_pre_tag() {
    check(
        "\n<div>\n<section>\n<pre>\ncode\n</pre>\n</section>\n</div>",
        "\n<div>\n    <section>\n        <pre>\ncode\n</pre>\n    </section>\n</div>",
    );
}

#[test]
fn indentation_keeps_existing_indentation_of_a_closing_pre_tag() {
    check(
        "\n<div>\n<pre>\ncode\n        </pre>\n</div>",
        "\n<div>\n    <pre>\ncode\n        </pre>\n</div>",
    );
}

#[test]
fn indentation_does_not_inject_whitespace_before_endverbatim() {
    check(
        "\n<div>\n<section>\n@verbatim\n{{ raw }}\n@endverbatim\n</section>\n</div>",
        "\n<div>\n    <section>\n        @verbatim\n{{ raw }}\n@endverbatim\n    </section>\n</div>",
    );
}

#[test]
fn indentation_continues_indenting_siblings_after_a_preserved_block() {
    check(
        "\n<div>\n<pre>\ncode\n</pre>\n<p>After</p>\n@verbatim\n{{ raw }}\n@endverbatim\n<p>End</p>\n</div>",
        "\n<div>\n    <pre>\ncode\n</pre>\n    <p>After</p>\n    @verbatim\n{{ raw }}\n@endverbatim\n    <p>End</p>\n</div>",
    );
}

/// A `<script>` body is shifted as a block, not reindented by brace
/// depth: JavaScript is not the formatter's to lay out.
#[test]
fn indentation_shifts_content_inside_script_tags() {
    check(
        "\n<div>\n<script>\nfunction hello() {\n    console.log('hi');\n}\n</script>\n</div>",
        "\n<div>\n    <script>\n        function hello() {\n            console.log('hi');\n        }\n    </script>\n</div>",
    );
}

#[test]
fn indentation_shifts_content_inside_style_tags() {
    check(
        "\n<div>\n<style>\n.foo {\n    color: red;\n}\n</style>\n</div>",
        "\n<div>\n    <style>\n        .foo {\n            color: red;\n        }\n    </style>\n</div>",
    );
}

#[test]
fn indentation_leaves_directives_inside_script_tags_alone() {
    check(
        "\n<script>\n@if($debug)\nconsole.log(@json($state));\n@endif\n</script>\n<p>After</p>",
        "\n<script>\n    @if($debug)\n    console.log(@json($state));\n    @endif\n</script>\n<p>After</p>",
    );
}

#[test]
fn indentation_handles_single_line_preserved_blocks_without_entering_preserve_mode() {
    check(
        "\n<div>\n<pre>single line</pre>\n<p>After</p>\n</div>",
        "\n<div>\n    <pre>single line</pre>\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_shifts_multi_line_comments_as_a_block() {
    check(
        "\n<div>\n{{--\nA note\n    indented\n--}}\n<!--\nhtml note\n-->\n<p>After</p>\n</div>",
        "\n<div>\n    {{--\n        A note\n            indented\n    --}}\n    <!--\n        html note\n    -->\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_shifts_php_islands_as_a_block() {
    check(
        "\n<div>\n<?php\n$a = 1;\n    $b = 2;\n?>\n<p>After</p>\n</div>",
        "\n<div>\n    <?php\n        $a = 1;\n            $b = 2;\n    ?>\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_honours_disable_and_enable_comments() {
    check(
        "\n<div>\n{{-- blade-formatter-disable --}}\n<p>\n      as\n   written\n</p>\n{{-- blade-formatter-enable --}}\n<p>After</p>\n</div>",
        "\n<div>\n    {{-- blade-formatter-disable --}}\n<p>\n      as\n   written\n</p>\n{{-- blade-formatter-enable --}}\n    <p>After</p>\n</div>",
    );
}

// ── Blade expression delimiters ──────────────────────────────────

#[test]
fn indentation_does_not_count_double_curly_braces_as_brace_nesting() {
    check(
        "\n<div>\n<p>{{ $user->name }}</p>\n<p>{{ $user->email }}</p>\n</div>",
        "\n<div>\n    <p>{{ $user->name }}</p>\n    <p>{{ $user->email }}</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_count_unescaped_braces_as_brace_nesting() {
    check(
        "\n<div>\n<p>{!! $html !!}</p>\n<p>After</p>\n</div>",
        "\n<div>\n    <p>{!! $html !!}</p>\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_does_not_count_blade_comments_as_brace_nesting() {
    check(
        "\n<div>\n{{-- This is a comment --}}\n<p>After</p>\n</div>",
        "\n<div>\n    {{-- This is a comment --}}\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_handles_mixed_blade_expressions_and_real_braces() {
    check(
        "\n@props([\n'name' => '{{ $default }}',\n])",
        "\n@props([\n    'name' => '{{ $default }}',\n])",
    );
}

#[test]
fn indentation_treats_escaped_directives_and_echoes_as_text() {
    check(
        "\n<div>\n@@if($x)\n@{{ $raw }}\n<p>After</p>\n</div>",
        "\n<div>\n    @@if($x)\n    @{{ $raw }}\n    <p>After</p>\n</div>",
    );
}

// ── Inline tags in text content ─────────────────────────────────

#[test]
fn indentation_does_not_indent_for_inline_tags_that_wrap_in_text() {
    check(
        "\n<li>\nClick <x-ui>Settings</x-ui>, select the items\nyou want to update, and click <x-ui>Save\nChanges</x-ui>.\n</li>",
        "\n<li>\n    Click <x-ui>Settings</x-ui>, select the items\n    you want to update, and click <x-ui>Save\n    Changes</x-ui>.\n</li>",
    );
}

#[test]
fn indentation_does_not_indent_for_inline_elements_that_wrap() {
    check(
        "\n<p>\nHowever,\n<strong>admin users can override the default settings for\nthis section</strong>. This provides additional flexibility.\n</p>",
        "\n<p>\n    However,\n    <strong>admin users can override the default settings for\n    this section</strong>. This provides additional flexibility.\n</p>",
    );
}

// ── Dynamic tag names ───────────────────────────────────────────

#[test]
fn indentation_indents_content_inside_dynamic_tags() {
    check(
        "\n<{{ $as }}>\n{{ $slot }}\n</{{ $as }}>",
        "\n<{{ $as }}>\n    {{ $slot }}\n</{{ $as }}>",
    );
}

#[test]
fn indentation_indents_multi_line_dynamic_tag_with_attributes_class() {
    check(
        "\n<{{ $as }} {{ $attributes->class([\n'border-l-4 bg-gray-50',\n'mt-0' => $as === 'div',\n]) }}>\n{{ $slot }}\n</{{ $as }}>",
        "\n<{{ $as }} {{ $attributes->class([\n    'border-l-4 bg-gray-50',\n    'mt-0' => $as === 'div',\n]) }}>\n    {{ $slot }}\n</{{ $as }}>",
    );
}

#[test]
fn indentation_handles_single_line_dynamic_tags() {
    check(
        "\n<div>\n<{{ $as }}>Content</{{ $as }}>\n</div>",
        "\n<div>\n    <{{ $as }}>Content</{{ $as }}>\n</div>",
    );
}

#[test]
fn indentation_handles_self_closing_dynamic_tags() {
    check(
        "\n<div>\n<{{ $tag }} />\n<p>After</p>\n</div>",
        "\n<div>\n    <{{ $tag }} />\n    <p>After</p>\n</div>",
    );
}

#[test]
fn indentation_handles_dynamic_tags_with_simple_attributes() {
    check(
        "\n<{{ $as }}\nclass=\"foo\"\n>\n{{ $slot }}\n</{{ $as }}>",
        "\n<{{ $as }}\n    class=\"foo\"\n>\n    {{ $slot }}\n</{{ $as }}>",
    );
}

// ── Conditional wrapping (crossed HTML/Blade nesting) ───────────

#[test]
fn indentation_aligns_endif_with_if_when_html_tag_opens_between_them() {
    check(
        "\n@if ($wrap)\n<div>\n@endif\n<p>Content</p>\n@if ($wrap)\n</div>\n@endif",
        "\n@if ($wrap)\n    <div>\n@endif\n<p>Content</p>\n@if ($wrap)\n    </div>\n@endif",
    );
}

#[test]
fn indentation_aligns_endif_in_nested_conditional_wrap() {
    check(
        "\n@if ($outer)\n@if ($wrap)\n<div>\n@endif\n<p>Content</p>\n@if ($wrap)\n</div>\n@endif\n@endif",
        "\n@if ($outer)\n    @if ($wrap)\n        <div>\n    @endif\n    <p>Content</p>\n    @if ($wrap)\n        </div>\n    @endif\n@endif",
    );
}

#[test]
fn indentation_restores_level_after_conditional_wrap_and_tracks_forgotten_closing_tag() {
    check(
        "\n@if ($outer)\n@if ($wrap)\n<div>\n@endif\n<figure>\n<p>Content</p>\n</figure>\n@if ($wrap)\n</div>\n@endif\n@endif",
        "\n@if ($outer)\n    @if ($wrap)\n        <div>\n    @endif\n    <figure>\n        <p>Content</p>\n    </figure>\n    @if ($wrap)\n        </div>\n    @endif\n@endif",
    );
}

#[test]
fn indentation_keeps_a_directive_open_across_an_elements_closing_tag() {
    check(
        "\n<div>\n@if ($x)\n</div>\n@endif\n<p>After</p>",
        "\n<div>\n    @if ($x)\n    </div>\n@endif\n<p>After</p>",
    );
}

// ── No-indent elements ──────────────────────────────────────────

#[test]
fn indentation_does_not_indent_children_of_html_tag() {
    check(
        "\n<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Test</title>\n</head>\n<body>\n<div>\n<p>Hello</p>\n</div>\n</body>\n</html>",
        "\n<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n    <meta charset=\"utf-8\">\n    <title>Test</title>\n</head>\n<body>\n    <div>\n        <p>Hello</p>\n    </div>\n</body>\n</html>",
    );
}

// ── Nested ternary depth ────────────────────────────────────────

#[test]
fn indentation_indents_nested_ternary_operators_with_cumulative_depth() {
    check(
        "\n<div\n:class=\"a\n? 'x'\n: b\n? 'y'\n: 'z'\"\n></div>",
        "\n<div\n    :class=\"a\n        ? 'x'\n        : b\n            ? 'y'\n            : 'z'\"\n></div>",
    );
}

#[test]
fn indentation_resets_ternary_depth_on_non_ternary_lines() {
    check(
        "\n<div\n:class=\"a\n? 'x'\n: 'y'\"\nclass=\"test\"\n:other=\"c\n? 'p'\n: 'q'\"\n></div>",
        "\n<div\n    :class=\"a\n        ? 'x'\n        : 'y'\"\n    class=\"test\"\n    :other=\"c\n        ? 'p'\n        : 'q'\"\n></div>",
    );
}

// ── Component inline text-after-tag ─────────────────────────────

#[test]
fn indentation_does_not_indent_for_components_with_inline_text() {
    check(
        "\n<p>\n<x-nav-link id=\"test\">Learn more about\nthis feature</x-nav-link>.\nMore text here.\n</p>",
        "\n<p>\n    <x-nav-link id=\"test\">Learn more about\n    this feature</x-nav-link>.\n    More text here.\n</p>",
    );
}

// ── Logical operators are not continuation-indented ─────────────

#[test]
fn indentation_does_not_add_continuation_indent_for_logical_operators() {
    check(
        "\n@if (\n! $a\n&& ! $b\n|| $c\n)\n<p>Content</p>\n@endif",
        "\n@if (\n    ! $a\n    && ! $b\n    || $c\n)\n    <p>Content</p>\n@endif",
    );
}

// ── Single-line directives inside multi-line tags ───────────────

#[test]
fn indentation_does_not_increase_depth_for_single_line_directive_in_multi_line_tag() {
    check(
        "\n<x-dropdown\n{{ $attributes->class('group') }}\n@if ($open) expanded @endif\ndata-role=\"menu\"\n>\n<div>Content</div>\n</x-dropdown>",
        "\n<x-dropdown\n    {{ $attributes->class('group') }}\n    @if ($open) expanded @endif\n    data-role=\"menu\"\n>\n    <div>Content</div>\n</x-dropdown>",
    );
}

// ── Line endings and file endings ───────────────────────────────

#[test]
fn indentation_keeps_crlf_line_endings() {
    check(
        "<div>\r\n<p>Hello</p>\r\n\r\n</div>\r\n",
        "<div>\r\n    <p>Hello</p>\r\n\r\n</div>\r\n",
    );
}

#[test]
fn indentation_keeps_trailing_whitespace_unless_asked() {
    check(
        "<div>\n<p>Hello</p>   \n</div>",
        "<div>\n    <p>Hello</p>   \n</div>",
    );
    let options = Options {
        trim_trailing_whitespace: true,
        ..Options::default()
    };
    assert_eq!(
        reindent(
            "<div>\n<p>Hello</p>   \n<pre>\n x  \n</pre>\n</div>",
            &options
        ),
        "<div>\n    <p>Hello</p>\n    <pre>\n x  \n</pre>\n</div>"
    );
}

#[test]
fn indentation_applies_final_newline_options() {
    let insert = Options {
        insert_final_newline: true,
        ..Options::default()
    };
    assert_eq!(reindent("<div>\n</div>", &insert), "<div>\n</div>\n");
    assert_eq!(reindent("<div>\n</div>\n", &insert), "<div>\n</div>\n");
    assert_eq!(reindent("", &insert), "");
    let trim = Options {
        trim_final_newlines: true,
        ..Options::default()
    };
    assert_eq!(reindent("<div>\n</div>\n\n\n", &trim), "<div>\n</div>\n");
    assert_eq!(
        reindent("<div>\r\n</div>\r\n\r\n", &trim),
        "<div>\r\n</div>\r\n"
    );
}

#[test]
fn indentation_never_panics_on_malformed_input() {
    for input in [
        "<div",
        "<div class=\"unterminated",
        "@if (\n<p>",
        "{{ $x",
        "{{--",
        "@verbatim\n<p>",
        "@php\n$x = 1;",
        "<script>\nvar x;",
        "<pre>\n  x",
        "</div></div>@endif@endforeach)}]",
        "<{{ $tag",
        "<!--",
        "<?php echo 1;",
        "{{-- blade-formatter-disable --}}\n<p>\n  x",
        "@@\n@\n<\n</\n<//>",
    ] {
        let once = reindent(input, &Options::default());
        assert_eq!(reindent(&once, &Options::default()), once, "{input:?}");
    }
}

/// The Laravel example project's views, which are real templates rather
/// than cases written for the formatter.
fn example_views() -> Vec<(std::path::PathBuf, String)> {
    let mut views = Vec::new();
    let mut pending =
        vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/laravel/resources/views")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.to_string_lossy().ends_with(".blade.php") {
                let source = std::fs::read_to_string(&path).unwrap();
                views.push((path, source));
            }
        }
    }
    assert!(views.len() > 10, "only {} views found", views.len());
    views
}

/// The idempotence and leading-whitespace-only properties hold over real
/// templates.
#[test]
fn indentation_is_idempotent_over_the_example_views() {
    for (path, source) in example_views() {
        let once = reindent(&source, &Options::default());
        assert_eq!(
            reindent(&once, &Options::default()),
            once,
            "{} is not idempotent",
            path.display()
        );
        assert_eq!(source.lines().count(), once.lines().count());
        for (before, after) in source.lines().zip(once.lines()) {
            assert_eq!(
                before.trim_start_matches([' ', '\t']),
                after.trim_start_matches([' ', '\t']),
                "{} changed beyond leading whitespace",
                path.display()
            );
        }
    }
}

// ── Whitespace-sensitive templates ──────────────────────────────

#[test]
fn whitespace_sensitive_templates_are_recognised() {
    let plain = "<div>\n<p>x</p>\n</div>";
    assert!(is_whitespace_sensitive(
        Path::new("/app/Envoy.blade.php"),
        plain
    ));
    assert!(is_whitespace_sensitive(
        Path::new("/app/resources/views/mail/welcome.blade.php"),
        plain
    ));
    assert!(is_whitespace_sensitive(
        Path::new("/app/resources/views/emails/welcome.blade.php"),
        plain
    ));
    assert!(is_whitespace_sensitive(
        Path::new("/app/resources/views/vendor/mail/html/layout.blade.php"),
        plain
    ));
    assert!(is_whitespace_sensitive(
        Path::new("C:\\app\\resources\\boost\\guidelines\\core.blade.php"),
        plain
    ));
    assert!(is_whitespace_sensitive(
        Path::new("/app/resources/views/welcome.blade.php"),
        "<x-mail::message>\n# Hi\n</x-mail::message>"
    ));
    assert!(is_whitespace_sensitive(
        Path::new("/app/resources/views/welcome.blade.php"),
        "@component('mail::message')\n# Hi\n@endcomponent"
    ));
    assert!(!is_whitespace_sensitive(
        Path::new("/app/resources/views/welcome.blade.php"),
        plain
    ));
}

#[test]
fn builtin_strategy_skips_whitespace_sensitive_templates() {
    let content = "<div>\n<p>x</p>\n</div>";
    let config = FormattingConfig::default();
    let skipped = format_blade_content(
        &BladeFormattingStrategy::BuiltIn(None),
        content,
        Path::new("/app/resources/views/mail/welcome.blade.php"),
        None,
        &config,
        &Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(skipped.is_none());
    let formatted = format_blade_content(
        &BladeFormattingStrategy::BuiltIn(None),
        content,
        Path::new("/app/resources/views/welcome.blade.php"),
        None,
        &config,
        &Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(formatted.as_deref(), Some("<div>\n    <p>x</p>\n</div>"));
}

// ── Editor options ──────────────────────────────────────────────

#[test]
fn lsp_options_choose_the_indent_unit() {
    use tower_lsp::lsp_types::FormattingOptions;
    let spaces = options_from_lsp(&FormattingOptions {
        tab_size: 2,
        insert_spaces: true,
        ..FormattingOptions::default()
    });
    assert_eq!(spaces.indent, "  ");
    let tabs = options_from_lsp(&FormattingOptions {
        tab_size: 8,
        insert_spaces: false,
        ..FormattingOptions::default()
    });
    assert_eq!(tabs.indent, "\t");
    let unset = options_from_lsp(&FormattingOptions {
        tab_size: 0,
        insert_spaces: true,
        trim_trailing_whitespace: Some(true),
        ..FormattingOptions::default()
    });
    assert_eq!(unset.indent, "    ");
    assert!(unset.trim_trailing_whitespace);
}

// ── Strategy resolution ─────────────────────────────────────────

/// A workspace whose `composer.json` requires Pint and whose `vendor/bin`
/// holds a runnable `pint` script with `body` as its shell commands.
fn pint_workspace(body: &str) -> (tempfile::TempDir, crate::composer::ComposerPackage) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("vendor/bin")).unwrap();
    let pint = dir.path().join("vendor/bin/pint");
    std::fs::write(&pint, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pint, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": { "laravel/pint": "^1.30" }
    }))
    .unwrap();
    (dir, composer)
}

fn is_pint(strategy: &BladeFormattingStrategy, flag: bool) -> bool {
    matches!(strategy, BladeFormattingStrategy::Pint { flag: f, .. } if *f == flag)
}

#[test]
fn blade_strategy_is_builtin_without_pint() {
    let strategy = resolve_blade_strategy(
        None,
        &FormattingConfig::default(),
        None,
        None,
        PhpVersion::default(),
    );
    assert!(matches!(strategy, BladeFormattingStrategy::BuiltIn(_)));
}

#[test]
fn blade_strategy_is_disabled_when_formatting_is() {
    let config = FormattingConfig {
        pint: Some(String::new()),
        php_cs_fixer: Some(String::new()),
        phpcbf: Some(String::new()),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(None, &config, None, None, PhpVersion::default());
    assert!(matches!(strategy, BladeFormattingStrategy::Disabled));
}

#[test]
fn blade_strategy_follows_the_pint_json_rule() {
    let (dir, composer) = pint_workspace("cat");
    let config = FormattingConfig::default();

    // Pint is the PHP formatter, but its Blade rule is off, so Blade
    // files must not go to it: it would echo them back unchanged.
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(matches!(strategy, BladeFormattingStrategy::BuiltIn(_)));

    std::fs::write(
        dir.path().join("pint.json"),
        r#"{"preset": "laravel", "rules": {"Pint/laravel_blade": false}}"#,
    )
    .unwrap();
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(matches!(strategy, BladeFormattingStrategy::BuiltIn(_)));

    std::fs::write(
        dir.path().join("pint.json"),
        r#"{"preset": "laravel", "rules": {"Pint/laravel_blade": true}}"#,
    )
    .unwrap();
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(is_pint(&strategy, false), "{strategy:?}");

    // The rule takes an options object as well as a boolean.
    std::fs::write(
        dir.path().join("pint.json"),
        r#"{"rules": {"Pint/laravel_blade": {"tailwind": false}}}"#,
    )
    .unwrap();
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(is_pint(&strategy, false), "{strategy:?}");
}

#[test]
fn blade_strategy_honours_pint_blade_config() {
    let (dir, composer) = pint_workspace("cat");

    let on = FormattingConfig {
        pint_blade: Some(true),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &on,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(is_pint(&strategy, true), "{strategy:?}");

    std::fs::write(
        dir.path().join("pint.json"),
        r#"{"rules": {"Pint/laravel_blade": true}}"#,
    )
    .unwrap();
    let off = FormattingConfig {
        pint_blade: Some(false),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &off,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    assert!(matches!(strategy, BladeFormattingStrategy::BuiltIn(_)));

    // Asking for Pint's Blade rule without Pint changes nothing.
    let strategy = resolve_blade_strategy(None, &on, None, None, PhpVersion::default());
    assert!(matches!(strategy, BladeFormattingStrategy::BuiltIn(_)));
}

#[cfg(unix)]
#[test]
fn pint_runs_in_the_workspace_root_with_the_blade_flag() {
    // A stand-in Pint that reports where it ran and what it was asked.
    let (dir, composer) = pint_workspace("cat >/dev/null; echo \"cwd=$(pwd)\"; echo \"args=$*\"");
    let config = FormattingConfig {
        pint_blade: Some(true),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    let file = dir.path().join("resources/views/welcome.blade.php");
    let output = format_blade_content(
        &strategy,
        "<div>\n<p>x</p>\n</div>\n",
        &file,
        Some(dir.path()),
        &config,
        &Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert!(
        output.contains(&format!("cwd={}", root.display())),
        "{output}"
    );
    assert!(
        output.contains(&format!("args=--stdin-filename={} --blade", file.display())),
        "{output}"
    );
}

/// The same working directory is what lets Pint find `pint.json` for
/// plain PHP: the server's own directory is not the project's.
#[cfg(unix)]
#[test]
fn pint_formats_php_from_the_workspace_root() {
    let (dir, composer) = pint_workspace("cat >/dev/null; ls pint.json");
    std::fs::write(dir.path().join("pint.json"), "{}").unwrap();
    let config = FormattingConfig::default();
    let strategy = super::super::resolve_strategy(Some(dir.path()), &config, Some(&composer), None);
    let output = super::super::format_content(
        &strategy,
        "<?php\n",
        &dir.path().join("app/Model.php"),
        Some(dir.path()),
        &config,
        crate::types::PhpVersion::default(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(output.trim(), "pint.json");
}

#[cfg(unix)]
#[test]
fn pint_failure_is_an_error_not_an_edit() {
    let (dir, composer) =
        pint_workspace("cat >/dev/null; echo 'Node dependencies missing'; exit 1");
    let config = FormattingConfig {
        pint_blade: Some(true),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(
        Some(dir.path()),
        &config,
        Some(&composer),
        None,
        PhpVersion::default(),
    );
    let result = format_blade_content(
        &strategy,
        "<div></div>\n",
        &dir.path().join("a.blade.php"),
        Some(dir.path()),
        &config,
        &Options::default(),
        &AtomicBool::new(false),
    );
    assert!(result.is_err(), "{result:?}");
}

// ── Embedded PHP ────────────────────────────────────────────────

/// Format `input` with `blade-php = true`, the opt-in that lets the
/// built-in formatter rewrite the PHP a template carries, and check it
/// against `expected`. Formatting the result again has to be a no-op:
/// the PHP pass and the reindenter run one after the other, so a
/// disagreement between them would show up as an oscillation.
fn check_embedded(input: &str, expected: &str) {
    let actual = format_embedded(input);
    assert_eq!(
        actual, expected,
        "\n--- input ---\n{input}\n--- actual ---\n{actual}"
    );
    assert_eq!(
        format_embedded(&actual),
        actual,
        "formatting is not idempotent"
    );
}

/// Check that `input` comes back unchanged.
fn check_embedded_unchanged(input: &str) {
    check_embedded(input, input);
}

fn format_embedded(input: &str) -> String {
    let config = FormattingConfig {
        blade_php: Some(true),
        ..FormattingConfig::default()
    };
    let strategy = resolve_blade_strategy(None, &config, None, None, PhpVersion::default());
    assert!(
        matches!(strategy, BladeFormattingStrategy::BuiltIn(Some(_))),
        "blade-php = true should resolve the embedded-PHP pass: {strategy:?}"
    );
    format_blade_content(
        &strategy,
        input,
        Path::new("/app/resources/views/page.blade.php"),
        None,
        &config,
        &Options::default(),
        &AtomicBool::new(false),
    )
    .expect("formatting failed")
    .unwrap_or_else(|| input.to_string())
}

#[test]
fn embedded_php_is_off_unless_the_project_asks_for_it() {
    let input = "{{$name}}\n@if($a&&$b)\n<p>x</p>\n@endif\n";
    let formatted = format_blade_content(
        &BladeFormattingStrategy::BuiltIn(None),
        input,
        Path::new("/app/resources/views/page.blade.php"),
        None,
        &FormattingConfig::default(),
        &Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        formatted.as_deref(),
        Some("{{$name}}\n@if($a&&$b)\n    <p>x</p>\n@endif\n"),
        "only the indentation may change"
    );
}

#[test]
fn embedded_php_pads_an_echo() {
    check_embedded("{{$name}}\n", "{{ $name }}\n");
}

#[test]
fn embedded_php_formats_an_echo_expression() {
    check_embedded(
        "{{ $user->name ?? 'anonymous'}}\n",
        "{{ $user->name ?? 'anonymous' }}\n",
    );
}

#[test]
fn embedded_php_pads_a_raw_echo() {
    check_embedded("{!!$html!!}\n", "{!! $html !!}\n");
}

#[test]
fn embedded_php_formats_an_echo_inside_an_attribute_value() {
    check_embedded(
        "<div class=\"{{$class}}\"></div>\n",
        "<div class=\"{{ $class }}\"></div>\n",
    );
}

#[test]
fn embedded_php_spaces_a_control_directive_from_its_condition() {
    check_embedded(
        "@if($a&&$b)\n<p>x</p>\n@endif\n",
        "@if ($a && $b)\n    <p>x</p>\n@endif\n",
    );
}

#[test]
fn embedded_php_writes_a_call_directive_without_a_space() {
    check_embedded(
        "@include ('shared.errors')\n",
        "@include('shared.errors')\n",
    );
}

#[test]
fn embedded_php_formats_a_foreach_header() {
    check_embedded(
        "@foreach($users   as $user)\n<p>{{$user->name}}</p>\n@endforeach\n",
        "@foreach ($users as $user)\n    <p>{{ $user->name }}</p>\n@endforeach\n",
    );
}

#[test]
fn embedded_php_formats_a_for_header() {
    check_embedded(
        "@for($i=0;$i<3;$i++)\n@endfor\n",
        "@for ($i = 0; $i < 3; $i++)\n@endfor\n",
    );
}

#[test]
fn embedded_php_formats_an_argument_list() {
    check_embedded("@section('title','Home')\n", "@section('title', 'Home')\n");
}

#[test]
fn embedded_php_formats_a_php_block_and_the_reindenter_places_it() {
    check_embedded(
        "@if ($a)\n@php\n$total=1+2;\n  $label='x';\n@endphp\n@endif\n",
        "@if ($a)\n    @php\n        $total = 1 + 2;\n        $label = 'x';\n    @endphp\n@endif\n",
    );
}

#[test]
fn embedded_php_keeps_a_one_line_php_block_on_its_line() {
    check_embedded(
        "@php $total=1+2; @endphp\n",
        "@php $total = 1 + 2; @endphp\n",
    );
}

#[test]
fn embedded_php_formats_a_raw_php_island() {
    check_embedded(
        "<div>\n<?php\n$x=1;\n?>\n</div>\n",
        "<div>\n    <?php\n        $x = 1;\n    ?>\n</div>\n",
    );
}

#[test]
fn embedded_php_formats_a_short_echo_island() {
    check_embedded(
        "<div><?=$user->name?></div>\n",
        "<div><?= $user->name ?></div>\n",
    );
}

/// The `;` a `<?=` island may end with is optional, so it stays or goes
/// as the author wrote it.
#[test]
fn embedded_php_keeps_a_short_echo_semicolon() {
    check_embedded("<?=$a->b(1,2) ;?>\n", "<?= $a->b(1, 2); ?>\n");
    check_embedded("<?=$a->b(1,2)?>\n", "<?= $a->b(1, 2) ?>\n");
}

#[test]
fn embedded_php_leaves_an_empty_short_echo_alone() {
    check_embedded_unchanged("<?= ?>\n");
}

/// Whether a short `<?` tag opens PHP at all depends on
/// `short_open_tag`, so the pass does not read what follows it as PHP.
#[test]
fn embedded_php_leaves_a_short_open_tag_alone() {
    check_embedded_unchanged("<? echo $x ; ?>\n");
}

/// A fragment the author broke over lines is re-spaced and stays
/// broken. Which lines it takes within that is mago's, and mago keeps
/// an array the author broke broken.
#[test]
fn embedded_php_formats_a_broken_props_array_without_joining_it() {
    check_embedded(
        "<div>\n@props([\n'on',\n'color'=>'blue',\n])\n</div>\n",
        "<div>\n    @props([\n        'on',\n        'color' => 'blue',\n    ])\n</div>\n",
    );
}

#[test]
fn embedded_php_formats_a_broken_echo_argument_list() {
    check_embedded(
        "<p>\n{{ __('messages.welcome',[\n'name'=>$name,\n]) }}\n</p>\n",
        "<p>\n    {{ __('messages.welcome', [\n        'name' => $name,\n    ]) }}\n</p>\n",
    );
}

/// A break the author put right after the opening delimiter and right
/// before the closing one is the pass's own padding, not part of the
/// fragment: `{{\n$x\n}}` is the one-line echo `{{ $x }}`.
#[test]
fn embedded_php_joins_a_fragment_only_its_delimiters_broke() {
    check_embedded("{{ $x\n}}\n", "{{ $x }}\n");
}

/// The column a broken fragment sits at reaches the formatter, so what
/// it wraps to still fits once the reindenter has put it back one level
/// in from the line it opened on.
#[test]
fn embedded_php_wraps_a_broken_fragment_for_the_column_it_sits_at() {
    let deep = "<div>\n".repeat(5);
    let close = "</div>\n".repeat(5);
    let echo = "{{ __('key', [\n'first' => $someQuiteLongVariableName, 'second' => $anotherQuiteLongName,\n]) }}\n";
    let formatted = format_embedded(&format!("{deep}{echo}{close}"));
    let widest = formatted.lines().map(str::len).max().unwrap_or(0);
    assert!(
        widest <= FormatSettings::default().print_width,
        "a line ran past the print width:\n{formatted}"
    );
    assert!(
        formatted.contains("'first' => $someQuiteLongVariableName,\n"),
        "the argument list should have wrapped:\n{formatted}"
    );
}

#[test]
fn embedded_php_spaces_a_self_closing_tag() {
    check_embedded("<br/>\n<x-alert   />\n", "<br />\n<x-alert />\n");
}

#[test]
fn embedded_php_leaves_a_fragment_that_does_not_parse_alone() {
    check_embedded(
        "{{ $a ** }}\n@if($ok)\n<p>x</p>\n@endif\n",
        "{{ $a ** }}\n@if ($ok)\n    <p>x</p>\n@endif\n",
    );
}

/// A broken fragment mago hands back on one line is left as written
/// rather than joined: the pass owns the spacing inside a fragment, not
/// how many lines it takes. mago joins a broken argument list by
/// default, so this echo is never rewritten.
#[test]
fn embedded_php_leaves_a_multi_line_echo_alone() {
    check_embedded_unchanged("{{ $items->map(\n    fn ($i) => $i->name\n) }}\n");
}

/// The same rule the other way round: an overlong fragment the author
/// wrote on one line is left there rather than broken.
#[test]
fn embedded_php_leaves_a_long_one_line_echo_on_its_line() {
    check_embedded_unchanged(
        "{{ __('a.very.long.translation.key.that.will.not.fit', ['first' => $first, 'second' => $second]) }}\n",
    );
}

#[test]
fn embedded_php_leaves_verbatim_alone() {
    check_embedded_unchanged("@verbatim\n{{$vue}}\n@endverbatim\n");
}

/// Only the reindenter's own block shift applies to a `<script>` or
/// `<style>` body: the echo inside it keeps the spacing it was written
/// with.
#[test]
fn embedded_php_leaves_script_and_style_bodies_alone() {
    check_embedded(
        "<script>\nlet x = {{$n}};\n</script>\n<style>\n.a{color:red}\n</style>\n",
        "<script>\n    let x = {{$n}};\n</script>\n<style>\n    .a{color:red}\n</style>\n",
    );
}

#[test]
fn embedded_php_leaves_pre_bodies_alone() {
    check_embedded_unchanged("<pre>\n{{$raw}}\n</pre>\n");
}

#[test]
fn embedded_php_leaves_a_lookalike_in_a_comment_alone() {
    check_embedded_unchanged("{{-- {{$name}} --}}\n");
}

#[test]
fn embedded_php_leaves_a_disabled_region_alone() {
    check_embedded_unchanged(
        "{{-- blade-formatter-disable --}}\n{{$name}}\n{{-- blade-formatter-enable --}}\n",
    );
}

#[test]
fn embedded_php_leaves_an_escaped_echo_alone() {
    check_embedded_unchanged("@{{$vue}}\n");
}

/// Blade closes a `@php` block with a non-greedy regex, so an `@endphp`
/// written inside a string literal really does end it. The pass reads it
/// the same way the reindenter and the compiler do, which leaves a body
/// that no longer parses, and an unparseable body is left as written.
#[test]
fn embedded_php_reads_a_delimiter_in_a_string_the_way_blade_does() {
    check_embedded(
        "@php\n$x = '@endphp';\n$y = 2;\n@endphp\n",
        "@php\n    $x = '@endphp';\n$y = 2;\n@endphp\n",
    );
}

#[test]
fn embedded_php_leaves_an_alpine_attribute_alone() {
    check_embedded_unchanged("<div x-data=\"{ open:false }\" @click=\"open=!open\"></div>\n");
}

#[test]
fn embedded_php_leaves_a_livewire_attribute_alone() {
    check_embedded_unchanged("<input wire:model.live=\"search\" />\n");
}

#[test]
fn embedded_php_leaves_an_alpine_bind_on_a_plain_element_alone() {
    check_embedded_unchanged("<div :class=\"open?'a':'b'\"></div>\n");
}

/// The pass and the reindenter run one after the other, so a template
/// that exercises both has to reach a fixed point.
#[test]
fn embedded_php_is_idempotent_over_a_whole_template() {
    let template = concat!(
        "@extends('layouts.app')\n",
        "@section('content')\n",
        "@php\n",
        "$items=collect([1,2,3]);\n",
        "@endphp\n",
        "<div class=\"wrap\" x-data=\"{ open: false }\">\n",
        "@forelse($items as $item)\n",
        "<x-row :item=\"$item\"/>\n",
        "{{-- a comment --}}\n",
        "<p>{{$item}}</p>\n",
        "@empty\n",
        "<p>@lang('none')</p>\n",
        "@endforelse\n",
        "@switch($mode)\n",
        "@case('a')\n",
        "@break\n",
        "@endswitch\n",
        "</div>\n",
        "@endsection\n",
    );
    let once = format_embedded(template);
    assert_eq!(format_embedded(&once), once, "\n--- once ---\n{once}");
}

/// The pass reaches a fixed point on real templates too, not only on the
/// cases written for it.
#[test]
fn embedded_php_is_idempotent_over_the_example_views() {
    for (path, source) in example_views() {
        let once = format_embedded(&source);
        assert_eq!(
            format_embedded(&once),
            once,
            "{} is not idempotent",
            path.display()
        );
    }
}

/// The same malformed templates the reindenter's own fixed-point test
/// uses, which are the shapes a scanner is most likely to run off the end
/// of.
#[test]
fn embedded_php_is_idempotent_over_malformed_templates() {
    for input in [
        "",
        "@if",
        "@if(",
        "{{",
        "{{ $a",
        "{!!",
        "@php",
        "@php $x =",
        "@verbatim",
        "<div",
        "<div attr=\"",
        "<script>",
        "<!--",
        "<?php echo 1;",
        "{{-- blade-formatter-disable --}}\n<p>\n  x",
        "@@\n@\n<\n</\n<//>",
    ] {
        let once = format_embedded(input);
        assert_eq!(format_embedded(&once), once, "{input:?}");
    }
}

/// A snippet is formatted on its own, so the print width has to be told
/// what column the reindenter will put it back at. Otherwise a deeply
/// nested `@php` block comes back filled to 120 columns and is then
/// pushed past the end of the line.
#[test]
fn embedded_php_formats_a_php_block_to_the_width_left_at_its_column() {
    // 107 columns: it fits at the top level, where the body starts at
    // column 4, and not three blocks in, where it starts at 16.
    let statement = "$url = $language->url . \\App\\Helpers\\Slug::translate('/page', $langCode) . '/' . $page->getSlugForLocale();";
    check_embedded(
        &format!("@php\n{statement}\n@endphp\n"),
        &format!("@php\n    {statement}\n@endphp\n"),
    );
    check_embedded(
        &format!(
            "@if ($a)\n@if ($b)\n@if ($c)\n@php\n{statement}\n@endphp\n@endif\n@endif\n@endif\n"
        ),
        concat!(
            "@if ($a)\n",
            "    @if ($b)\n",
            "        @if ($c)\n",
            "            @php\n",
            "                $url =\n",
            "                    $language->url . \\App\\Helpers\\Slug::translate('/page', $langCode) . '/' . $page->getSlugForLocale();\n",
            "            @endphp\n",
            "        @endif\n",
            "    @endif\n",
            "@endif\n",
        ),
    );
}
