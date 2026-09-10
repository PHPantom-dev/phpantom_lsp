use super::*;

/// `<?xml ... ?>` is never a PHP open tag regardless of
/// `short_open_tag`; PHP special-cases it so XML declarations and
/// feeds embedded in templates aren't misparsed as PHP.
#[test]
fn test_preprocess_xml_declaration_is_not_a_php_tag() {
    let content = "<?xml version=\"1.0\" ?>\n<users>\n    <user>{{ $user }}</user>\n</users>\n";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("version"),
        "<?xml ...?> should be masked as HTML, not parsed as PHP: {}",
        php
    );
    assert!(
        php.contains("echo e( $user )"),
        "{{ $user }} after the XML declaration should still translate normally: {}",
        php
    );
}

#[test]
fn test_preprocess_directive_with_string_parens() {
    let content = "@if(str_contains($val, \")\"))\n    {{ $val }}\n@endif";
    let (php, _) = preprocess(content);
    // It should properly wait for the outer parenthesis to close
    assert!(
        php.contains(" if (str_contains($val, \")\")):"),
        "Failed to parse parens inside string: {}",
        php
    );
}

#[test]
fn test_preprocess_foreach_loop_variable() {
    let content = "@foreach($items as $item)\n{{ $loop->first }}\n@endforeach\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$loop"),
        "should inject $loop variable: {}",
        php
    );
    assert!(
        php.contains("object{index: int"),
        "should have typed $loop: {}",
        php
    );
    // $loop should be declared before its usage
    let loop_decl = php.find("$loop = (object)[];").unwrap();
    let loop_use = php.rfind("$loop").unwrap();
    assert!(
        loop_use > loop_decl,
        "$loop usage after declaration: {}",
        php
    );
}

#[test]
fn test_preprocess_errors_bag_visible_inside_template_function() {
    let content = "{{ $errors->has('name') }}";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("function __blade_template() { global $errors, $__env;"),
        "$errors/$__env must be pulled into the wrapper function's scope: {}",
        php
    );
}

/// A template that renders with a component instance bound wraps its
/// body in a method of a subclass of that component, which is the only
/// way `$this` can carry a type: PHP allows neither `$this = …` nor
/// `global $this`.
#[test]
fn test_a_bound_this_wraps_the_body_in_a_subclass_method() {
    let (php, map) = preprocess_with_vars(
        "{{ $this->count }}",
        &[],
        TemplateKind::View,
        Some("App\\Livewire\\Counter"),
        None,
        &Default::default(),
    );
    assert!(
        php.contains(
            "abstract class __blade_scope_App_Livewire_Counter \
             extends \\App\\Livewire\\Counter \
             { public function __blade_template() { global $errors, $__env;"
        ),
        "the body must sit in a method of a subclass of the component: {}",
        php
    );
    assert!(
        php.trim_end().ends_with("} }"),
        "the method and the class holding it must both close: {}",
        php
    );
    // The wrapper still occupies exactly one prologue line, so Blade
    // positions map the same as they do without a bound `$this`.
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES);
}

#[test]
fn test_component_prologue_declares_attributes_and_slot() {
    let (php, map) = preprocess_with_vars(
        "<img {{ $attributes->merge(['class' => 'x']) }} />{{ $slot }}",
        &[],
        TemplateKind::Component,
        None,
        None,
        &Default::default(),
    );
    assert!(
        php.contains("/** @var \\Illuminate\\View\\ComponentAttributeBag $attributes */")
            && php.contains("/** @var \\Illuminate\\View\\ComponentSlot $slot */"),
        "component variables must be declared with their framework types: {}",
        php
    );
    assert!(
        php.contains("/** @var string $componentName */"),
        "a component also knows its own name: {}",
        php
    );
    assert!(
        php.contains(
            "function __blade_template() { global $errors, $__env, $attributes, $slot, $componentName;"
        ),
        "component variables must be pulled into the wrapper scope: {}",
        php
    );
    // Three declarations of two lines each on top of the base prologue.
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 6);
}

#[test]
fn test_plain_view_prologue_has_no_component_variables() {
    let (php, _) = preprocess("{{ $slot }}");
    assert!(
        !php.contains("$attributes = new") && !php.contains("$slot = new"),
        "a plain view must not receive component variables: {}",
        php
    );
}

/// A caller cannot pass `$attributes`, so a call-site inference that
/// produced one must not overwrite the framework's own declaration.
#[test]
fn test_component_variables_are_not_overwritten_by_inferred_vars() {
    let (php, map) = preprocess_with_vars(
        "{{ $attributes }}",
        &[("attributes".to_string(), "string".to_string())],
        TemplateKind::Component,
        None,
        None,
        &Default::default(),
    );
    assert!(
        !php.contains("$attributes = null;"),
        "the inferred declaration must be dropped: {}",
        php
    );
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 6);
}

#[test]
fn test_preprocess_with_vars_injects_declarations() {
    let content = "{{ $user->name }}";
    let (php, map) = preprocess_with_vars(
        content,
        &[
            ("results".to_string(), "array<int, string>".to_string()),
            ("user".to_string(), "\\App\\Models\\User".to_string()),
        ],
        TemplateKind::View,
        None,
        None,
        &Default::default(),
    );
    assert!(
        php.contains("/** @var array<int, string> $results */"),
        "injected @var declaration missing: {}",
        php
    );
    assert!(
        php.contains("/** @var \\App\\Models\\User $user */"),
        "injected @var declaration missing: {}",
        php
    );
    assert!(
        php.contains("function __blade_template() { global $errors, $__env, $results, $user;"),
        "injected variables must be pulled into the wrapper scope: {}",
        php
    );
    // Each injected variable adds a @var line and an assignment line.
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 4);

    // Round trip: blade (0,0) → php and back lands on the same line.
    let php_pos = map.blade_to_php(tower_lsp::lsp_types::Position {
        line: 0,
        character: 3,
    });
    assert_eq!(php_pos.line, map.prologue_lines);
    let back = map.php_to_blade(php_pos);
    assert_eq!(back.line, 0);
}

#[test]
fn test_preprocess_without_vars_keeps_default_prologue() {
    let (_, map) = preprocess("{{ $x }}");
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES);
}

/// A literal-string type keeps its source form, and PHP allows a real
/// line break inside a quoted string — so an inferred type can arrive
/// with a newline in it. It must not add a prologue line (that would
/// shift every position in the template) nor leave the `@var` docblock
/// straddling two lines.
#[test]
fn test_preprocess_with_vars_multiline_type_does_not_shift_positions() {
    let (php, map) = preprocess_with_vars(
        "{{ $body }}",
        &[("body".to_string(), "'line1\nline2'".to_string())],
        TemplateKind::View,
        None,
        None,
        &Default::default(),
    );
    assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 2);
    assert!(
        php.contains("/** @var mixed $body */"),
        "a multi-line type must degrade to mixed: {}",
        php
    );
    // The template body still starts exactly at the prologue height.
    let php_lines: Vec<&str> = php.lines().collect();
    assert!(
        php_lines[map.prologue_lines as usize].contains("$body"),
        "template line 0 must sit at prologue_lines: {}",
        php
    );
}

/// A `*/` inside an inferred type would close the docblock early and
/// spill the remainder into code.
#[test]
fn test_preprocess_with_vars_type_cannot_close_the_docblock() {
    let (php, _) = preprocess_with_vars(
        "{{ $x }}",
        &[("x".to_string(), "'*/ evil()'".to_string())],
        TemplateKind::View,
        None,
        None,
        &Default::default(),
    );
    assert!(
        php.contains("/** @var mixed $x */") && !php.contains("evil()"),
        "a type containing */ must degrade to mixed: {}",
        php
    );
}

/// A component tag's attribute names are HTML, so a caller writing
/// `wire:model.live="…"` or `@click="…"` offers a name PHP cannot bind.
/// Blade's `extract()` skips those keys, and so must the prologue:
/// emitting `$wire:model.live = null;` would be a syntax error that
/// takes the whole template down with it.
#[test]
fn test_preprocess_with_vars_skips_names_php_cannot_bind() {
    let (php, _) = preprocess_with_vars(
        "{{ $ok }}",
        &[
            ("wire:model.live".to_string(), "string".to_string()),
            ("@click".to_string(), "string".to_string()),
            ("ok".to_string(), "string".to_string()),
        ],
        TemplateKind::Component,
        None,
        None,
        &Default::default(),
    );
    assert!(
        !php.contains("wire:model.live") && !php.contains("@click"),
        "an attribute name that is not a PHP variable must not be declared: {}",
        php
    );
    assert!(
        php.contains("$ok = null;") && php.contains(", $ok;"),
        "a valid name alongside it must still be declared: {}",
        php
    );
}

