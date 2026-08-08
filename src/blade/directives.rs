pub fn match_directive(s: &str) -> Option<&'static str> {
    let directives = [
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
        "endcomponent",
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
        // Authorization directives
        "canany",
        "cannot",
        "can",
        "elsecanany",
        "elsecannot",
        "elsecan",
        "endcanany",
        "endcannot",
        "endcan",
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
        "hasstack",
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
    ];

    for d in directives {
        if let Some(stripped) = s.strip_prefix(d) {
            let next_char = stripped.chars().next();
            if next_char.is_none() || !next_char.unwrap().is_alphanumeric() {
                return Some(d);
            }
        }
    }
    None
}

pub fn translate_directive(directive: &str) -> String {
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
        // Authorization blocks lower to a synthetic call so the ability
        // string is extracted like the PHP `Gate::allows()` forms.
        "can" | "canany" => "if(blade_can_directive".to_string(),
        "cannot" => "if(!blade_can_directive".to_string(),
        "elsecan" | "elsecanany" => "elseif(blade_can_directive".to_string(),
        "elsecannot" => "elseif(!blade_can_directive".to_string(),
        "isset" => "if(isset".to_string(),
        "empty" => "if(empty".to_string(),
        "break" => "break;".to_string(),
        "default" => "default:".to_string(),
        "extends" | "include" | "includeIf" | "includeWhen" | "includeUnless" | "includeFirst"
        | "component" | "each" => "blade_view_directive".to_string(),
        "section" | "yield" | "push" | "prepend" | "slot" | "aware" | "class" | "style"
        | "checked" | "selected" | "disabled" | "readonly" | "required" | "stack" | "json"
        | "dump" => "blade_directive".to_string(),
        "endsection" | "endpush" | "endprepend" | "endcomponent" | "endslot" | "stop" | "show"
        | "append" | "overwrite" => "".to_string(),
        _ => format!("/* @{directive} */"),
    }
}
