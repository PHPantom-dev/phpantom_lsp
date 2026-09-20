/// Every directive name `match_directive` recognises, in no particular
/// order beyond the loose grouping comments below. [`DIRECTIVE_COMPLETIONS`]
/// is checked against this list (`every_known_directive_has_a_completion`)
/// so the two cannot silently drift apart.
const KNOWN_DIRECTIVES: &[&str] = &[
    "if",
    "elseif",
    "else",
    "endif",
    "foreach",
    "endforeach",
    "forelse",
    "endforelse",
    "for",
    "endfor",
    "while",
    "endwhile",
    "unless",
    "endunless",
    "isset",
    "endisset",
    "empty",
    "endempty",
    "switch",
    "endswitch",
    "case",
    "default",
    "break",
    "php",
    "endphp",
    "use",
    "inject",
    "class",
    "style",
    "checked",
    "selected",
    "disabled",
    "readonly",
    "required",
    "json",
    "dump",
    "extends",
    "extendsFirst",
    "section",
    "endsection",
    "yield",
    "include",
    "includeIf",
    "includeWhen",
    "includeUnless",
    "includeFirst",
    "stack",
    "push",
    "endpush",
    "prepend",
    "endprepend",
    "component",
    "componentFirst",
    "endcomponent",
    "endcomponentFirst",
    "slot",
    "endslot",
    "props",
    "aware",
    "stop",
    "show",
    "append",
    "overwrite",
    // Auth/env directives
    "auth",
    "endauth",
    "guest",
    "endguest",
    "production",
    "endproduction",
    "env",
    "endenv",
    // Session/context directives
    "session",
    "endsession",
    "context",
    "endcontext",
    // Section helpers
    "hasSection",
    "sectionMissing",
    "parent",
    // Include variants
    "includeIsolated",
    "each",
    // Stack directives
    "pushIf",
    "endPushIf",
    "pushOnce",
    "endPushOnce",
    "prependOnce",
    "endPrependOnce",
    "hasStack",
    // Form directives
    "csrf",
    "method",
    "error",
    "enderror",
    // Continuation
    "continue",
    // Misc directives
    "once",
    "endonce",
    "verbatim",
    "endverbatim",
    "fragment",
    "endfragment",
    // Authorization directives
    "can",
    "cannot",
    "canany",
    "elsecan",
    "elsecannot",
    "elsecanany",
    "endcan",
    "endcannot",
    "endcanany",
    // Translation directives
    "lang",
    "endlang",
    "choice",
    // Raw PHP
    "unset",
    // JS/asset helpers
    "js",
    "vite",
    "viteReactRefresh",
    "fonts",
    "dd",
];

pub fn match_directive(s: &str) -> Option<&'static str> {
    for &d in KNOWN_DIRECTIVES {
        if let Some(stripped) = s.strip_prefix(d) {
            let next_char = stripped.chars().next();
            if next_char.is_none() || !next_char.unwrap().is_alphanumeric() {
                return Some(d);
            }
        }
    }
    None
}

/// A directive a service provider registered on top of the ones Blade
/// compiles itself, as the provider scan recorded it (see
/// `crate::virtual_members::laravel::ProviderResources::custom_directives`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomDirective {
    /// The registered name, without the leading `@`.
    pub name: String,
    /// Whether `Blade::if()` registered it rather than `Blade::directive()`.
    /// An `if` registration also gives the template the other three members
    /// of its family (`@unless…`, `@else…`, `@end…`).
    pub conditional: bool,
}

/// What real PHP a custom directive lowers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomForm {
    /// A `Blade::directive()` registration. Its handler returns whatever PHP
    /// it likes, so a marker statement stands in for it and the argument the
    /// template wrote is still type-checked.
    Statement,
    /// The `@admin` or `@unlessadmin` opener of a `Blade::if()` family, which
    /// Blade compiles to an `if (…):` that `@endadmin` closes. The negation
    /// `@unless…` applies is not modelled: the condition stands in for a
    /// callback this scan cannot evaluate either way.
    Open,
    /// The `@elseadmin` of a `Blade::if()` family.
    Else,
    /// The `@endadmin` of a `Blade::if()` family.
    End,
}