/// Inline attribute directives (`@class`, `@style`, `@checked`,
/// `@selected`, `@disabled`, `@readonly`, `@required`) must consume
/// their own argument list and return to HTML mode, not fall into the
/// generic directive branch (which leaves everything after them
/// parsed as PHP for the rest of the template).
#[test]
fn test_preprocess_attribute_directives_return_to_html() {
    let content = r#"<div @class(['a', 'b' => $cond]) id="x"></div>"#;
    let (php, _) = preprocess(content);
    // HTML content is masked with spaces (it is not meant to be parsed
    // as PHP), so the literal `id="x"` markup must NOT survive as raw
    // PHP source after the directive — that was the bug: the
    // generic-directive fallback left the parser in PHP mode for the
    // rest of the template, so `id="x"></div>` leaked through
    // unmasked and caused cascading syntax errors.
    assert!(
        !php.contains(r#"id="x""#),
        "content after @class(...) should be masked as HTML, not left as raw PHP: {}",
        php
    );
    assert!(
        php.contains("blade_directive (['a', 'b' => $cond]);"),
        "unexpected @class(...) translation: {}",
        php
    );
}

/// `@stack('name')` (render a named stack) must consume its own
/// argument list and return to HTML mode, like `@yield`/`@section`,
/// instead of falling into the generic directive branch.
#[test]
fn test_preprocess_stack_directive_returns_to_html() {
    let content = r#"<div>@stack('scripts')</div><p>after</p>"#;
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("after"),
        "content after @stack(...) should be masked as HTML, not left as raw PHP: {}",
        php
    );
    assert!(
        php.contains("blade_stack_directive ('scripts');"),
        "unexpected @stack(...) translation: {}",
        php
    );
}

/// `@json($var)` must consume its argument as a real expression so a
/// variable used only inside it is not silently invisible to the
/// forward walker (it previously fell outside `match_directive`
/// entirely, so `$var` in `@json($var)` was never emitted as PHP and
/// the variable was reported as unused).
#[test]
fn test_preprocess_json_directive_consumes_argument() {
    let content = r#"<script>window.foo = @json($value);</script><p>after</p>"#;
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("after"),
        "content after @json(...) should be masked as HTML, not left as raw PHP: {}",
        php
    );
    assert!(
        php.contains("blade_directive ($value);"),
        "unexpected @json(...) translation: {}",
        php
    );
}

/// `@dump($var)` must likewise consume its argument as a real
/// expression, for the same reason as `@json` above.
#[test]
fn test_preprocess_dump_directive_consumes_argument() {
    let content = r#"<div>@dump($value)</div><p>after</p>"#;
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("after"),
        "content after @dump(...) should be masked as HTML, not left as raw PHP: {}",
        php
    );
    assert!(
        php.contains("blade_directive ($value);"),
        "unexpected @dump(...) translation: {}",
        php
    );
}

/// The directives a project registered, as the provider scan would have
/// recorded them.
fn registered(names: &[(&str, bool)]) -> CustomDirectives {
    let registrations: Vec<super::super::directives::CustomDirective> = names
        .iter()
        .map(
            |(name, conditional)| super::super::directives::CustomDirective {
                name: name.to_string(),
                conditional: *conditional,
            },
        )
        .collect();
    CustomDirectives::from_registrations(&registrations)
}

/// The wrapped template body of a preprocessed template, without the
/// prologue — whose marker declarations would otherwise answer a search
/// for a marker the body never calls.
fn preprocess_with_directives(content: &str, directives: &CustomDirectives) -> String {
    let (php, _) = preprocess_with_vars(content, &[], TemplateKind::View, None, None, directives);
    let body_start = php
        .find("global $errors")
        .expect("wrapper function prologue");
    php[body_start..].to_string()
}

/// A `Blade::directive()` registration is a statement whose argument the
/// template still gets type-checked on, rather than the comment an
/// unregistered `@name` degrades to.
#[test]
fn a_registered_directive_keeps_its_argument_as_real_php() {
    let php = preprocess_with_directives(
        "<p>@datetime($post->createdAt)</p>",
        &registered(&[("datetime", false)]),
    );
    assert!(
        php.contains("blade_custom_directive ($post->createdAt);"),
        "unexpected @datetime translation: {php}"
    );
}

/// Blade hands a handler an empty expression when the template writes no
/// argument list, so a bare name must complete on the spot instead of
/// scanning ahead for a closing paren that was never opened.
#[test]
fn a_registered_directive_without_arguments_stands_alone() {
    let php = preprocess_with_directives(
        "<p>@datetime</p>{{ $after }}",
        &registered(&[("datetime", false)]),
    );
    assert!(
        php.contains("blade_custom_directive();"),
        "unexpected bare @datetime translation: {php}"
    );
    assert!(
        php.contains("echo e( $after"),
        "the rest of the template must still be scanned as Blade: {php}"
    );
}

/// `Blade::if('admin')` gives the template four directives, and the
/// three that are not the `@end` open a real condition so the `@endadmin`
/// closing them balances.
#[test]
fn a_registered_condition_opens_a_real_if() {
    let php = preprocess_with_directives(
        "@admin('editor')\n<p>yes</p>\n@elseadmin('viewer')\n<p>maybe</p>\n@endadmin\n<p>after</p>",
        &registered(&[("admin", true)]),
    );
    assert!(
        php.contains("if (blade_custom_directive ('editor')):"),
        "@admin should open a balanced if: {php}"
    );
    assert!(
        php.contains("elseif (blade_custom_directive ('viewer')):"),
        "@elseadmin should open a balanced elseif: {php}"
    );
    assert!(
        php.contains("endif;"),
        "@endadmin should close what @admin opened: {php}"
    );
}

/// The argument list is optional for every member of the family, and
/// `@unlessadmin` is closed by the same `@endadmin` as `@admin`.
#[test]
fn a_registered_condition_without_arguments_is_still_balanced() {
    let php = preprocess_with_directives(
        "@unlessadmin\n<p>no</p>\n@endadmin\n",
        &registered(&[("admin", true)]),
    );
    assert!(
        php.contains("if (blade_custom_directive()):") && php.contains("endif;"),
        "a bare @unlessadmin must open and close a real if: {php}"
    );
}

/// Nothing was registered, so the directive is still what it always was:
/// inert markup, with its argument not read as PHP at all.
#[test]
fn an_unregistered_directive_stays_masked() {
    let php = preprocess_with_directives("<p>@datetime($x)</p>", &CustomDirectives::default());
    assert!(
        !php.contains("blade_custom_directive") && !php.contains("$x"),
        "an unregistered directive must stay masked: {php}"
    );
}

/// Blade's compiler consults its custom table before its own directives,
/// but a registration shadowing a core name would break the block
/// structure of every template that writes it, so the core table wins.
#[test]
fn a_registration_does_not_shadow_a_core_directive() {
    let php = preprocess_with_directives(
        "@if ($ok)\n<p>hi</p>\n@endif\n",
        &registered(&[("if", false), ("endif", false)]),
    );
    assert!(
        php.contains("($ok):") && !php.contains("blade_custom_directive"),
        "@if must still compile as Blade's own directive: {php}"
    );
}

/// `@can`/`@cannot`/`@canany` (and their `@elsecan*` counterparts) open a
/// real `if`/`elseif` so the always-literal `@endif` that closes them
/// stays balanced, while their arguments are still type-checked.
#[test]
fn test_preprocess_can_directive_opens_a_real_if() {
    let content = "@can('update', $post)\n<p>can</p>\n@elsecan('view', $post)\n<p>view</p>\n@endcan\n<p>after</p>";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (blade_can_directive ('update', $post)):"),
        "@can should open a balanced if with its arguments type-checked: {}",
        php
    );
    assert!(
        php.contains("elseif (blade_can_directive ('view', $post)):"),
        "@elsecan should open a balanced elseif with its arguments type-checked: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endcan should close the if opened by @can: {}",
        php
    );
    assert!(
        !php.contains("after"),
        "content after @endcan should be masked as HTML, not left as raw PHP: {}",
        php
    );
}

