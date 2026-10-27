//! Symfony service ID, container parameter, route name, translation, Twig
//! template, form field, bundle configuration key, event name, and
//! Messenger bus completion.
//!
//! In PHP the cursor's call is read from the raw buffer, which is mid-edit
//! and usually does not parse, and is judged by the same predicates the
//! symbol map uses (`symfony_receiver_method`, the configurator functions,
//! and the attributes), so completion and navigation agree on which strings
//! name what.  A method call's receiver is resolved by the type engine, as
//! the symbol map's is.
//!
//! In YAML and XML configuration the line before the cursor says it: an `@`
//! starts a service reference, an unclosed `%` a parameter placeholder, and
//! a handful of XML attributes take a service ID.  An event or a bus is
//! read by the same scan of the file the symbol map makes (see
//! [`crate::symfony::tags`] and [`crate::symfony::messenger`]).  In a Twig template, the
//! first argument of `path()` and `url()` is a route name, a string the
//! `trans` filter is applied to a translation key, and the name an
//! `extends`, an `include`, or another tag that loads a template takes a
//! template name.
//!
//! A form field is completed from its form type's data class, by the same
//! reading of the form type the symbol map makes (see
//! [`crate::symfony::forms`]), and a configuration key in
//! `config/packages/` from the keys declared under the one it is nested in
//! (see [`crate::symfony::config_tree`]).
//!
//! A translation key is completed from the domain it is looked up in.  In
//! PHP that is read from the symbol map, whose site for the string being
//! edited records it, and is the default domain while the call does not
//! parse yet.

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::eloquent_string::detect_string_call_context;
use crate::completion::laravel_string_keys::{
    ArgumentSlot, attribute_class_start, scalar_argument_at, string_key_item_kind,
    string_key_response,
};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::extraction::{
    configurator_function_kind, is_autowire_attribute, is_bus_name_stamp,
    is_event_listener_attribute, is_message_handler_attribute, is_template_attribute,
    is_translatable_message, is_translate_function, symfony_receiver_method,
};
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::symfony::translations::{DEFAULT_DOMAIN, split_translation_key};
use crate::text_position::position_to_offset;
use crate::types::{AccessKind, FileContext};

impl Backend {
    /// Complete the service ID, parameter, route name, translation key or
    /// domain, or template a PHP string literal is being typed as.
    pub(crate) fn try_symfony_string_key_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let literal = content.get(literal_start..cursor)?;

        if self.is_symfony_project()
            && let Some(response) =
                self.symfony_form_field_completion(content, literal_start, cursor, position, ctx)
        {
            return Some(response);
        }