/// The marker call a custom directive lowers to.
///
/// Deliberately not the generic `blade_directive`: that marker's calls are
/// counted in document order to pair a component tag's bound attributes with
/// the expressions they pass (`super::component_tags`), so a directive
/// emitting one would shift every pairing after it. Returning `bool` lets
/// the same marker stand inside the condition a `Blade::if()` family
/// compiles to.
pub const CUSTOM_MARKER: &str = "blade_custom_directive";

/// One name a template can write, and what the preprocessor does with it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CustomEntry {
    name: String,
    form: CustomForm,
    /// The `@end…` closing the block a `Blade::if()` opener opens, so
    /// completing the opener can insert the whole pair.
    closer: Option<String>,
}

/// The directives a project's service providers register, expanded so that
/// every name a template can write is one lookup away.
///
/// Empty for a project that registers none, which is the common case and
/// costs the preprocessor nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomDirectives {
    /// Longest name first, so a registration that another name is a prefix
    /// of (`foo` and `foo::bar`) is not shadowed by the shorter one.
    entries: Vec<CustomEntry>,
}

/// A custom directive offered as a directive-name completion candidate.
pub struct CustomDirectiveCompletion<'a> {
    pub name: &'a str,
    /// The text inserted after the `@` the user has already typed, matching
    /// how [`DIRECTIVE_COMPLETIONS`] entries are written.
    pub insert_text: String,
    pub is_snippet: bool,
}

impl CustomDirectives {
    /// Expand the registrations a provider scan found, giving each
    /// `Blade::if()` the four names Blade synthesizes from it.
    pub fn from_registrations(registrations: &[CustomDirective]) -> Self {
        let mut entries: Vec<CustomEntry> = Vec::new();
        let mut record = |name: String, form: CustomForm, closer: Option<String>| {
            match entries.iter_mut().find(|entry| entry.name == name) {
                // Blade holds one handler per name and a later registration
                // replaces the one before it.
                Some(entry) => {
                    entry.form = form;
                    entry.closer = closer;
                }
                None => entries.push(CustomEntry { name, form, closer }),
            }
        };

        for registration in registrations {
            let name = registration.name.as_str();
            if !is_directive_name(name) {
                continue;
            }
            if registration.conditional {
                let closer = format!("end{name}");
                record(name.to_string(), CustomForm::Open, Some(closer.clone()));
                record(
                    format!("unless{name}"),
                    CustomForm::Open,
                    Some(closer.clone()),
                );
                record(format!("else{name}"), CustomForm::Else, None);
                record(closer, CustomForm::End, None);
            } else {
                record(name.to_string(), CustomForm::Statement, None);
            }
        }

        entries.sort_unstable_by(|a, b| {
            b.name
                .len()
                .cmp(&a.name.len())
                .then_with(|| a.name.cmp(&b.name))
        });
        Self { entries }
    }

    /// The custom directive the text right after an `@` names, and how it
    /// lowers.
    pub fn match_directive(&self, after_at: &str) -> Option<(&str, CustomForm)> {
        self.entries.iter().find_map(|entry| {
            let rest = after_at.strip_prefix(entry.name.as_str())?;
            // Blade reads a directive name as `\w+`, so a registered name
            // glued to further word characters is a different directive
            // that nobody registered.
            rest.chars()
                .next()
                .is_none_or(|next| !is_name_char(next))
                .then_some((entry.name.as_str(), entry.form))
        })
    }