/// `@hasStack`/`@hasSection`/`@sectionMissing` are always closed by a
/// literal `@endif`, not a dedicated end-directive, so they must open a
/// real `if` too (previously they degraded to a bare comment, leaving
/// `@endif` dangling with no matching `if` and breaking the rest of the
/// virtual PHP file).
#[test]
fn test_preprocess_has_stack_and_has_section_open_a_real_if() {
    let (php, _) = preprocess("@hasStack('scripts')\nx\n@endif\n<p>after</p>");
    assert!(
        php.contains("if (blade_stack_directive ('scripts')):"),
        "@hasStack should open a balanced if with its argument type-checked: {}",
        php
    );
    assert!(
        !php.contains("after"),
        "content after @endif should be masked as HTML, not left as raw PHP: {}",
        php
    );

    let (php, _) = preprocess("@hasSection('content')\nx\n@endif\n<p>after</p>");
    assert!(
        php.contains("if (blade_section_directive ('content')):"),
        "@hasSection should open a balanced if with its argument type-checked: {}",
        php
    );

    let (php, _) = preprocess("@sectionMissing('content')\nx\n@endif\n<p>after</p>");
    assert!(
        php.contains("if (blade_section_directive ('content')):"),
        "@sectionMissing should open a balanced if with its argument type-checked: {}",
        php
    );
}

/// `@pushIf`/`@pushOnce`/`@prependOnce`/`@hasStack` used to fall through
/// to `translate_directive`'s default `/* @directive */` comment, which
/// left their arguments completely untyped.
#[test]
fn test_preprocess_push_if_and_push_once_consume_arguments() {
    let (php, _) = preprocess("@pushIf($condition, 'scripts')\nx\n@endPushIf\n<p>after</p>");
    assert!(
        php.contains("blade_push_if_directive ($condition, 'scripts');"),
        "@pushIf should type-check its arguments: {}",
        php
    );

    let (php, _) = preprocess("@pushOnce('scripts')\nx\n@endPushOnce\n<p>after</p>");
    assert!(
        php.contains("blade_stack_directive ('scripts');"),
        "@pushOnce should type-check its argument: {}",
        php
    );

    let (php, _) = preprocess("@prependOnce('scripts')\nx\n@endPrependOnce\n<p>after</p>");
    assert!(
        php.contains("blade_stack_directive ('scripts');"),
        "@prependOnce should type-check its argument: {}",
        php
    );
}

/// `@lang` is optional-argument: bare it opens a translation-buffering
/// block paired with `@endlang` (nothing to type-check), and with an
/// argument it is a one-shot call whose expression should be checked.
#[test]
fn test_preprocess_lang_directive_optional_argument() {
    let (php, _) = preprocess("@lang\n<p>x</p>\n@endlang\n<p>after</p>");
    assert!(
        !php.contains("after"),
        "bare @lang/@endlang should not swallow the rest of the template into raw PHP: {}",
        php
    );

    let (php, _) = preprocess("@lang($key)\n<p>after</p>");
    assert!(
        php.contains("blade_directive ($key);"),
        "@lang(...) should type-check its argument: {}",
        php
    );
}

/// `@vite`/`@fonts` take an optional argument list; a bare `@vite` must
/// not send the scanner hunting for a closing paren that was never
/// opened, which would swallow the rest of the template.
#[test]
fn test_preprocess_vite_and_fonts_optional_argument() {
    let (php, _) = preprocess("@vite\n<p>after</p>");
    assert!(
        !php.contains("after"),
        "bare @vite should not swallow the rest of the template into raw PHP: {}",
        php
    );

    let (php, _) = preprocess("@vite(['resources/js/app.js'])\n<p>after</p>");
    assert!(
        php.contains("blade_directive (['resources/js/app.js']);"),
        "@vite(...) should type-check its argument: {}",
        php
    );

    let (php, _) = preprocess("@fonts\n<p>after</p>");
    assert!(
        !php.contains("after"),
        "bare @fonts should not swallow the rest of the template into raw PHP: {}",
        php
    );
}

/// `@unset($var)` must compile to a real `unset(...)` statement, not a
/// `blade_directive(...)` call — `unset` is a language construct and
/// cannot be used as a function-call argument.
#[test]
fn test_preprocess_unset_directive() {
    let (php, _) = preprocess("@unset($value)\n<p>after</p>");
    assert!(
        php.contains("unset ($value);"),
        "@unset should compile to a real unset() statement: {}",
        php
    );
}

/// `@choice`/`@js`/`@dd`, previously unrecognised entirely (masked as
/// inert HTML), must type-check their arguments like other expression
/// directives.
#[test]
fn test_preprocess_choice_js_dd_directives_consume_arguments() {
    let (php, _) = preprocess("@choice('apples', $count)\n<p>after</p>");
    assert!(
        php.contains("blade_directive ('apples', $count);"),
        "@choice should type-check its arguments: {}",
        php
    );

    let (php, _) = preprocess("@js($data)\n<p>after</p>");
    assert!(
        php.contains("blade_directive ($data);"),
        "@js should type-check its argument: {}",
        php
    );

    let (php, _) = preprocess("@dd($value)\n<p>after</p>");
    assert!(
        php.contains("blade_directive ($value);"),
        "@dd should type-check its argument: {}",
        php
    );
}

/// A bound attribute on a component tag (`:src="$image"`) must emit
/// its expression as real PHP so the variable is seen by the forward
/// walker (otherwise a variable used only there is a false-positive
/// "unused variable"). The surrounding tag markup stays masked.
#[test]
fn test_preprocess_bound_attribute_emits_expression() {
    let content = r#"<x-img.size :src="$image" alt="x" />"#;
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($image);"),
        "bound attribute expression should be emitted as PHP: {}",
        php
    );
    // A tag no component index resolves becomes a comment naming it,
    // never executable PHP.
    assert!(
        php.contains("/* x-img.size */"),
        "an unresolved tag should degrade to a comment: {}",
        php
    );
    assert!(
        !php.contains(r#"alt="x""#),
        "unbound attribute markup should stay masked: {}",
        php
    );
}

/// Package tag namespaces (`<livewire:...>`) and method-call
/// expressions inside the binding must work the same way.
#[test]
fn test_preprocess_bound_attribute_livewire_and_method_call() {
    let content = r#"<livewire:edit-channel :key="$item->id" />"#;
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($item->id);"),
        "method-call expression in a bound attribute should be emitted: {}",
        php
    );
    // The `:` inside the `livewire:edit-channel` tag name is part of
    // the name, not an attribute, so it must not open a directive call.
    assert!(
        !php.contains("blade_bound_attr_directive(edit-channel"),
        "namespace colon in the tag name must not be treated as a binding: {}",
        php
    );
}

/// A resolver over a fixed table, standing in for the project's
/// discovery index.
struct TestComponents(Vec<(String, ComponentTarget)>);

impl ComponentResolver for TestComponents {
    fn x_component(&self, tag: &str) -> Option<ComponentTarget> {
        self.lookup(&format!("x-{tag}"))
    }

    fn livewire_component(&self, name: &str) -> Option<ComponentTarget> {
        self.lookup(&format!("livewire:{name}"))
    }
}

impl TestComponents {
    fn lookup(&self, tag: &str) -> Option<ComponentTarget> {
        self.0
            .iter()
            .find(|(known, _)| known == tag)
            .map(|(_, target)| target.clone())
    }
}

/// A parameter list from `name` / `name?` / `name=fallback` spellings:
/// required with nothing to stand in for it, optional, and required
/// with what Laravel's container would pass.
fn params(spec: &[&str]) -> Vec<ComponentParameter> {
    spec.iter()
        .map(|entry| match entry.split_once('=') {
            Some((name, fallback)) => ComponentParameter {
                name: name.to_string(),
                fallback: Some(fallback.to_string()),
            },
            None => ComponentParameter {
                name: entry.trim_end_matches('?').to_string(),
                fallback: None,
            },
        })
        .collect()
}

fn target(fqn: &str, binding: ComponentBinding) -> ComponentTarget {
    ComponentTarget {
        fqn: fqn.to_string(),
        binding,
    }
}

fn preprocess_with_components(content: &str, components: Vec<(String, ComponentTarget)>) -> String {
    let resolver = TestComponents(components);
    preprocess_with_vars(
        content,
        &[],
        TemplateKind::View,
        None,
        Some(&resolver as &dyn ComponentResolver),
        &Default::default(),
    )
    .0
}

