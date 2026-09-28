use super::{ComponentBinding, ComponentParameter, ComponentResolver, ComponentTarget};

use crate::blade::component_tags::{self, TagKind, camel_case_attr_name};

/// A `<x-…>` / `<livewire:…>` tag opening (or its closing counterpart)
/// found at the current scan position.
pub(super) struct ComponentTag {
    /// Characters from the `<` up to (not including) the first character
    /// after the tag name — the attribute list is left to the HTML
    /// scanner.
    pub(super) len: usize,
    /// Which index resolves [`Self::name`].
    kind: TagKind,
    /// The name as written, without the prefix (`alert`, `forms.input`,
    /// `pkg::calendar`, `counter`).
    name: String,
    closing: bool,
}

impl ComponentTag {
    /// The class this tag names, or `None` for a closing tag, one Blade
    /// claims for itself, or one no index answers for.
    pub(super) fn resolve(
        &self,
        components: Option<&dyn ComponentResolver>,
    ) -> Option<ComponentTarget> {
        if self.closing || self.is_reserved() {
            return None;
        }
        let resolver = components?;
        match self.kind {
            TagKind::Livewire => resolver.livewire_component(&self.name),
            TagKind::Blade => resolver.x_component(&self.name),
        }
    }

    /// The PHP this tag becomes, and the call its attributes fill.
    ///
    /// A tag whose class resolves binds `$component` to it, the variable
    /// Blade's own compiled output uses, so `$component->` inside the tag
    /// body carries the component's members.  When the class also has a
    /// signature the attributes are arguments to, the whole call is
    /// emitted where the tag *closes*, since everything between the tag's
    /// name and its `>` is markup the scanner turns into statements — a
    /// `{{ }}` echo, a directive, a bound attribute the attribute bag
    /// takes — and none of those can sit in an argument list.  A bound
    /// attribute that is an argument is bound to a variable where it is
    /// written, so hovering the expression still lands on the template's
    /// own text, and the call passes that variable.
    ///
    /// Everything else (a closing tag, a component no index knows,
    /// `<x-dynamic-component>`, a `<x-slot>`) becomes a comment naming the
    /// tag: a bound attribute's expression is emitted by the HTML scanner
    /// either way, so nothing the type engine could use is lost.
    ///
    /// `rest` is the remainder of the line after the tag name and
    /// `following` the lines after it, which the attribute list may run
    /// into.
    pub(super) fn emit(
        &self,
        target: Option<ComponentTarget>,
        rest: &[char],
        following: &[&str],
    ) -> (String, Option<OpenComponentCall>) {
        let Some(target) = target else {
            return (
                format!(
                    " /* {slash}{prefix}{name} */ ",
                    slash = if self.closing { "/" } else { "" },
                    prefix = self.kind.prefix(),
                    name = self.name,
                ),
                None,
            );
        };
        let fqn = target.fqn.trim_matches('\\');
        let var = crate::blade::COMPONENT_VAR;
        let (parameters, mount) = match &target.binding {
            ComponentBinding::Construct(parameters) => (parameters, false),
            ComponentBinding::Mount(parameters) => (parameters, true),
            // A component whose attributes are arguments to nothing (an
            // anonymous one, whose attributes are its *view's* variables)
            // just declares the variable.
            ComponentBinding::Declare => {
                return (format!(" /** @var \\{fqn} ${var} */ ${var} = null; "), None);
            }
        };
        // The plain attributes have to be read ahead: their markup stays
        // masked where it is written, so there is nowhere to emit them but
        // the call itself. A tag whose `>` is nowhere to be found has no
        // call to emit at all.
        let Some(literals) = tag_attribute_arguments(rest, following) else {
            return (format!(" /** @var \\{fqn} ${var} */ ${var} = null; "), None);
        };

        let call = OpenComponentCall {
            fqn: fqn.to_string(),
            mount,
            pending: parameters.clone(),
            literals,
            arguments: Vec::new(),
        };
        (String::new(), Some(call))
    }

    /// Whether the tag name is one Blade's compiler claims for itself
    /// rather than looking up as a component: `<x-slot>` / `<x-slot:name>`
    /// open a slot on the surrounding component, and
    /// `<x-dynamic-component>` names its target through a `:component`
    /// attribute the surrounding scan already emits.  A project component
    /// that happened to share one of those names would never be reached
    /// by the tag anyway.
    fn is_reserved(&self) -> bool {
        self.kind == TagKind::Blade
            && matches!(
                self.name
                    .split_once(':')
                    .map_or(self.name.as_str(), |(head, _)| head),
                "slot" | "dynamic-component"
            )
    }
}