    /// Every custom directive a template can write, as completion
    /// candidates.
    ///
    /// A `Blade::if()` family has a shape Blade itself guarantees, so its
    /// opener inserts the whole block. A `Blade::directive()` handler's
    /// arity is its own business, so its name is inserted bare rather than
    /// an argument list being invented for it.
    pub fn completions(&self) -> impl Iterator<Item = CustomDirectiveCompletion<'_>> {
        self.entries.iter().map(|entry| match &entry.closer {
            Some(closer) => CustomDirectiveCompletion {
                name: &entry.name,
                insert_text: format!("{}\n\t$0\n@{closer}", entry.name),
                is_snippet: true,
            },
            None => CustomDirectiveCompletion {
                name: &entry.name,
                insert_text: entry.name.clone(),
                is_snippet: false,
            },
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Whether `name` is one Blade accepts: `\w+`, optionally with a
/// `::`-separated second segment. `BladeCompiler::directive()` throws on
/// anything else, so a registration this rejects never reaches a template.
fn is_directive_name(name: &str) -> bool {
    let segment_ok = |segment: &str| !segment.is_empty() && segment.chars().all(is_name_char);
    let mut segments = name.split("::");
    segments.next().is_some_and(segment_ok)
        && segments.next().is_none_or(segment_ok)
        && segments.next().is_none()
}

/// Whether `ch` is one of the characters PHP's `\w` matches, which is what
/// Blade validates a directive name against.
fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// A directive-name completion candidate offered when `@` is typed in an
/// HTML/directive position of a Blade template.
///
/// `insert_text` never includes the leading `@` — the trigger character is
/// already in the buffer by the time the completion request fires, so only
/// the rest is inserted, mirroring how PHPDoc tag completion inserts text
/// after an already-typed `@` (`src/completion/phpdoc/mod.rs`).
pub struct DirectiveCompletion {
    pub name: &'static str,
    pub insert_text: &'static str,
    pub is_snippet: bool,
}

/// One completion entry per name in [`match_directive`]'s list, verified
/// against Laravel's own compiler (`Illuminate\View\Compilers\Concerns\*`).
/// A directive that opens a block inserts the whole matching pair with a
/// `$0` exit tab stop, the way Blade authors write it by hand; a directive
/// with no arguments inserts just its own name.
///
/// Kept in sync with `match_directive` by
/// `every_known_directive_has_a_completion` below rather than sharing a
/// single array, so a plain `&[&str]` (used on every preprocessor
/// character) doesn't have to carry unused snippet payloads.
pub const DIRECTIVE_COMPLETIONS: &[DirectiveCompletion] = &[
    DirectiveCompletion {
        name: "if",
        insert_text: "if ($1)\n\t$0\n@endif",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "elseif",
        insert_text: "elseif ($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "else",
        insert_text: "else",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "endif",
        insert_text: "endif",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "foreach",
        insert_text: "foreach ($1 as $2)\n\t$0\n@endforeach",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endforeach",
        insert_text: "endforeach",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "forelse",
        insert_text: "forelse ($1 as $2)\n\t$0\n@empty\n\t\n@endforelse",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endforelse",
        insert_text: "endforelse",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "for",
        insert_text: "for ($1)\n\t$0\n@endfor",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endfor",
        insert_text: "endfor",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "while",
        insert_text: "while ($1)\n\t$0\n@endwhile",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endwhile",
        insert_text: "endwhile",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "unless",
        insert_text: "unless ($1)\n\t$0\n@endunless",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endunless",
        insert_text: "endunless",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "isset",
        insert_text: "isset ($1)\n\t$0\n@endisset",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endisset",
        insert_text: "endisset",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "empty",
        insert_text: "empty ($1)\n\t$0\n@endempty",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endempty",
        insert_text: "endempty",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "switch",
        insert_text: "switch ($1)\n\t@case($2)\n\t\t$0\n\t\t@break\n\n\t@default\n\t\t\n@endswitch",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endswitch",
        insert_text: "endswitch",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "case",
        insert_text: "case($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "default",
        insert_text: "default",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "break",
        insert_text: "break",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "php",
        insert_text: "php\n$0\n@endphp",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endphp",
        insert_text: "endphp",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "use",
        insert_text: "use('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "inject",
        insert_text: "inject('$1', '$2')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "class",
        insert_text: "class([$1])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "style",
        insert_text: "style([$1])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "checked",
        insert_text: "checked($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "selected",
        insert_text: "selected($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "disabled",
        insert_text: "disabled($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "readonly",
        insert_text: "readonly($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "required",
        insert_text: "required($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "json",
        insert_text: "json($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "dump",
        insert_text: "dump($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "extends",
        insert_text: "extends('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "extendsFirst",
        insert_text: "extendsFirst(['$1'])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "section",
        insert_text: "section('$1')\n\t$0\n@endsection",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endsection",
        insert_text: "endsection",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "yield",
        insert_text: "yield('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "include",
        insert_text: "include('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "includeIf",
        insert_text: "includeIf('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "includeWhen",
        insert_text: "includeWhen($1, '$2')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "includeUnless",
        insert_text: "includeUnless($1, '$2')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "includeFirst",
        insert_text: "includeFirst(['$1'])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "stack",
        insert_text: "stack('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "push",
        insert_text: "push('$1')\n\t$0\n@endpush",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endpush",
        insert_text: "endpush",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "prepend",
        insert_text: "prepend('$1')\n\t$0\n@endprepend",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endprepend",
        insert_text: "endprepend",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "component",
        insert_text: "component('$1')\n\t$0\n@endcomponent",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "componentFirst",
        insert_text: "componentFirst(['$1'])\n\t$0\n@endcomponentFirst",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endcomponent",
        insert_text: "endcomponent",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "endcomponentFirst",
        insert_text: "endcomponentFirst",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "slot",
        insert_text: "slot('$1')\n\t$0\n@endslot",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endslot",
        insert_text: "endslot",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "props",
        insert_text: "props([$1])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "aware",
        insert_text: "aware([$1])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "stop",
        insert_text: "stop",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "show",
        insert_text: "show",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "append",
        insert_text: "append",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "overwrite",
        insert_text: "overwrite",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "auth",
        insert_text: "auth\n\t$0\n@endauth",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endauth",
        insert_text: "endauth",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "guest",
        insert_text: "guest\n\t$0\n@endguest",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endguest",
        insert_text: "endguest",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "production",
        insert_text: "production\n\t$0\n@endproduction",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endproduction",
        insert_text: "endproduction",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "env",
        insert_text: "env('$1')\n\t$0\n@endenv",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endenv",
        insert_text: "endenv",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "session",
        insert_text: "session('$1')\n\t$0\n@endsession",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endsession",
        insert_text: "endsession",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "context",
        insert_text: "context('$1')\n\t$0\n@endcontext",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endcontext",
        insert_text: "endcontext",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "hasSection",
        insert_text: "hasSection('$1')\n\t$0\n@endif",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "sectionMissing",
        insert_text: "sectionMissing('$1')\n\t$0\n@endif",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "parent",
        insert_text: "parent",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "includeIsolated",
        insert_text: "includeIsolated('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "each",
        insert_text: "each('$1', $2, '$3')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "pushIf",
        insert_text: "pushIf($1, '$2')\n\t$0\n@endPushIf",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endPushIf",
        insert_text: "endPushIf",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "pushOnce",
        insert_text: "pushOnce('$1')\n\t$0\n@endPushOnce",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endPushOnce",
        insert_text: "endPushOnce",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "prependOnce",
        insert_text: "prependOnce('$1')\n\t$0\n@endPrependOnce",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endPrependOnce",
        insert_text: "endPrependOnce",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "hasStack",
        insert_text: "hasStack('$1')\n\t$0\n@endif",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "csrf",
        insert_text: "csrf",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "method",
        insert_text: "method('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "error",
        insert_text: "error('$1')\n\t$0\n@enderror",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "enderror",
        insert_text: "enderror",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "continue",
        insert_text: "continue",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "once",
        insert_text: "once\n\t$0\n@endonce",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endonce",
        insert_text: "endonce",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "verbatim",
        insert_text: "verbatim\n\t$0\n@endverbatim",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endverbatim",
        insert_text: "endverbatim",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "fragment",
        insert_text: "fragment('$1')\n\t$0\n@endfragment",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endfragment",
        insert_text: "endfragment",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "can",
        insert_text: "can('$1')\n\t$0\n@endcan",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "cannot",
        insert_text: "cannot('$1')\n\t$0\n@endcannot",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "canany",
        insert_text: "canany(['$1'])\n\t$0\n@endcanany",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "elsecan",
        insert_text: "elsecan('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "elsecannot",
        insert_text: "elsecannot('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "elsecanany",
        insert_text: "elsecanany(['$1'])",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endcan",
        insert_text: "endcan",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "endcannot",
        insert_text: "endcannot",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "endcanany",
        insert_text: "endcanany",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "lang",
        insert_text: "lang\n\t$0\n@endlang",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "endlang",
        insert_text: "endlang",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "choice",
        insert_text: "choice('$1', $2)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "unset",
        insert_text: "unset($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "js",
        insert_text: "js($1)",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "vite",
        insert_text: "vite('$1')",
        is_snippet: true,
    },
    DirectiveCompletion {
        name: "viteReactRefresh",
        insert_text: "viteReactRefresh",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "fonts",
        insert_text: "fonts",
        is_snippet: false,
    },
    DirectiveCompletion {
        name: "dd",
        insert_text: "dd($1)",
        is_snippet: true,
    },
];

pub fn translate_directive(directive: &str) -> String {
    // A section or stack name is a cross-file key, so these lower to
    // markers of their own for symbol extraction to recognise them by (see
    // `crate::blade::blocks`).  `@hasSection` and its cousins compile to a
    // condition, and Laravel always follows them with an `@endif`, so the
    // marker opens a real `if` for that `@endif` to close.
    if let Some(entry) = super::blocks::named_block_directive(directive) {
        let marker = entry.marker();
        return if entry.opens_condition() {
            format!("if ({marker}")
        } else {
            marker.to_string()
        };
    }
    match directive {
        "php" | "endphp" => "".to_string(),
        "if" | "elseif" | "foreach" | "for" | "while" | "switch" | "case" => directive.to_string(),
        "forelse" => "foreach".to_string(),
        "unless" => "if(!".to_string(),
        "else" => "else:".to_string(),
        "endif" | "endforeach" | "endfor" | "endwhile" | "endunless" | "endisset" | "endempty"
        | "endswitch" | "endforelse" | "endsession" | "endcontext" | "enderror" | "endauth"
        | "endguest" | "endproduction" | "endenv" | "endonce" | "endcan" | "endcannot"
        | "endcanany" => {
            let mapped = match directive {
                "endunless" | "endisset" | "endempty" | "endsession" | "endcontext"
                | "enderror" | "endauth" | "endguest" | "endproduction" | "endenv" | "endonce"
                | "endcan" | "endcannot" | "endcanany" => "endif",
                "endforelse" => "endif",
                other => other,
            };
            format!("{mapped};")
        }
        "isset" => "if(isset".to_string(),
        "empty" => "if(empty".to_string(),
        "break" => "break;".to_string(),
        "default" => "default:".to_string(),
        "extends" | "extendsFirst" | "include" | "includeIf" | "includeWhen" | "includeUnless"
        | "includeFirst" | "component" | "componentFirst" => "blade_view_directive".to_string(),
        // `@each` renders its partial once per entry of a collection, with
        // only the item and the key in scope.  The arguments after the view
        // name therefore mean something entirely different from every other
        // render directive's data array, so it gets a marker of its own.
        "each" => "blade_each_directive".to_string(),
        "slot" | "props" | "aware" | "class" | "style" | "checked" | "selected" | "disabled"
        | "readonly" | "required" | "json" | "dump" | "lang" | "choice" | "js" | "vite"
        | "fonts" | "dd" => "blade_directive".to_string(),
        // `unset(...)` is a language construct, not a function — it cannot
        // be passed as an argument to `blade_directive(...)`, so it keeps
        // its own real name instead of the generic marker.
        "unset" => "unset".to_string(),
        // `@can`/`@cannot`/`@canany` (and their `@hasStack`/`@hasSection`/
        // `@sectionMissing` cousins) are conditionals that Laravel compiles
        // to a real gate/environment method call inside `if (...):`. A
        // marker call stands in for it rather than a hard-coded
        // `\Illuminate\Contracts\Auth\Access\Gate` reference that may not
        // exist in every project's autoload map — the point is to keep the
        // arguments' expressions real PHP that gets type-checked, and to
        // open a genuine `if` so the `@endif` that always follows these
        // stays balanced.
        //
        // The authorization three get a marker of their own rather than
        // sharing `blade_directive` with everything else: their first
        // argument is an ability name, and symbol extraction recognises it
        // by the callee it is passed to.
        "can" | "cannot" | "canany" => "if (blade_can_directive".to_string(),
        "elsecan" | "elsecannot" | "elsecanany" => "elseif (blade_can_directive".to_string(),
        "endsection" | "endpush" | "endprepend" | "endcomponent" | "endcomponentFirst"
        | "endslot" | "stop" | "show" | "append" | "overwrite" => "".to_string(),
        _ => format!("/* @{directive} */"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`DIRECTIVE_COMPLETIONS`] is a separate table from [`KNOWN_DIRECTIVES`]
    /// (a plain `&[&str]` on the preprocessor's hot path doesn't need to carry
    /// unused snippet payloads), so nothing at compile time stops the two
    /// from drifting apart. This test is that guard: every directive
    /// `match_directive` recognises must have exactly one completion entry,
    /// and no completion entry may name a directive that doesn't exist.
    #[test]
    fn every_known_directive_has_exactly_one_completion() {
        let mut known: Vec<&str> = KNOWN_DIRECTIVES.to_vec();
        known.sort_unstable();
        known.dedup();
        assert_eq!(
            known.len(),
            KNOWN_DIRECTIVES.len(),
            "KNOWN_DIRECTIVES contains a duplicate name"
        );

        let mut completions: Vec<&str> = DIRECTIVE_COMPLETIONS.iter().map(|c| c.name).collect();
        completions.sort_unstable();
        completions.dedup();
        assert_eq!(
            completions.len(),
            DIRECTIVE_COMPLETIONS.len(),
            "DIRECTIVE_COMPLETIONS contains a duplicate name"
        );

        assert_eq!(
            known, completions,
            "DIRECTIVE_COMPLETIONS must have exactly one entry per known directive"
        );
    }

    /// Every directive `match_directive` recognises by itself (with nothing
    /// after it) must round-trip: recognising the directive by name and then
    /// re-matching that exact name must yield the same directive back. This
    /// guards against a prefix collision (a short name shadowing a longer
    /// one earlier in the list) silently truncating recognition.
    #[test]
    fn every_known_directive_matches_its_own_bare_name() {
        for &name in KNOWN_DIRECTIVES {
            assert_eq!(
                match_directive(name),
                Some(name),
                "directive {name:?} did not match its own bare name"
            );
        }
    }

    /// A directive that names a section or a stack has to lower to the
    /// marker call its table entry names, since symbol extraction reads
    /// the name off that callee and nothing else links the two tables.
    #[test]
    fn every_named_block_directive_lowers_to_its_own_marker() {
        for entry in crate::blade::blocks::NAMED_BLOCK_DIRECTIVES {
            let translated = translate_directive(entry.name);
            assert!(
                translated.contains(entry.marker()),
                "@{} lowers to {translated:?}, which does not call {}",
                entry.name,
                entry.marker()
            );
        }
    }

    fn custom(names: &[(&str, bool)]) -> CustomDirectives {
        let registrations: Vec<CustomDirective> = names
            .iter()
            .map(|(name, conditional)| CustomDirective {
                name: name.to_string(),
                conditional: *conditional,
            })
            .collect();
        CustomDirectives::from_registrations(&registrations)
    }

    /// `Blade::if('admin')` registers four directives, not one.
    #[test]
    fn a_condition_registration_expands_to_its_whole_family() {
        let directives = custom(&[("admin", true)]);
        for (written, expected) in [
            ("admin", CustomForm::Open),
            ("unlessadmin", CustomForm::Open),
            ("elseadmin", CustomForm::Else),
            ("endadmin", CustomForm::End),
        ] {
            assert_eq!(
                directives.match_directive(written),
                Some((written, expected)),
                "@{written} did not resolve to {expected:?}"
            );
        }
    }

    #[test]
    fn a_plain_registration_is_a_statement() {
        let directives = custom(&[("datetime", false)]);
        assert_eq!(
            directives.match_directive("datetime($post->createdAt)"),
            Some(("datetime", CustomForm::Statement))
        );
        // A name that merely starts with a registered one is a different
        // directive, exactly as Blade's own `\w+` name pattern reads it.
        assert_eq!(directives.match_directive("datetimezone"), None);
        assert_eq!(directives.match_directive("datetime_utc"), None);
    }

    /// A registered name another registration is a prefix of still wins for
    /// the text that spells it out, since `::` ends a name as surely as a
    /// space does.
    #[test]
    fn the_longest_registered_name_wins() {
        let directives = custom(&[("foo", false), ("foo::bar", false)]);
        assert_eq!(
            directives.match_directive("foo::bar"),
            Some(("foo::bar", CustomForm::Statement))
        );
        assert_eq!(
            directives.match_directive("foo"),
            Some(("foo", CustomForm::Statement))
        );
    }

    /// `BladeCompiler::directive()` throws on a name that is not `\w+`
    /// (optionally with a `::` segment), so such a registration can never
    /// produce a directive a template writes.
    #[test]
    fn a_name_blade_would_reject_registers_nothing() {
        for name in ["", "has space", "dash-ed", "a::b::c", "trailing::"] {
            assert!(
                custom(&[(name, false)]).is_empty(),
                "{name:?} was accepted as a directive name"
            );
        }
    }

    /// The opener of a condition family completes to the whole block; a
    /// plain registration completes to its bare name, since nothing says
    /// what arguments its handler takes.
    #[test]
    fn completions_insert_what_the_registration_guarantees() {
        let directives = custom(&[("admin", true), ("datetime", false)]);
        let mut inserted: Vec<(String, String, bool)> = directives
            .completions()
            .map(|c| (c.name.to_string(), c.insert_text, c.is_snippet))
            .collect();
        inserted.sort();
        assert_eq!(
            inserted,
            vec![
                (
                    "admin".to_string(),
                    "admin\n\t$0\n@endadmin".to_string(),
                    true
                ),
                ("datetime".to_string(), "datetime".to_string(), false),
                ("elseadmin".to_string(), "elseadmin".to_string(), false),
                ("endadmin".to_string(), "endadmin".to_string(), false),
                (
                    "unlessadmin".to_string(),
                    "unlessadmin\n\t$0\n@endadmin".to_string(),
                    true
                ),
            ]
        );
    }

    /// Every completion's insert text must actually start with the
    /// directive's own name (allowing for the `$`-prefixed tab-stop syntax
    /// only after it), since a client replaces exactly the region right
    /// after the `@` the user already typed — the label promises the
    /// directive named, so the text must open with it.
    #[test]
    fn every_completion_insert_text_starts_with_its_own_name() {
        for completion in DIRECTIVE_COMPLETIONS {
            assert!(
                completion.insert_text.starts_with(completion.name),
                "completion for {:?} inserts {:?}, which doesn't start with the directive name",
                completion.name,
                completion.insert_text
            );
        }
    }
}