/// `Alert::__construct(string $type, ?Post $post = null)`.
fn alert() -> Vec<(String, ComponentTarget)> {
    vec![(
        "x-alert".to_string(),
        target(
            "App\\View\\Components\\Alert",
            ComponentBinding::Construct(params(&["type", "post?"])),
        ),
    )]
}

/// A tag the component index knows binds `$component` to the class
/// behind it, and its attributes become the arguments Laravel builds
/// that class with, so a component handed the wrong thing is reported
/// the way any other call is.
#[test]
fn test_preprocess_component_tag_builds_the_component() {
    let php =
        preprocess_with_components(r#"<x-alert type="danger">{{ $slot }}</x-alert>"#, alert());
    assert!(
        php.contains("$component = new \\App\\View\\Components\\Alert("),
        "the tag should build its component: {php}"
    );
    assert!(
        php.contains("type: 'danger'"),
        "a plain attribute becomes a named argument: {php}"
    );
    assert!(
        php.contains("/* /x-alert */"),
        "the closing tag should become a comment: {php}"
    );
    assert!(
        !php.contains(r#"type="danger""#),
        "attribute markup should stay masked: {php}"
    );
}

/// A bound attribute's expression is the argument, emitted where the
/// template wrote it so that hovering it lands on the template's own
/// text. An attribute naming no parameter is markup Laravel routes to
/// the component's attribute bag, so it is not an argument at all --
/// but its expression is still emitted, or a variable used only there
/// would read as unused.
#[test]
fn test_preprocess_component_tag_partitions_its_attributes() {
    let php = preprocess_with_components(
        r#"<x-alert :type="$kind" class="m-2" :data-id="$id" />"#,
        alert(),
    );
    assert!(
        php.contains("$__blade_arg_type = $kind;") && php.contains("type: $__blade_arg_type"),
        "a bound attribute naming a parameter is an argument: {php}"
    );
    assert!(
        !php.contains("class:") && !php.contains("dataId:"),
        "an attribute the attribute bag takes is not an argument: {php}"
    );
    assert!(
        php.contains("blade_bound_attr_directive($id);"),
        "a bound attribute that is not an argument still contributes \
         its expression: {php}"
    );
}

/// Laravel builds a component the tag left incomplete through the
/// container, so a parameter no attribute filled is passed what the
/// container would pass rather than being reported missing. One
/// nothing can stand in for is left out, which is the case Laravel
/// itself fails on.
#[test]
fn test_preprocess_component_tag_fills_what_the_container_would() {
    let components = vec![(
        "x-card".to_string(),
        target(
            "App\\View\\Components\\Card",
            ComponentBinding::Construct(params(&[
                "title",
                "footer?",
                "service=resolve(\\App\\Service::class)",
                "count=null",
            ])),
        ),
    )];
    let php = preprocess_with_components("<x-card />", components);
    assert!(
        php.contains("service: resolve(\\App\\Service::class)") && php.contains("count: null"),
        "the container's own arguments should be passed: {php}"
    );
    assert!(
        !php.contains("title:") && !php.contains("footer:"),
        "a parameter with a default and one nothing can fill are both \
         left out: {php}"
    );
}

/// A dotted tag names a component in a sub-directory, and an index
/// component answers to its directory alone; both are ordinary index
/// lookups, so the preprocessor only has to pass the name through
/// unchanged.
#[test]
fn test_preprocess_component_tag_passes_the_written_name_through() {
    let nested = [
        ("x-forms.input", "App\\View\\Components\\Forms\\Input"),
        ("x-card", "App\\View\\Components\\Card\\Card"),
        ("x-pkg::calendar", "Vendor\\Pkg\\Calendar"),
    ];
    let components: Vec<(String, ComponentTarget)> = nested
        .iter()
        .map(|(tag, fqn)| {
            (
                (*tag).to_string(),
                target(fqn, ComponentBinding::Construct(Vec::new())),
            )
        })
        .collect();
    for (tag, fqn) in nested {
        let php = preprocess_with_components(&format!("<{tag} />"), components.clone());
        assert!(
            php.contains(&format!("$component = new \\{fqn}(")),
            "<{tag}> should resolve to {fqn}: {php}"
        );
    }
}

/// Livewire builds its component through the container and hands the
/// tag's attributes to `mount()`.
#[test]
fn test_preprocess_livewire_tag_mounts_the_component() {
    let components = vec![(
        "livewire:counter".to_string(),
        target(
            "App\\Livewire\\Counter",
            ComponentBinding::Mount(params(&["count"])),
        ),
    )];
    let php = preprocess_with_components(r#"<livewire:counter :count="$n" />"#, components);
    assert!(
        php.contains("$component = new \\App\\Livewire\\Counter();"),
        "the tag should build its Livewire component: {php}"
    );
    assert!(
        php.contains("$component->mount(count: $__blade_arg_count);"),
        "the attributes are `mount()`'s arguments: {php}"
    );
}

/// A component whose attributes are arguments to nothing (an
/// anonymous one, whose attributes are its *view's* variables) still
/// declares `$component` so the tag body can reach it.
#[test]
fn test_preprocess_anonymous_component_declares_without_a_call() {
    let components = vec![(
        "x-banner".to_string(),
        target(super::super::ANONYMOUS_COMPONENT, ComponentBinding::Declare),
    )];
    let php = preprocess_with_components(r#"<x-banner :title="$t" />"#, components);
    assert!(
        php.contains(
            "/** @var \\Illuminate\\View\\AnonymousComponent $component */ $component = null;"
        ),
        "an anonymous component is declared, not built: {php}"
    );
    assert!(
        php.contains("blade_bound_attr_directive($t);"),
        "its attributes still contribute their expressions: {php}"
    );
}

/// Markup between a tag's name and its `>` becomes statements — a
/// `{{ }}` echo in an attribute value, a directive, a bound attribute
/// the attribute bag takes — which is why the call is emitted where
/// the tag closes rather than spanning it. The statements stand, and
/// the call still happens.
#[test]
fn test_preprocess_component_tag_survives_markup_between_its_attributes() {
    let php = preprocess_with_components(r#"<x-alert type="a {{ $kind }}" />"#, alert());
    assert!(
        php.contains("echo e( $kind );"),
        "the echo in the attribute value still runs: {php}"
    );
    assert!(
        php.contains("new \\App\\View\\Components\\Alert(type: (string) '')"),
        "an interpolated value is a string and nothing more precise: {php}"
    );

    let php = preprocess_with_components("<x-alert @if($a) type=\"x\" @endif />", alert());
    assert!(
        php.contains("if ($a):") && php.contains("new \\App\\View\\Components\\Alert("),
        "a directive between attributes is still a directive: {php}"
    );
}

/// A bound attribute is only at attribute position inside a tag, and
/// the component tag's `<` never reaches the HTML scanner — so the
/// tag state has to be carried across the replacement, including on a
/// tag that spans lines.
#[test]
fn test_preprocess_component_tag_keeps_bound_attributes_in_scope() {
    let php = preprocess_with_components("<x-alert\n  :type=\"$kind\"\n/>\n:notAnAttr", alert());
    assert!(
        php.contains("$__blade_arg_type = $kind;") && php.contains("type: $__blade_arg_type"),
        "a bound attribute on a multi-line component tag: {php}"
    );
    assert!(
        !php.contains("blade_bound_attr_directive(notAnAttr"),
        "a colon after the tag closed is not an attribute: {php}"
    );
}

/// A tag name written inside an attribute value is markup, not a tag.
#[test]
fn test_preprocess_component_tag_inside_an_attribute_value_is_not_a_tag() {
    let php = preprocess_with_components(r#"<div data-tpl="<x-alert />"></div>"#, alert());
    assert!(
        !php.contains("$component ="),
        "a tag written inside an attribute value is not a tag: {php}"
    );
}

/// A tag no index resolves — an unknown component, `<x-slot>`, or
/// `<x-dynamic-component>` — degrades to a comment, and a bound
/// attribute on it still contributes its expression.
#[test]
fn test_preprocess_unresolved_component_tags_degrade_to_comments() {
    let php = preprocess_with_components(
        r#"<x-dynamic-component :component="$name" :attr="$v" /><x-slot:title>t</x-slot><x-unknown />"#,
        alert(),
    );
    for comment in [
        "/* x-dynamic-component */",
        "/* x-slot:title */",
        "/* x-unknown */",
    ] {
        assert!(php.contains(comment), "expected {comment} in: {php}");
    }
    assert!(
        !php.contains("$component ="),
        "no unresolved tag may bind a component: {php}"
    );
    assert!(
        php.contains("blade_bound_attr_directive($name);")
            && php.contains("blade_bound_attr_directive($v);"),
        "a dynamic component's expressions are still parsed: {php}"
    );
}

/// The `:$var` shorthand expands to a bound `var` attribute whose
/// expression is `$var`.
#[test]
fn test_preprocess_bound_attribute_shorthand() {
    let content = r#"<x-alert :$message />"#;
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($message);"),
        "`:$var` shorthand should emit the variable as PHP: {}",
        php
    );
}

/// A bound attribute whose value contains a PHP string literal (with
/// the opposite quote) must be captured whole, not truncated at the
/// inner quote.
#[test]
fn test_preprocess_bound_attribute_with_inner_string() {
    let content = r#"<x-btn :class="$active ? 'on' : 'off'" />"#;
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($active ? 'on' : 'off');"),
        "inner string literals should be preserved in the expression: {}",
        php
    );
}

/// Colons that are not at attribute position must never be treated as
/// bindings: inside an attribute value (`mailto:`), in text between
/// tags (`10:30`), or as an escaped literal colon (`::class`).
#[test]
fn test_preprocess_bound_attribute_does_not_misfire_on_value_colons() {
    let content =
        "<a href=\"mailto:x@example.com\">10:30</a>\n<x-c ::class=\"literal\" :real=\"$v\" />";
    let (php, _) = preprocess(content);
    // The only binding here is `:real="$v"`.
    assert!(
        php.contains("blade_bound_attr_directive($v);"),
        "the real binding should still be emitted: {}",
        php
    );
    assert_eq!(
        php.matches("blade_bound_attr_directive(").count(),
        1,
        "no spurious bindings from value/text/escaped colons: {}",
        php
    );
    // `mailto:` and the escaped `::class` literal must stay masked.
    assert!(
        !php.contains("mailto"),
        "attr value must stay masked: {}",
        php
    );
    assert!(
        !php.contains("literal"),
        "escaped `::` attribute must stay masked: {}",
        php
    );
}

/// A `:name="..."` written outside any tag (in text) must not be
/// treated as a binding.
#[test]
fn test_preprocess_bound_attribute_ignored_outside_tag() {
    let content = r#"<p>ratio :w="16" here</p>"#;
    let (php, _) = preprocess(content);
    assert_eq!(
        php.matches("blade_bound_attr_directive(").count(),
        0,
        "a colon in text (outside a tag span) is not a binding: {}",
        php
    );
}

/// A bound attribute split across lines from its tag opener must still
/// be recognized (tags span multiple lines in real templates).
#[test]
fn test_preprocess_bound_attribute_multiline_tag() {
    let content = "<x-img.size\n    :src=\"$image\"\n/>";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($image);"),
        "binding on a continuation line should be recognized: {}",
        php
    );
}

/// A bound attribute whose expression is wrapped over several lines
/// (what a formatter does to a long array) must be emitted whole, not
/// truncated at the first line break.
#[test]
fn test_preprocess_bound_attribute_multiline_expression() {
    let content = "<x-file.upload name=\"image\"\n    :rules=\"[\n        'Dimensions must match: 2420 x 1614',\n        'Max file size: 2 mb',\n    ]\" />\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive([") && php.contains("]);"),
        "the wrapped array must be emitted whole: {}",
        php
    );
    assert!(
        php.contains("'Dimensions must match: 2420 x 1614',"),
        "continuation lines must survive: {}",
        php
    );
    assert!(
        !php.contains("[);"),
        "the expression must not be closed off at the line break: {}",
        php
    );
    assert!(
        !php.contains("name=\"image\""),
        "the surrounding tag markup must stay masked: {}",
        php
    );
}

/// A multi-line bound attribute holding a call must keep every argument,
/// otherwise the truncated call reports a bogus argument-count mismatch.
#[test]
fn test_preprocess_bound_attribute_multiline_call() {
    let content = "<x-alert\n    :message=\"__('a.b',\n        ['count' => 2])\"\n/>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("__('a.b',") && php.contains("['count' => 2]));"),
        "both call arguments must survive the wrap: {}",
        php
    );
}