/// The prefix of the variables a component tag's bound attributes are
/// bound to before the call that consumes them.
///
/// They are the preprocessor's own, so they are exempt from the
/// unused-variable diagnostic the way `$loop` and `$component` are.
pub const ARGUMENT_VAR_PREFIX: &str = "__blade_arg_";

/// The call a resolved component tag makes, held between the tag's name
/// and the `>` that closes it.
pub(super) struct OpenComponentCall {
    /// Fully qualified class name, without a leading `\`.
    fqn: String,
    /// Whether the attributes are `mount()`'s arguments rather than the
    /// constructor's.
    mount: bool,
    /// The parameters no attribute has filled yet, in declaration order.
    pending: Vec<ComponentParameter>,
    /// The tag's plain attributes as `(camelCase name, PHP expression)`,
    /// read ahead when the tag opened, since their markup stays masked
    /// where it is written.
    literals: Vec<(String, String)>,
    /// The `name: value` arguments settled so far, in the order the
    /// attributes filling them were written.
    arguments: Vec<String>,
}

impl OpenComponentCall {
    /// Claim the parameter a bound attribute named `attr` fills, and
    /// return the variable its expression is bound to. `None` when the
    /// attribute names no parameter — Laravel routes that one to the
    /// component's attribute bag instead.
    ///
    /// A bound attribute claims ahead of a plain one of the same name
    /// (which is duplicate markup either way), so that this and the scan
    /// in [`crate::blade::component_tags`] agree on which attributes are
    /// arguments without either having to know the order the other saw
    /// them in.
    pub(super) fn take(&mut self, attr: &str) -> Option<String> {
        let index = self.pending.iter().position(|param| param.name == attr)?;
        let param = self.pending.remove(index);
        let variable = format!("${ARGUMENT_VAR_PREFIX}{}", param.name);
        self.arguments.push(format!("{}: {variable}", param.name));
        Some(variable)
    }

    /// The whole call: the arguments the tag's attributes settled, then
    /// what Laravel itself would pass for a parameter no attribute
    /// filled.
    pub(super) fn close(mut self) -> String {
        for (name, value) in std::mem::take(&mut self.literals) {
            if self.pending.iter().any(|param| param.name == name) {
                self.pending.retain(|param| param.name != name);
                self.arguments.push(format!("{name}: {value}"));
            }
        }
        for param in &self.pending {
            if let Some(fallback) = &param.fallback {
                self.arguments.push(format!("{}: {fallback}", param.name));
            }
        }
        let var = crate::blade::COMPONENT_VAR;
        let arguments = self.arguments.join(", ");
        if self.mount {
            format!(
                " ${var} = new \\{}(); ${var}->mount({arguments}); ",
                self.fqn
            )
        } else {
            format!(" ${var} = new \\{}({arguments}); ", self.fqn)
        }
    }
}

/// How many lines past the one a tag opens on its attribute list may run
/// before the call it makes is given up on.
const MAX_TAG_LOOKAHEAD_LINES: usize = 64;

/// The plain attributes of the tag whose name ends at `rest`, as
/// `(camelCase name, PHP expression)`, or `None` when the tag's `>` is
/// nowhere to be found and there is therefore nowhere to emit its call.
///
/// Bound attributes (`:name="$expr"`, `:$name`) are left out: their
/// expression is emitted where it is written, so that hovering it still
/// lands on the template's own text.
fn tag_attribute_arguments(rest: &[char], following: &[&str]) -> Option<Vec<(String, String)>> {
    let mut text: String = rest.iter().collect();
    for line in following.iter().take(MAX_TAG_LOOKAHEAD_LINES) {
        text.push('\n');
        text.push_str(line);
    }

    let lexed = component_tags::lex_tag_attributes(&text, 0);
    if !lexed.closed {
        // The tag never closes (or a value never does, which eats the
        // `>`): the tag is malformed and its call cannot be placed.
        return None;
    }
    let mut attributes = Vec::new();
    for attr in lexed.attributes {
        if attr.bound {
            continue;
        }
        let name = camel_case_attr_name(&text[attr.name]);
        let value = match attr.value {
            // A bare attribute is `true`.
            None => "true".to_string(),
            Some(value) => {
                let value = &text[value];
                // An attribute value carrying an echo is whatever the echo
                // renders concatenated with the text around it, which is a
                // string and nothing more precise. The echo's own expression
                // is emitted where it is written, as it is on any other tag.
                if value.contains("{{") || value.contains("{!!") {
                    "(string) ''".to_string()
                } else {
                    php_string_literal(value)
                }
            }
        };
        attributes.push((name, value));
    }
    Some(attributes)
}