        let (kind, prefix_start) = self
            .symfony_call_key(content, cursor, code, ctx)
            .map(|kind| (kind, literal_start))
            .or_else(|| attribute_key(content, code, literal, literal_start, ctx))
            .filter(|(kind, _)| self.string_key_kind_enabled(*kind))?;
        if kind == StringKeyKind::YiiRouteArgument {
            let arguments = handler_at(ctx, literal_start as u32)
                .and_then(|handler| self.yii_handler_arguments(&handler))
                .unwrap_or_default();
            return self.symfony_names_response(
                kind,
                arguments,
                content,
                prefix_start,
                cursor,
                position,
            );
        }
        if kind == StringKeyKind::SymfonyTranslation {
            let domain = self.translation_domain_at(uri, content, literal_start);
            let messages = self.symfony_translation_messages(&domain);
            return self.symfony_names_response(
                kind,
                messages,
                content,
                prefix_start,
                cursor,
                position,
            );
        }
        self.symfony_key_response(uri, kind, content, prefix_start, cursor, position)
    }

    /// Complete a form field from the properties of the form type's data
    /// class, when the string starting at `literal_start` is one the form
    /// type's `buildForm()` names a field by (see
    /// [`crate::symfony::forms`]).
    fn symfony_form_field_completion(
        &self,
        content: &str,
        literal_start: usize,
        cursor: usize,
        position: Position,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        // Only a form type is worth parsing the buffer for.
        if !content.contains("buildForm") {
            return None;
        }
        let start = literal_start as u32;
        let data_class =
            crate::parser::with_parsed_program(content, "symfony_form_fields", |program, _| {
                let class = class_at(program.statements.iter(), start)?;
                let resolve = |written: &str, offset: u32| ctx.resolve_name_at(written, offset);
                let form = crate::symfony::forms::form_fields(class, &resolve)?;
                form.fields
                    .iter()
                    .any(|(field_start, field_end)| {
                        *field_start == start && cursor as u32 <= *field_end
                    })
                    .then(|| resolve(form.data_class.0, form.data_class.1))
            })?;

        let loaders = self.file_loaders(ctx);
        let class = (loaders.resolved_class_loader())(crate::Fqn::new(&data_class))?;
        let mut properties: Vec<String> = class
            .properties
            .iter()
            .filter(|property| !property.is_static)
            .map(|property| property.name.trim_start_matches('$').to_string())
            .collect();
        properties.sort_unstable();
        properties.dedup();
        let prefix = content.get(literal_start..cursor)?.as_bytes();
        properties.retain(|name| {
            name.as_bytes()
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        });
        string_key_response(
            properties,
            tower_lsp_server::gen_lsp_types::CompletionItemKind::Property,
            content,
            literal_start,
            position,
        )
    }

    /// The domain the translation key whose string starts at
    /// `literal_start` is looked up in, as the file's symbol map recorded
    /// it, or the default domain when it has no record of the string.
    fn translation_domain_at(&self, uri: &str, content: &str, literal_start: usize) -> String {
        let start = literal_start as u32;
        let recorded = self
            .symbol_map_for(uri)
            .filter(|map| map.matches_source(content))
            .and_then(|map| {
                let span_key = map.spans.iter().find_map(|span| match &span.kind {
                    SymbolKind::StringKey {
                        kind: StringKeyKind::SymfonyTranslation,
                        key,
                        ..
                    } if span.start == start => Some(key.clone()),
                    _ => None,
                });
                span_key.or_else(|| {
                    map.typed_receiver_sites
                        .iter()
                        .find(|site| {
                            site.start == start
                                && site.receiver == crate::symbol_map::ReceiverClass::Translator
                        })
                        .map(|site| site.key.clone())
                })
            });
        recorded.map_or_else(
            || DEFAULT_DOMAIN.to_string(),
            |key| split_translation_key(&key).0.to_string(),
        )
    }

    /// What a string passed straight to a method or function call names:
    /// a container lookup, a configurator call, or a translation on a
    /// receiver the type engine confirms, a configurator function, `t()`,
    /// or a `TranslatableMessage`.
    fn symfony_call_key(
        &self,
        content: &str,
        cursor: usize,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<StringKeyKind> {
        // A name is the argument itself, not an element of an array one.
        if !matches!(code.last_code_byte(), Some(b'(' | b',')) {
            return None;
        }
        let call = detect_string_call_context(content, cursor, code)?;
        let Some(subject) = call.subject.as_deref() else {
            let paren = code.enclosing_paren()?;
            let written = trailing_name(&content[..paren.code_before]);
            let offset = (paren.code_before - written.len()) as u32;
            let translates = if is_instantiation(&content[..offset as usize]) {
                let class = ctx.resolve_name_at(written, offset);
                if call.arg_index == 0 && is_bus_name_stamp(&class) {
                    return Some(StringKeyKind::MessengerBus);
                }
                is_translatable_message(&class)
            } else {
                let function = resolve_function_name(ctx, written, offset);
                if call.arg_index == 0
                    && let Some(kind) = configurator_function_kind(&function)
                {
                    return Some(kind);
                }
                is_translate_function(&function)
            };
            return match call.arg_index {
                0 if translates => Some(StringKeyKind::SymfonyTranslation),
                2 if translates => Some(StringKeyKind::TranslationDomain),
                _ => None,
            };
        };
        if call.is_static {
            return None;
        }
        self.receiver_call_key(
            subject,
            &call.method_name,
            call.arg_index,
            content,
            cursor,
            ctx,
        )
    }

    /// What the argument `index` of a call of `method` on `subject`, written
    /// at `cursor`, names, by the receiver the type engine resolves it to.
    pub(crate) fn receiver_call_key(
        &self,
        subject: &str,
        method: &str,
        index: usize,
        content: &str,
        cursor: usize,
        ctx: &FileContext,
    ) -> Option<StringKeyKind> {
        let (receiver, _) = symfony_receiver_method(method, index)?;
        let loaders = self.file_loaders(ctx);
        let class_loader = loaders.resolved_class_loader();
        let rctx = loaders.resolution_ctx_at(content, cursor as u32);
        crate::type_engine::resolver::resolve_target_classes(subject, AccessKind::Arrow, &rctx)
            .into_iter()
            .filter_map(|resolved| resolved.class_info)
            .find_map(|class| {
                self.typed_receiver_key_kind(
                    &crate::php_type::PhpType::named(class.fqn()),
                    method,
                    index,
                    receiver,
                    class_loader,
                )
            })
    }

    fn symfony_key_response(
        &self,
        uri: &str,
        kind: StringKeyKind,
        content: &str,
        prefix_start: usize,
        cursor: usize,
        position: Position,
    ) -> Option<CompletionResponse> {
        let names = self.symfony_declared_names_except(kind, Some((uri, prefix_start as u32)));
        self.symfony_names_response(kind, names, content, prefix_start, cursor, position)
    }

    /// The completion response offering those of `names` that start with
    /// what is typed between `prefix_start` and `cursor`.
    fn symfony_names_response(
        &self,
        kind: StringKeyKind,
        names: Vec<String>,
        content: &str,
        prefix_start: usize,
        cursor: usize,
        position: Position,
    ) -> Option<CompletionResponse> {
        let prefix = content.get(prefix_start..cursor)?.as_bytes();
        let names: Vec<String> = names
            .into_iter()
            .filter(|name| {
                name.as_bytes()
                    .get(..prefix.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
            })
            .collect();
        string_key_response(
            names,
            string_key_item_kind(&kind),
            content,
            prefix_start,
            position,
        )
    }

    /// Complete a service reference or parameter placeholder in a YAML or
    /// XML configuration file, or a route name, translation key, or template
    /// name in a Twig template.
    pub(crate) fn try_symfony_resource_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        let is_symfony = self.is_symfony_project();
        let cursor = position_to_offset(content, position) as usize;
        let (kind, prefix_start) = if self.is_twig_template_uri(uri) {
            // Template names are Twig's, and route names and translation
            // keys Symfony's.
            if !self.is_twig_project() {
                return None;
            }
            if is_symfony
                && let Some((start, domain)) =
                    crate::symfony::translations::twig_translation_completion(content, cursor)
            {
                let messages = self.symfony_translation_messages(&domain);
                return self.symfony_names_response(
                    StringKeyKind::SymfonyTranslation,
                    messages,
                    content,
                    start,
                    cursor,
                    position,
                );
            }
            if let Some(start) = crate::twig::templates::twig_template_name_start(content, cursor) {
                (StringKeyKind::TwigTemplate, start)
            } else if is_symfony {
                let start = crate::symfony::routes::twig_route_name_start(content, cursor)?;
                (StringKeyKind::SymfonyRoute, start)
            } else {
                return None;
            }
        } else if !is_symfony {
            return None;
        } else if crate::symfony::resources::is_container_config_uri(uri) {
            match config_slot_at(uri, content, cursor).or_else(|| resource_key_at(content, cursor))
            {
                Some(found) => found,
                None if crate::symfony::config_tree::is_bundle_config_uri(uri) => {
                    let (parent, start) =
                        crate::symfony::config_tree::config_key_completion(content, cursor)?;
                    let declared = self.symfony_declared_names(StringKeyKind::SymfonyConfigKey);
                    let children = crate::symfony::config_tree::child_keys(&declared, &parent);
                    return self.symfony_names_response(
                        StringKeyKind::SymfonyConfigKey,
                        children,
                        content,
                        start,
                        cursor,
                        position,
                    );
                }
                None => return None,
            }
        } else {
            return None;
        };
        self.symfony_key_response(uri, kind, content, prefix_start, cursor, position)
    }
}