/// A bound attribute whose closing quote never appears is malformed;
/// the call is closed at end of line so only that attribute is lost.
#[test]
fn test_preprocess_bound_attribute_unterminated() {
    let content = "<x-alert :message=\"$msg\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_bound_attr_directive($msg);"),
        "an unterminated attribute must be closed at end of line: {}",
        php
    );
    assert!(
        php.contains("echo e( $after );"),
        "the rest of the template must still be processed: {}",
        php
    );
}

#[test]
fn test_preprocess_forelse_loop_variable() {
    let content = "@forelse($items as $item)\n{{ $loop->index }}\n@empty\n@endforelse\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$loop = (object)[];"),
        "forelse should also inject $loop: {}",
        php
    );
}

#[test]
fn test_preprocess_echo_with_string_braces() {
    let content = "{{ \"}} \" }}";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo e( \"}} \" );"),
        "Failed to parse braces inside string: {}",
        php
    );
}

/// An `@` before `{{ ... }}` leaves the expression untouched for a
/// frontend template engine, even when the expression is not valid PHP.
#[test]
fn test_preprocess_at_escaped_echo_is_masked() {
    let content = "@{{.Image}} and {{ $serverValue }}";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains(".Image"),
        "the frontend expression must stay out of virtual PHP: {php}"
    );
    assert!(
        php.contains("echo e( $serverValue );"),
        "a real Blade echo after it must still compile: {php}"
    );
}

/// Laravel's escaped-echo pattern spans lines and does not interpret
/// quotes inside the frontend expression as PHP string delimiters.
#[test]
fn test_preprocess_at_escaped_echo_spans_lines() {
    let content = "@{{\n    frontend['unterminated]\n}}\n{{ $after }}";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("frontend"),
        "the whole multiline frontend expression must be masked: {php}"
    );
    assert!(
        php.contains("echo e( $after );"),
        "processing must return to Blade after the escaped echo: {php}"
    );
}

/// Escaping a raw echo leaves its contents as frontend template text and
/// resumes Blade processing after the raw `!!}` terminator.
#[test]
fn test_preprocess_at_escaped_raw_echo_is_masked() {
    let content = "@{!! $frontendName !!} and {!! $serverValue !!}";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("$frontendName"),
        "the escaped raw expression must stay out of virtual PHP: {php}"
    );
    assert!(
        php.contains("echo  $serverValue ;"),
        "a real raw Blade echo after it must still compile: {php}"
    );
}

/// An escape the file never closes, which is what a half-typed `@{{`
/// looks like, must not mask on to the end of the file: the `@endif`
/// it swallows leaves the emitted `if (…):` open and reports the whole
/// template as a syntax error.
#[test]
fn test_preprocess_unclosed_at_escaped_echo_ends_at_eol() {
    let content = "@if($a)\n<p>@{{ mess\n@endif";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if ($a):") && php.contains("endif;"),
        "the block around an unclosed escape must still close: {php}"
    );

    let raw = "@foreach($rows as $r)\n@{!! mess\n@endforeach";
    let (php, _) = preprocess(raw);
    assert!(
        php.contains("endforeach;"),
        "the same for an unclosed escaped raw echo: {php}"
    );
}

/// A raw `{!! … !!}` echo compiles to a naked `echo` with no `e()`
/// wrapper, and it starts at `{!!`, not `{{!!`: `{!! $v !!}` after a
/// `<?php $v = …; ?>` block must count as a use of `$v`.
#[test]
fn test_preprocess_raw_echo_single_brace() {
    let content = "<?php\n$acmeProfile = \"xxx\";\n?>\n\n{!! $acmeProfile !!}\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo  $acmeProfile ;"),
        "raw echo should emit a naked echo of the expression: {}",
        php
    );
}