/// `text` as a single-quoted PHP string literal.
fn php_string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        if ch == '\'' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// The component tag `rem` opens, or `None` when `rem` is not one.
pub(super) fn component_tag_at(rem: &[char]) -> Option<ComponentTag> {
    if rem.first() != Some(&'<') {
        return None;
    }
    let closing = rem.get(1) == Some(&'/');
    let after_angle = 1 + usize::from(closing);
    for kind in [TagKind::Blade, TagKind::Livewire] {
        let prefix = kind.prefix();
        if !starts_with_ascii(&rem[after_angle..], prefix) {
            continue;
        }
        let name_start = after_angle + prefix.len();
        let mut end = name_start;
        while end < rem.len() && component_tags::is_tag_name_char(rem[end]) {
            end += 1;
        }
        if end == name_start {
            return None;
        }
        return Some(ComponentTag {
            len: end,
            kind,
            name: rem[name_start..end].iter().collect(),
            closing,
        });
    }
    None
}

/// Whether `chars` opens with the ASCII `prefix`, without allocating: the
/// HTML scan asks this of every `<` in the template.
fn starts_with_ascii(chars: &[char], prefix: &str) -> bool {
    chars.len() >= prefix.len()
        && chars
            .iter()
            .zip(prefix.bytes())
            .all(|(ch, byte)| *ch == byte as char)
}

/// If `rem` (starting at a `:`) opens a `:name="` or `:name='` bound
/// attribute, return the length (in chars) of that opening span, up to and
/// including the opening quote. Returns `None` when the syntax does not
/// match, so the `:` is left as ordinary masked tag markup.
pub(super) fn bound_attr_open_len(rem: &[char]) -> Option<usize> {
    // rem[0] is the ':'.
    let mut i = 1;
    let name_start = i;
    while i < rem.len() && (rem[i].is_ascii_alphanumeric() || matches!(rem[i], '_' | '-' | '.')) {
        i += 1;
    }
    if i == name_start {
        return None; // no attribute name after the colon
    }
    if rem.get(i) != Some(&'=') {
        return None;
    }
    i += 1;
    match rem.get(i) {
        Some('"') | Some('\'') => Some(i + 1),
        _ => None,
    }
}

/// Whether `needle` occurs anywhere in `haystack`.
pub(super) fn contains_seq(haystack: &[char], needle: &[char]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether a bound attribute delimited by `quote` closes on a line after
/// the one it opens on. `rest` is the remainder of the opening line (after
/// the opening quote) and `following` the lines after it.
///
/// `false` covers both the single-line case and a malformed attribute whose
/// closing quote never appears, so the caller closes the expression at end
/// of line in either case. A malformed attribute can still pick up a quote
/// from further down the template, but that markup is already broken.
pub(super) fn bound_attr_spans_lines(quote: char, rest: &[char], following: &[&str]) -> bool {
    let mut in_string = None;
    let mut is_escaped = false;
    if scan_to_bound_attr_end(quote, rest.iter().copied(), &mut in_string, &mut is_escaped) {
        return false;
    }
    following
        .iter()
        .any(|line| scan_to_bound_attr_end(quote, line.chars(), &mut in_string, &mut is_escaped))
}

/// Scan one line's worth of a bound-attribute expression, reporting whether
/// the closing `quote` was reached. `in_string` and `is_escaped` carry the
/// PHP string state into the next line and must mirror how the main scan
/// tracks it, or the two disagree about where the attribute ends.
fn scan_to_bound_attr_end(
    quote: char,
    chars: impl Iterator<Item = char>,
    in_string: &mut Option<char>,
    is_escaped: &mut bool,
) -> bool {
    for ch in chars {
        match *in_string {
            _ if ch == quote && in_string.is_none() => return true,
            Some(delim) => {
                if *is_escaped {
                    *is_escaped = false;
                } else if ch == '\\' {
                    *is_escaped = true;
                } else if ch == delim {
                    *in_string = None;
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    *in_string = Some(ch);
                }
            }
        }
    }
    false
}