/// What an `#[Autowire]`, `#[Template]`, `#[AsEventListener]`, or
/// `#[AsMessageHandler]` string names, and where the name in it starts:
/// `service:` a service, `param:` a parameter, and its value an `@id`
/// service reference or a `%name%` placeholder; a `#[Template]`'s
/// `template:` a template; an `#[AsEventListener]`'s `event:` an event; an
/// `#[AsMessageHandler]`'s `bus:` a bus.
fn attribute_key(
    content: &str,
    code: &CodeContext<'_>,
    literal: &str,
    literal_start: usize,
    ctx: &FileContext,
) -> Option<(StringKeyKind, usize)> {
    let (callable, slot) = scalar_argument_at(content, code)?;
    let name_start = callable.len() - trailing_name(callable).len();
    let class_start = attribute_class_start(callable, name_start)?;
    let written = &callable[class_start..];
    let attribute = ctx.resolve_name_at(written, class_start as u32);
    if is_template_attribute(&attribute) {
        return matches!(
            slot,
            ArgumentSlot::Named("template") | ArgumentSlot::Positional(0)
        )
        .then_some((StringKeyKind::TwigTemplate, literal_start));
    }
    if is_event_listener_attribute(&attribute) {
        return matches!(
            slot,
            ArgumentSlot::Named("event") | ArgumentSlot::Positional(0)
        )
        .then_some((StringKeyKind::SymfonyEvent, literal_start));
    }
    if crate::yii::routes::is_route_argument_attribute(&attribute) {
        return matches!(
            slot,
            ArgumentSlot::Named("name") | ArgumentSlot::Positional(0)
        )
        .then_some((StringKeyKind::YiiRouteArgument, literal_start));
    }
    if is_message_handler_attribute(&attribute) {
        return matches!(
            slot,
            ArgumentSlot::Named("bus") | ArgumentSlot::Positional(0)
        )
        .then_some((StringKeyKind::MessengerBus, literal_start));
    }
    if !is_autowire_attribute(&attribute) {
        return None;
    }
    match slot {
        ArgumentSlot::Named("service") => Some((StringKeyKind::ServiceId, literal_start)),
        ArgumentSlot::Named("param") => Some((StringKeyKind::ContainerParameter, literal_start)),
        ArgumentSlot::Named("value") | ArgumentSlot::Positional(0) => {
            if literal.starts_with('@') && !literal.starts_with("@@") {
                return Some((StringKeyKind::ServiceId, literal_start + 1));
            }
            open_placeholder(literal)
                .map(|start| (StringKeyKind::ContainerParameter, literal_start + start))
        }
        _ => None,
    }
}