/// Blade matches echo tags longest-opening-first, so `{{!! $v !!}}` is
/// a literal `{`, a raw echo of `$v`, and a literal `}` — not an
/// escaped echo of `!! $v !!`.
#[test]
fn test_preprocess_raw_echo_wrapped_in_extra_braces() {
    let content = "{{!! $html !!}}";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo  $html ;"),
        "the raw echo inside the extra braces should still compile: {}",
        php
    );
    assert!(
        !php.contains("echo e("),
        "the outer braces are literal text, not an escaped echo: {}",
        php
    );
}

#[test]
fn test_preprocess_raw_and_escaped_echo_close_independently() {
    let content = "{!! $html !!} and {{ $safe }}";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo  $html ;"),
        "raw echo emits without e(): {}",
        php
    );
    assert!(
        php.contains("echo e( $safe );"),
        "escaped echo still wraps in e(): {}",
        php
    );
}

/// An echo opener that nothing in the rest of the file closes must not
/// swallow every later line as PHP: it is closed at end of line, so at
/// most one line degrades and the rest of the template still parses.
#[test]
fn test_preprocess_unterminated_echo_opener_is_closed_at_end_of_line() {
    let content = "<script>if (a) {!!b}</script>\n{{ $after }}\n<p>plain markup</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo e( $after );"),
        "the echo on the next line must still compile: {}",
        php
    );
    assert!(
        !php.contains("plain markup"),
        "later markup must be masked as HTML, not emitted as PHP: {}",
        php
    );
    // The unclosed echo must be closed as a statement rather than
    // left to swallow the wrapper function's closing brace.
    assert!(
        php.contains("echo b}</script>; "),
        "the opener's own line degrades and is closed at its end: {}",
        php
    );
}

/// A half-typed echo is not an unpaired opener when a terminator exists
/// further down (e.g. the user is typing inside an echo whose `!!}` is
/// already there, or another echo's terminator follows): the expression
/// must stay open so completion keeps working mid-edit.
#[test]
fn test_preprocess_echo_spanning_lines_stays_open() {
    let content = "{{ $user\n    ->name }}\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo e( $user"),
        "the multi-line echo must open: {}",
        php
    );
    assert!(
        php.contains("->name );"),
        "the multi-line echo must close at its own terminator: {}",
        php
    );
}

/// An echo left open across lines must not swallow the `@end…` that
/// closes the block it sits inside, even when some later line has a
/// `}}` that belongs to a different echo (which would otherwise make
/// the unpaired-opener safety net think this echo is still closable).
/// A directive seen while the echo is open ends the echo instead of
/// being absorbed into it, matching Blade's own compile order
/// (statements before echoes).
#[test]
fn test_preprocess_directive_ends_an_echo_left_open_across_lines() {
    let content = "@if($showName)\n    <p>{{ $user->name\n@endif\n{{ $footer }}\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if ($showName):"),
        "the if block should open normally: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "the @endif must still close the if block rather than being \
         swallowed by the still-open echo: {}",
        php
    );
    assert!(
        php.contains("echo e( $footer )"),
        "the later, unrelated echo must still compile normally: {}",
        php
    );
}

/// The same swallowing bug affects the raw echo and the `@`-escaped
/// echo forms: a directive must end them too.
#[test]
fn test_preprocess_directive_ends_a_raw_or_escaped_echo_left_open() {
    let raw = "@if($a)\n{!! $x\n@endif\n{!! $y !!}\n";
    let (php, _) = preprocess(raw);
    assert!(
        php.contains("endif;"),
        "@endif must close the if even though a later {{!! !!}} exists: {}",
        php
    );

    let escaped = "@if($a)\n<p>@{{ mess\n@endif\n{{ $after }}\n";
    let (php, _) = preprocess(escaped);
    assert!(
        php.contains("endif;"),
        "@endif must close the if even though a later escaped echo exists: {}",
        php
    );
    assert!(
        php.contains("echo e( $after )"),
        "the later real echo must still compile: {}",
        php
    );
}

#[test]
fn test_preprocess_foreach() {
    let content = r#"@php
/**
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@foreach($users->active()->byName() as $user)
    <p>{{ $user->name }}</p>
@endforeach
"#;
    let (php, _) = preprocess(content);
    for (i, line) in php.lines().enumerate() {
        eprintln!("{:2}: {}", i, line);
    }
    assert!(php.contains("$user->name"));
}

#[test]
fn test_preprocess_forelse() {
    let content = r#"@forelse($users as $user)
    <p>{{ $user->name }}</p>
@empty
    <p>No users</p>
@endforelse
"#;
    let (php, _) = preprocess(content);
    for (i, line) in php.lines().enumerate() {
        eprintln!("{:2}: {}", i, line);
    }
    assert!(php.contains("foreach"), "should contain foreach: {}", php);
    assert!(
        php.contains("endforeach"),
        "should contain endforeach: {}",
        php
    );
    assert!(
        php.contains("if (false):"),
        "should contain if (false): {}",
        php
    );
    assert!(php.contains("endif;"), "should contain endif: {}", php);
}

#[test]
fn test_preprocess_session_directive() {
    let content = "@session('key')\n    <p>{{ $value }}</p>\n@endsession\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true)"),
        "should contain if (true): {}",
        php
    );
    assert!(
        php.contains("$value = '';"),
        "should inject $value: {}",
        php
    );
    assert!(php.contains("endif;"), "should contain endif: {}", php);
}

#[test]
fn test_preprocess_verbatim() {
    let content = "@verbatim\n    {{ $name }}\n    @if(true)\n@endverbatim\n<p>{{ $real }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("$name"),
        "verbatim content should be skipped: {}",
        php
    );
    assert!(
        php.contains("$real"),
        "content after endverbatim should work: {}",
        php
    );
}

#[test]
fn test_preprocess_verbatim_with_comment_syntax() {
    // Verbatim blocks may contain */ which would break PHP block comments
    let content =
        "@verbatim\n    {{ /* js comment */ value }}\n@endverbatim\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("js comment"),
        "verbatim content should be skipped: {}",
        php
    );
    assert!(
        php.contains("$after"),
        "content after endverbatim should work: {}",
        php
    );
}

#[test]
fn test_preprocess_error_directive() {
    let content = "@error('email')\n    <p>{{ $message }}</p>\n@enderror\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true)"),
        "should contain if (true): {}",
        php
    );
    assert!(
        php.contains("$message = '';"),
        "should inject $message: {}",
        php
    );
    assert!(php.contains("endif;"), "should contain endif: {}", php);
}

#[test]
fn test_preprocess_context_directive() {
    let content = "@context('key')\n    <p>{{ $value }}</p>\n@endcontext\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true)"),
        "should contain if (true): {}",
        php
    );
    assert!(
        php.contains("$value = '';"),
        "should inject $value: {}",
        php
    );
    assert!(php.contains("endif;"), "should contain endif: {}", php);
}

/// The marker functions are registered once for the whole project, so
/// a template calls them without declaring anything itself.
#[test]
fn test_preprocess_declares_no_marker_functions() {
    let (php, _) = preprocess("<p>hello</p>");
    assert!(
        !php.contains("function blade_"),
        "a template must not declare the markers it calls: {}",
        php
    );

    let stubs = crate::blade::with_marker_stubs(crate::ci_map::CiMap::new());
    for marker in ["blade_view_directive", "blade_each_directive"] {
        let source = stubs
            .get(marker)
            .unwrap_or_else(|| panic!("{marker} should be registered as a stub"));
        assert!(
            source.contains(&format!("function {marker}")),
            "the stub should declare {marker}: {source}"
        );
    }
}

/// `@each` gets a marker of its own: the arguments after its view name
/// are a collection and an item name, not a data array.
#[test]
fn test_preprocess_each_uses_its_own_marker() {
    let (php, _) = preprocess("@each('partials.row', $rows, 'row')\n");
    assert!(
        php.contains("blade_each_directive ('partials.row', $rows, 'row');"),
        "@each should compile to a blade_each_directive call: {}",
        php
    );
}

#[test]
fn test_preprocess_multiline_directive() {
    let content = "@include('vendor.fbRemarket', [\n    'facebook_pixel_id' => Config::get('services.facebook.pixel_id'),\n])\n\n@include('vendor.googleRemarket')";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_view_directive"),
        "@include should produce blade_view_directive call: {}",
        php
    );

    let content2 = "{{\n    $var\n}}";
    let (php2, _) = preprocess(content2);
    assert!(
        php2.contains("$var"),
        "Multiline echo should preserve variable: {}",
        php2
    );
}