/// The handler whose declaration `offset` is in, as a route argument's key
/// names it: the innermost class around it, and the last of its methods
/// whose name comes before it.
fn handler_at(ctx: &FileContext, offset: u32) -> Option<String> {
    let class = ctx
        .classes
        .iter()
        .filter(|class| class.start_offset <= offset && offset < class.end_offset)
        .min_by_key(|class| class.end_offset - class.start_offset)?;
    let method = class
        .methods
        .iter()
        .filter(|method| method.name_offset != 0 && method.name_offset < offset)
        .max_by_key(|method| method.name_offset)?;
    Some(crate::yii::routes::handler_key(&class.fqn(), &method.name))
}

/// Where the name of a `%placeholder%` the end of `text` is inside starts.
fn open_placeholder(text: &str) -> Option<usize> {
    let percents = text.bytes().filter(|byte| *byte == b'%').count();
    (percents % 2 == 1).then(|| text.rfind('%').map(|index| index + 1))?
}

/// The event a listener tag subscribes to, or the bus a handler tag or the
/// Messenger configuration reads, being typed at `cursor`, and where it
/// starts.  A bus a key declares is not completed: it is a new name.
fn config_slot_at(uri: &str, content: &str, cursor: usize) -> Option<(StringKeyKind, usize)> {
    let mut slots = crate::symfony::tags::tag_slots(uri, content);
    slots.extend(crate::symfony::messenger::bus_slots(uri, content));
    slots
        .into_iter()
        .find(|slot| {
            slot.start <= cursor
                && cursor <= slot.end
                && (slot.kind == StringKeyKind::SymfonyEvent || !slot.is_write)
        })
        .map(|slot| (slot.kind, slot.start))
}

/// The service reference or parameter placeholder being typed at `cursor`
/// in a YAML or XML line, and where the name in it starts.
fn resource_key_at(content: &str, cursor: usize) -> Option<(StringKeyKind, usize)> {
    let line_start = content[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let line = &content[line_start..cursor];
    let is_name_byte =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-');

    if let Some(start) = open_placeholder(line)
        && line[start..].bytes().all(is_name_byte)
    {
        return Some((StringKeyKind::ContainerParameter, line_start + start));
    }

    let name_start = line
        .bytes()
        .rposition(|byte| !is_name_byte(byte))
        .map_or(0, |index| index + 1);
    let before = &line[..name_start];
    let before = before
        .strip_suffix(['?', '!'])
        .filter(|rest| rest.ends_with('@'))
        .unwrap_or(before);
    if let Some(at) = before.strip_suffix('@') {
        let opens_value = at.is_empty() || at.ends_with([' ', '\'', '"', '[', ',', '{', '\t']);
        return opens_value.then_some((StringKeyKind::ServiceId, line_start + name_start));
    }

    // An XML attribute that takes a service ID.
    let attribute = before.strip_suffix('"')?.strip_suffix('=')?;
    let attribute = trailing_name(attribute.trim_end());
    let takes_service = match attribute {
        "alias" | "decorates" | "parent" | "service" => true,
        "id" => {
            let tag = &line[line.rfind('<')?..];
            tag.starts_with("<argument") && tag.contains("type=\"service\"")
        }
        _ => false,
    };
    takes_service.then_some((StringKeyKind::ServiceId, line_start + name_start))
}

/// The class whose body holds `offset`, among `statements` and the
/// namespaces they declare.
fn class_at<'p, 'a>(
    statements: impl Iterator<Item = &'p mago_syntax::cst::Statement<'a>>,
    offset: u32,
) -> Option<&'p mago_syntax::cst::Class<'a>>
where
    'a: 'p,
{
    use mago_span::HasSpan;
    use mago_syntax::cst::Statement;
    for statement in statements {
        match statement {
            Statement::Class(class) if class.span().has_offset(offset) => return Some(class),
            Statement::Namespace(namespace) if namespace.span().has_offset(offset) => {
                return class_at(namespace.statements().iter(), offset);
            }
            _ => {}
        }
    }
    None
}

/// Whether `before`, the text before a callee's name, ends with `new`.
fn is_instantiation(before: &str) -> bool {
    let before = before.trim_end();
    before.strip_suffix("new").is_some_and(|rest| {
        !rest
            .bytes()
            .next_back()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$')
    })
}

/// The (possibly qualified) name `text` ends with.
fn trailing_name(text: &str) -> &str {
    let start = text
        .bytes()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\')))
        .map_or(0, |index| index + 1);
    &text[start..]
}

/// The function a call written as `written` at `offset` names, by PHP's
/// rule: a `use function` import, else the current namespace.  A name
/// resolved by the last parse is used as it is.
fn resolve_function_name(ctx: &FileContext, written: &str, offset: u32) -> String {
    if let Some(resolved) = ctx
        .resolved_names
        .as_deref()
        .and_then(|names| names.get(offset))
    {
        return resolved.to_string();
    }
    if let Some(qualified) = written.strip_prefix('\\') {
        return qualified.to_string();
    }
    let imports = &ctx.symbol_imports_at(offset).functions;
    if let Some(imported) = imports
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(written))
        .map(|(_, fqn)| fqn)
    {
        return imported.clone();
    }
    crate::util::build_fqn(written, ctx.namespace_at(offset).as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key being typed where `|` is, and the name typed so far.
    fn at(content: &str) -> Option<(StringKeyKind, String)> {
        let cursor = content.find('|').unwrap();
        let content = content.replacen('|', "", 1);
        let (kind, start) = resource_key_at(&content, cursor)?;
        Some((kind, content[start..cursor].to_string()))
    }

    #[test]
    fn yaml_references_and_placeholders_are_recognised() {
        assert_eq!(
            at("    arguments: ['@app.ma|']"),
            Some((StringKeyKind::ServiceId, "app.ma".to_string()))
        );
        assert_eq!(
            at("    arguments: ['@?app.|']"),
            Some((StringKeyKind::ServiceId, "app.".to_string()))
        );
        assert_eq!(
            at("    dir: '%kernel.pro|"),
            Some((StringKeyKind::ContainerParameter, "kernel.pro".to_string()))
        );
        assert_eq!(at("    email: 'me@exam|'"), None);
        assert_eq!(at("    done: '%app.x%/ab|'"), None);
    }

    #[test]
    fn xml_attributes_that_take_a_service_are_recognised() {
        assert_eq!(
            at(r#"<service id="b" alias="app.m|"#),
            Some((StringKeyKind::ServiceId, "app.m".to_string()))
        );
        assert_eq!(
            at(r#"<argument type="service" id="app.|"#),
            Some((StringKeyKind::ServiceId, "app.".to_string()))
        );
        assert_eq!(at(r#"<service id="app.|"#), None);
    }
}