#[test]
fn test_preprocess_stub_directives() {
    // @csrf should produce a comment (no-args directive)
    let content = "@csrf\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("/* @csrf */"),
        "@csrf should become a comment: {}",
        php
    );

    // @auth without args should produce if (true):
    let content = "@auth\n<p>logged in</p>\n@endauth\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true):"),
        "@auth should produce if (true):: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endauth should produce endif;: {}",
        php
    );

    // @auth with args should also produce if (true):
    let content = "@auth('admin')\n<p>admin</p>\n@endauth\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true)"),
        "@auth('admin') should produce if (true): {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endauth should produce endif;: {}",
        php
    );

    // @guest without args
    let content = "@guest\n<p>guest</p>\n@endguest\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true):"),
        "@guest should produce if (true):: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endguest should produce endif;: {}",
        php
    );

    // @production (never takes args)
    let content = "@production\n<p>prod</p>\n@endproduction\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true):"),
        "@production should produce if (true):: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endproduction should produce endif;: {}",
        php
    );

    // @env with args
    let content = "@env('local')\n<p>local</p>\n@endenv\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true)"),
        "@env should produce if (true): {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endenv should produce endif;: {}",
        php
    );

    // @once without args
    let content = "@once\n<script>app.js</script>\n@endonce\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("if (true):"),
        "@once should produce if (true):: {}",
        php
    );
    assert!(
        php.contains("endif;"),
        "@endonce should produce endif;: {}",
        php
    );
}

#[test]
fn test_preprocess_raw_php_tag_preserves_at_prefixed_string() {
    // A raw <?php ... ?> block (not @php/@endphp) containing a string
    // literal that starts with '@' (e.g. a JSON-LD '@context' key) must
    // not be misread as a Blade directive.
    let content = "@php\n@endphp\n<?php\n$schema = ['@context' => 'x'];\n?>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("'@context' => 'x'"),
        "raw PHP tag content should pass through verbatim: {}",
        php
    );
}

#[test]
fn test_preprocess_raw_php_tag_short_echo() {
    let content = "<p><?= $value ?></p>";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo  $value ;"),
        "<?= ?> should translate to an echo statement: {}",
        php
    );
}

#[test]
fn test_preprocess_switch_case_with_class_constant() {
    let content =
        "@switch($x)\n    @case (App\\Enums\\E::A)\n        {{ 1 }}\n        @break\n@endswitch\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("case  (App\\Enums\\E::A):"),
        "@case should preserve its argument and emit a trailing colon: {}",
        php
    );
    assert!(php.contains("break;"), "@break should emit break;: {}", php);
}

#[test]
fn test_preprocess_session_value_accessible() {
    // $value should be accessible inside @session block
    let content = "@session('status')\n{{ $value }}\n@endsession\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$value = '';"),
        "should declare $value: {}",
        php
    );
    // The $value echo should appear after the declaration
    let val_decl = php.find("$value = '';").unwrap();
    // Find last occurrence of $value (the echo usage)
    let val_echo = php.rfind("$value").unwrap();
    assert!(
        val_echo > val_decl,
        "$value usage should come after declaration: {}",
        php
    );
}

#[test]
fn test_preprocess_error_message_accessible() {
    // $message should be accessible inside @error block
    let content = "@error('email')\n{{ $message }}\n@enderror\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$message = '';"),
        "should declare $message: {}",
        php
    );
    let msg_decl = php.find("$message = '';").unwrap();
    let msg_echo = php.rfind("$message").unwrap();
    assert!(
        msg_echo > msg_decl,
        "$message usage should come after declaration: {}",
        php
    );
}

/// `@unless`/`@isset`/`@empty(...)` translate to `if(!`/`if(isset`/
/// `if(empty` respectively — an extra, unmatched opening paren on top
/// of the directive's own argument parens — so the directive needs a
/// second closing paren before the trailing `:`, or the next PHP
/// parser sees `unexpected token ':', expected ')'` and the rest of
/// the template is corrupted.
#[test]
fn test_preprocess_unless_isset_empty_close_extra_paren() {
    let (unless_php, _) = preprocess("@unless($cond)\nx\n@endunless\n<p>after</p>");
    assert!(
        unless_php.contains("if(! ($cond)):"),
        "@unless should close both the synthetic and the argument paren: {}",
        unless_php
    );

    let (isset_php, _) = preprocess("@isset($var)\nx\n@endisset\n<p>after</p>");
    assert!(
        isset_php.contains("if(isset ($var)):"),
        "@isset should close both the synthetic and the argument paren: {}",
        isset_php
    );

    let (empty_php, _) = preprocess("@empty($var)\nx\n@endempty\n<p>after</p>");
    assert!(
        empty_php.contains("if(empty ($var)):"),
        "@empty(...) should close both the synthetic and the argument paren: {}",
        empty_php
    );
}

/// `@use('App\Models\Post')` must become a real top-level `use` import
/// (hoisted out of the wrapper function), and must not leave the parser
/// in PHP mode corrupting the rest of the template.
#[test]
fn test_preprocess_use_directive_emits_import() {
    let content = "@use('App\\Models\\Post')\n<p>after</p>";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("use App\\Models\\Post;"),
        "@use should emit a real use import: {}",
        php
    );
    // The import is hoisted into the prologue: top-level (not inside
    // the wrapper function) and ahead of every name it imports, since
    // name resolution runs in source order.
    let wrapper = php.find("function __blade_template()").unwrap();
    let import = php.find("use App\\Models\\Post;").unwrap();
    assert!(
        import < wrapper,
        "the use import must be hoisted into the prologue: {}",
        php
    );
    // Content after @use must stay masked as HTML, not leak as raw PHP.
    assert!(
        !php.contains("after"),
        "content after @use(...) should be masked as HTML: {}",
        php
    );
}

/// The inline-alias form `@use('App\Models\Post as Article')` keeps the
/// alias.
#[test]
fn test_preprocess_use_directive_inline_alias() {
    let content = "@use('App\\Models\\Post as Article')\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("use App\\Models\\Post as Article;"),
        "@use with an inline `as` should preserve the alias: {}",
        php
    );
}

/// The two-argument alias form `@use('App\Models\Post', 'Article')`
/// produces the same aliased import.
#[test]
fn test_preprocess_use_directive_second_arg_alias() {
    let content = "@use('App\\Models\\Post', 'Article')\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("use App\\Models\\Post as Article;"),
        "@use with a second alias argument should preserve the alias: {}",
        php
    );
}

/// The `function`/`const` modifiers are carried through to the import.
#[test]
fn test_preprocess_use_directive_function_modifier() {
    let content = "@use('function App\\Support\\helper')\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("use function App\\Support\\helper;"),
        "@use with a function modifier should emit `use function`: {}",
        php
    );
}

/// `@inject('metrics', 'App\Services\Metrics')` becomes an inline
/// `$metrics = app(...)` assignment so the injected variable is defined
/// and typed, and does not corrupt the rest of the template.
#[test]
fn test_preprocess_inject_directive_emits_assignment() {
    let content = "@inject('metrics', 'App\\Services\\Metrics')\n<p>after</p>";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$metrics = app('App\\Services\\Metrics');"),
        "@inject should emit an inline app() assignment: {}",
        php
    );
    // The assignment is inline (inside the wrapper function), so it must
    // come before the wrapper function's closing brace.
    let brace = php.rfind('}').unwrap();
    let assign = php.find("$metrics = app(").unwrap();
    assert!(
        assign < brace,
        "the inject assignment must stay inside the wrapper function: {}",
        php
    );
    assert!(
        !php.contains("after"),
        "content after @inject(...) should be masked as HTML: {}",
        php
    );
}

/// An apostrophe inside a `{{-- ... --}}` comment must not be mistaken
/// for the start of a PHP string literal — that previously made the
/// scanner hunt for a matching closing quote instead of the comment's
/// `--}}` terminator, desyncing the rest of the file.
#[test]
fn test_preprocess_comment_with_apostrophe_does_not_desync() {
    let content = "{{-- user's note --}}\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("/*  user's note"),
        "comment should translate to a block comment: {}",
        php
    );
    assert!(
        php.contains("echo e( $after )"),
        "content after the comment should still translate normally: {}",
        php
    );
}

/// A double quote inside a `{{-- ... --}}` comment must not be mistaken
/// for the start of a PHP string literal either — same root cause as
/// the apostrophe case above.
#[test]
fn test_preprocess_comment_with_double_quote_does_not_desync() {
    let content = "{{-- say \"hi\" --}}\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("/*  say \"hi\""),
        "comment should translate to a block comment: {}",
        php
    );
    assert!(
        php.contains("echo e( $after )"),
        "content after the comment should still translate normally: {}",
        php
    );
}

/// The text of the first `/* ... */` block comment in the virtual PHP.
/// Panics if there is no closed block comment, which is itself the bug
/// the callers are guarding against.
fn comment_body(php: &str) -> &str {
    let start = php.find("/* ").expect("a block comment should be emitted");
    let rest = &php[start + 3..];
    let end = rest.find("*/").expect("the comment should be closed");
    &rest[..end]
}

/// Commenting out an echo is the usual reason to write a Blade comment,
/// so the `}}` / `!!}` of the commented-out echo must not be taken for
/// the comment's terminator: only a contiguous `--}}` ends a comment.
#[test]
fn test_preprocess_comment_containing_echo_does_not_desync() {
    for content in [
        "{{-- {{ $old }} --}}\n<p>{{ $after }}</p>\n",
        "{{-- {!! $old !!} --}}\n<p>{{ $after }}</p>\n",
    ] {
        let (php, _) = preprocess(content);
        assert!(
            comment_body(&php).contains("$old"),
            "the commented-out echo should stay inside the block comment: {}",
            php
        );
        assert!(
            php.contains("echo e( $after )"),
            "content after the comment should still translate normally: {}",
            php
        );
    }
}

/// `@endphp` mentioned in comment prose is text, not the end of an
/// `@php` block, so it must not terminate the comment either.
#[test]
fn test_preprocess_comment_mentioning_endphp_does_not_desync() {
    let content = "{{-- use @php/@endphp instead --}}\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    assert!(
        comment_body(&php).contains("@endphp instead"),
        "the mentioned directive should stay inside the block comment: {}",
        php
    );
    assert!(
        php.contains("echo e( $after )"),
        "content after the comment should still translate normally: {}",
        php
    );
}

/// Commenting out a block of PHP is the usual reason to write a Blade
/// comment, so a `*/` in the comment text must not close the emitted
/// block comment early — everything after it would become live PHP.
#[test]
fn test_preprocess_comment_containing_block_comment_end_does_not_desync() {
    let content = "{{-- see /* legacy */ code --}}\n<p>{{ $after }}</p>\n";
    let (php, _) = preprocess(content);
    let body = comment_body(&php);
    assert!(
        body.contains("legacy") && body.contains("code"),
        "the whole comment text should stay inside the block comment: {}",
        php
    );
    assert!(
        php.contains("echo e( $after )"),
        "content after the comment should still translate normally: {}",
        php
    );
    let emitted = php
        .lines()
        .find(|l| l.contains("legacy"))
        .expect("the comment line");
    assert_eq!(
        emitted.encode_utf16().count(),
        content.lines().next().unwrap().encode_utf16().count() + 2,
        "blanking `*/` must keep the columns aligned; only the \
         two-character `--}}` terminator grows (to ` */ `): {}",
        php
    );
}

/// An unterminated `{{--` must still emit a closed `/* ... */`, or the
/// open comment swallows the wrapper function's closing brace and makes
/// the whole virtual file unparseable.
#[test]
fn test_preprocess_unterminated_comment_is_closed() {
    let content = "<p>{{ $before }}</p>\n{{-- forgot to close\nstill comment\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("echo e( $before )"),
        "content before the comment should translate normally: {}",
        php
    );
    let comment_start = php.find("/* ").expect("comment should be emitted");
    let comment_end = php[comment_start..]
        .find("*/")
        .expect("unterminated comment should still be closed");
    assert!(
        php[comment_start + comment_end..].contains('}'),
        "the wrapper function's closing brace must not be inside the comment: {}",
        php
    );
}

/// `@inject` with a `::class` service expression is preserved verbatim
/// (Blade keeps the second argument unquoted-trimmed).
#[test]
fn test_preprocess_inject_directive_class_constant_service() {
    let content = "@inject('repo', App\\Repo::class)\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("$repo = app(App\\Repo::class);"),
        "@inject should preserve a ::class service expression: {}",
        php
    );
}

/// The prologue text before the wrapper function: where every declared
/// variable lives.
fn prologue_of(php: &str) -> &str {
    php.split_once("function __blade_template()").unwrap().0
}

/// `@props` declares each key in the prologue, typed from its default
/// value, so the forward walker sees it as defined and typed without
/// waiting on the caller's `<x-… />` attributes.
#[test]
fn test_preprocess_props_directive_declares_variables() {
    let content = "@props(['caption' => '', 'count' => 0])\n{{ $caption }}\n";
    let (php, _) = preprocess(content);
    let prologue = prologue_of(&php);
    assert!(
        prologue.contains("$caption = '';") && prologue.contains("$count = 0;"),
        "@props should declare each key with its default: {}",
        php
    );
    assert!(
        php.contains("global $errors, $__env, $caption, $count;"),
        "props must be pulled into the wrapper scope: {}",
        php
    );
}

/// The declaration belongs in the prologue, not the template body. An
/// assignment in the body would overwrite whatever type the author
/// declared for the same name, and read as a dead local assignment to
/// the unused-variable check.
#[test]
fn test_preprocess_props_directive_does_not_assign_in_the_body() {
    let content = "@props(['caption' => ''])\n<span>{{ $caption }}</span>\n";
    let (php, _) = preprocess(content);
    let body = php.split_once("function __blade_template()").unwrap().1;
    assert!(
        !body.contains("$caption ="),
        "the body must not re-assign a prop: {}",
        php
    );
    // The default expression stays visible so it is still type-checked.
    assert!(
        body.contains("blade_directive"),
        "the directive's arguments should still be analysed: {}",
        php
    );
}

/// A `@props` key the template's own docblock already declares keeps the
/// declared type: the signature is the contract, `@props` only supplies
/// what the signature leaves out.
#[test]
fn test_declared_signature_wins_over_a_props_default() {
    let content = "@php\n/**\n * @var \\App\\Options $options\n */\n@endphp\n@props(['options' => []])\n{{ $options->first() }}\n";
    let (php, _) = preprocess(content);
    assert!(
        !php.contains("$options = [];"),
        "the props default must not shadow the declared type: {}",
        php
    );
}

/// The array literal in `@props(...)` commonly spans multiple lines;
/// the whole argument list must be read, not just the closing line, or
/// every prop declared before the last line is lost.
#[test]
fn test_preprocess_props_directive_spans_multiple_lines() {
    let content = "@props([\n    'caption' => '',\n])\n{{ $caption }}\n";
    let (php, _) = preprocess(content);
    assert!(
        prologue_of(&php).contains("$caption = '';"),
        "a multi-line @props array must still declare its keys: {}",
        php
    );
}

/// A prop with no default (`@props(['visible'])`) is *required*: its
/// value comes from the caller, so it is declared `mixed` rather than
/// being invented as `null`, which would make every use of it a type
/// error against whatever the prop is really passed.
#[test]
fn test_preprocess_props_directive_shorthand_without_default() {
    let content = "@props(['visible'])\n{{ $visible }}\n";
    let (php, _) = preprocess(content);
    assert!(
        prologue_of(&php).contains("/** @var mixed $visible */"),
        "a defaultless prop should be declared mixed: {}",
        php
    );
}

/// `@aware` pulls a value from the parent component, falling back to the
/// declared default, so it types the body exactly as `@props` does.
#[test]
fn test_preprocess_aware_directive_declares_variables() {
    let content = "@aware(['color' => 'gray'])\n{{ $color }}\n";
    let (php, _) = preprocess(content);
    assert!(
        prologue_of(&php).contains("$color = 'gray';"),
        "@aware should declare its keys: {}",
        php
    );
}

/// A dynamic props argument (not a plain array literal) cannot be read,
/// so no variable is invented; the expression still reaches PHP as an
/// inert call so its own variables are seen.
#[test]
fn test_preprocess_props_directive_dynamic_argument_falls_back() {
    let content = "@props($dynamicProps)\n";
    let (php, _) = preprocess(content);
    assert!(
        php.contains("blade_directive ($dynamicProps);"),
        "a non-literal @props argument should fall back to the inert call: {}",
        php
    );
    assert!(
        php.contains("global $errors, $__env;"),
        "a non-literal @props argument declares nothing: {}",
        php
    );
}
