//! Hover for Laravel, Blade, and Symfony: the string keys a call names (routes,
//! config, views, translations, Artisan commands, gate abilities, container
//! bindings, morph aliases, `.env` names), Blade sections and stacks, an
//! Artisan command's own arguments and options, and the column a
//! `model-property<Model>` string names.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::php_type::{PhpType, TypeKind};
use crate::virtual_members::laravel::config_values::{ConfigNode, ConfigValue};

use super::formatting::{inline_code, make_hover};

impl Backend {
    /// Where a Laravel string key resolves to, with the path shortened to
    /// start at the `dir` segment its kind is filed under.
    ///
    /// `config/app.php` reads better in a hover than the absolute path it
    /// sits at. A file outside that directory keeps its path whole rather
    /// than being cut at a segment it does not have.
    fn resolved_key_location(
        &self,
        kind: &crate::symbol_map::StringKeyKind,
        key: &str,
        uri: &str,
        dir: &str,
    ) -> Option<(Uri, String)> {
        let location =
            crate::virtual_members::laravel::resolve_laravel_string_key(self, kind, key, uri)
                .into_iter()
                .next()?;
        let path = location.uri.path();
        let short_path = match path.rsplit_once(&format!("/{dir}/")) {
            Some((_, rest)) => format!("{dir}/{rest}"),
            None => path.to_string(),
        };
        Some((location.uri, short_path))
    }

    /// Build hover content for a Laravel string key (route name, config
    /// key, view name, or translation key).
    pub(super) fn hover_laravel_string_key(
        &self,
        kind: &crate::symbol_map::StringKeyKind,
        key: &str,
        uri: &str,
    ) -> Option<Hover> {
        use crate::symbol_map::StringKeyKind;

        // A translation key is shown as the message it looks up.
        let mut shown = key;
        // A Yii view is keyed by the path it names, which reads best from
        // the workspace root.
        let view_path;
        let (label, detail) = match kind {
            StringKeyKind::Section => ("Section", self.blade_block_detail(uri, kind, key)),
            StringKeyKind::Stack => ("Stack", self.blade_block_detail(uri, kind, key)),
            StringKeyKind::Route => {
                let locations = crate::virtual_members::laravel::resolve_laravel_string_key(
                    self, kind, key, uri,
                );
                let detail = if let Some(loc) = locations.first() {
                    let path = loc.uri.path();
                    // A conventional route file lives under `routes/`, so
                    // that segment onward is the useful part; a Folio page
                    // lives anywhere under the view roots, so it falls back
                    // to a workspace-relative path the same way View does.
                    let short_path = path.rsplit_once("/routes/").map_or_else(
                        || {
                            self.workspace_relative_path(loc.uri.as_str())
                                .unwrap_or_else(|| path.to_string())
                        },
                        |(_, rest)| format!("routes/{}", rest),
                    );
                    format!("Defined in `{}`", short_path)
                } else {
                    "Route name".to_string()
                };
                ("Route", detail)
            }
            StringKeyKind::Config => {
                let location = match self.resolved_key_location(kind, key, uri, "config") {
                    Some((_, short_path)) => format!("Defined in `{short_path}`"),
                    None => "Config key".to_string(),
                };
                let detail = match self.with_config_node(key, |node| self.config_value_detail(node))
                {
                    Some(Some(value)) => format!("{value}\n\n{location}"),
                    _ => location,
                };
                ("Config", detail)
            }
            StringKeyKind::ConfigResource(resource) => {
                let descriptor = crate::symbol_map::laravel_resources::descriptor(*resource);
                let locations = crate::virtual_members::laravel::resolve_laravel_string_key(
                    self, kind, key, uri,
                );
                let detail = locations
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                    .map_or_else(
                        || format!("Laravel {}", descriptor.label),
                        |path| format!("Defined in `{path}`"),
                    );
                (descriptor.hover_label, detail)
            }
            StringKeyKind::View => {
                let locations = crate::virtual_members::laravel::resolve_laravel_string_key(
                    self, kind, key, uri,
                );
                let detail = if let Some(loc) = locations.first() {
                    // Show the path relative to the workspace root so
                    // custom view directories (from `config/view.php`)
                    // display cleanly rather than as absolute paths.
                    let short_path = self
                        .workspace_relative_path(loc.uri.as_str())
                        .unwrap_or_else(|| loc.uri.path().to_string());
                    format!("`{}`", short_path)
                } else {
                    "View template".to_string()
                };
                ("View", detail)
            }
            StringKeyKind::TwigTemplate => {
                let detail = self
                    .symfony_declaration_locations(*kind, key)
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                    .map_or_else(
                        || "Not one of the project's templates".to_string(),
                        |path| format!("`{}`", path),
                    );
                ("Template", detail)
            }
            StringKeyKind::Trans => ("Trans", self.translation_hover_detail(key)),
            StringKeyKind::Command => {
                let index = self.laravel.commands.read();
                let detail = if let Some(entry) = index.get(key) {
                    let mut parts = Vec::new();
                    if let Some(fqn) = &entry.fqn {
                        parts.push(format!("Defined by `{}`", fqn));
                    }
                    let sig = &entry.signature;
                    if !sig.arguments.is_empty() {
                        let args: Vec<String> =
                            sig.arguments.iter().map(|a| a.name.clone()).collect();
                        parts.push(format!("Arguments: `{}`", args.join("`, `")));
                    }
                    if !sig.options.is_empty() {
                        let opts: Vec<String> = sig
                            .options
                            .iter()
                            .map(|o| format!("--{}", o.name))
                            .collect();
                        parts.push(format!("Options: `{}`", opts.join("`, `")));
                    }
                    if parts.is_empty() {
                        "Artisan command".to_string()
                    } else {
                        parts.join("\n\n")
                    }
                } else {
                    "Artisan command".to_string()
                };
                ("Command", detail)
            }
            StringKeyKind::MorphAlias => {
                let index = self.laravel.morph_map.read();
                let detail = match index.get(key) {
                    Some(target) => {
                        let mut detail = format!("Maps to `{}`", target.fqn);
                        if let Some(rel) = self.workspace_relative_path(&target.uri) {
                            detail.push_str(&format!("\n\nRegistered in `{}`", rel));
                        }
                        detail
                    }
                    None if index.is_enforced() => "Not registered in the morph map".to_string(),
                    None => "Morph alias".to_string(),
                };
                ("Morph type", detail)
            }
            StringKeyKind::GateAbility => ("Ability", self.gate_ability_detail(key)),
            StringKeyKind::ContainerBinding => {
                let detail = match self.container_binding_target(key) {
                    Some(target) => {
                        let mut detail = format!("Resolves to `{}`", target.fqn);
                        if let Some(rel) = target
                            .site
                            .and_then(|site| self.workspace_relative_path(&site.uri))
                        {
                            detail.push_str(&format!("\n\nRegistered in `{}`", rel));
                        }
                        detail
                    }
                    None => "Container binding".to_string(),
                };
                ("Container", detail)
            }
            StringKeyKind::Env => {
                // The value is shown like every sibling hover shows what its
                // key resolves to, except for names that read as naming a
                // credential: those are the ones worth not putting on screen
                // during a screen share.
                let detail = match crate::virtual_members::laravel::env_declaration(self, key) {
                    Some(decl) => {
                        let value = if crate::virtual_members::laravel::env_name_is_sensitive(key) {
                            "Value hidden".to_string()
                        } else if decl.value.is_empty() {
                            "Set to an empty value".to_string()
                        } else {
                            inline_code(&decl.value)
                        };
                        format!("{}\n\nDeclared in `{}`", value, decl.file)
                    }
                    None => "Not declared in `.env`".to_string(),
                };
                ("Env", detail)
            }
            StringKeyKind::ServiceId
            | StringKeyKind::ContainerParameter
            | StringKeyKind::SymfonyRoute
            | StringKeyKind::SymfonyTranslation
            | StringKeyKind::TranslationDomain => {
                let label = match kind {
                    StringKeyKind::ServiceId => "Service",
                    StringKeyKind::ContainerParameter => "Parameter",
                    StringKeyKind::SymfonyRoute => "Route",
                    StringKeyKind::SymfonyTranslation => "Translation",
                    _ => "Translation domain",
                };
                let declared = self.symfony_declaration_locations(*kind, key);
                let mut detail = match declared
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                {
                    Some(path) => format!("Declared in `{}`", path),
                    None if kind.is_symfony_translation() => {
                        "Not declared in the project's catalogues".to_string()
                    }
                    None => "Not declared in the project's configuration".to_string(),
                };
                if *kind == StringKeyKind::SymfonyTranslation {
                    let (domain, message) =
                        crate::symfony::translations::split_translation_key(key);
                    shown = message;
                    detail = format!("Domain `{}`\n\n{}", domain, detail);
                }
                (label, detail)
            }
            // Every listener declares the event it subscribes to, so the
            // declarations are its listeners.
            StringKeyKind::SymfonyEvent => {
                let listeners = self.symfony_declaration_locations(*kind, key).len();
                let detail = match listeners {
                    0 => "No listener in the project subscribes to it".to_string(),
                    1 => "1 listener in the project".to_string(),
                    count => format!("{count} listeners in the project"),
                };
                ("Event", detail)
            }
            StringKeyKind::MessengerBus => {
                let detail = self
                    .symfony_declaration_locations(*kind, key)
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                    .map_or_else(
                        || "Not declared in the project's configuration".to_string(),
                        |path| format!("Declared in `{}`", path),
                    );
                ("Messenger bus", detail)
            }
            StringKeyKind::YiiRoute => {
                let declared = self.symfony_declaration_locations(*kind, key);
                let mut detail = declared
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                    .map_or_else(
                        || "Not declared in the project's routes".to_string(),
                        |path| format!("Declared in `{}`", path),
                    );
                for path in self.yii_route_paths(key) {
                    detail.push_str(&format!("\n\nPath `{}`", path));
                }
                ("Route", detail)
            }
            // An argument is shown by its name, under the paths that
            // declare it.
            StringKeyKind::YiiRouteArgument => {
                let (handler, name) = crate::yii::routes::split_route_argument_key(key);
                shown = name;
                let paths = self.yii_handler_paths(handler);
                let detail = if paths.is_empty() {
                    "No route the project declares dispatches here".to_string()
                } else {
                    paths
                        .iter()
                        .map(|path| format!("Path `{}`", path))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };
                ("Route argument", detail)
            }
            // An installed bundle's schema is not read, so a key no schema
            // of the project's declares is only not one of its own.
            StringKeyKind::SymfonyConfigKey => {
                let detail = self
                    .symfony_declaration_locations(*kind, key)
                    .first()
                    .and_then(|location| self.workspace_relative_path(location.uri.as_str()))
                    .map_or_else(
                        || "Not declared by the project's own bundles".to_string(),
                        |path| format!("Declared in `{}`", path),
                    );
                ("Configuration key", detail)
            }
            StringKeyKind::YiiView => {
                view_path = self.yii_view_display(key);
                shown = &view_path;
                let detail = self
                    .yii_view_file(key)
                    .and_then(|file| crate::util::path_to_url(&file))
                    .and_then(|uri| self.workspace_relative_path(uri.as_str()))
                    .map_or_else(
                        || "No template is there".to_string(),
                        |path| format!("`{}`", path),
                    );
                ("View", detail)
            }
        };

        Some(make_hover(format!(
            "**{}** `{}`\n\n{}",
            label, shown, detail
        )))
    }

    /// What a config key holds, for its hover: the value as written, or for
    /// an `env()` read, the variable with its default and what `.env` sets
    /// it to.  `None` when the value is not one that can be read statically.
    fn config_value_detail(&self, node: &ConfigNode) -> Option<String> {
        let value = match node {
            ConfigNode::Array(entries) => {
                return Some(count_phrase("An array", entries.len(), "key", "keys"));
            }
            ConfigNode::List(items) => {
                return Some(count_phrase("A list", items.len(), "item", "items"));
            }
            ConfigNode::Leaf(value) => value,
        };
        let ConfigValue::Env {
            name: Some(name),
            default,
        } = value
        else {
            return describe_config_value(value).map(|value| format!("Value: {value}"));
        };
        let mut detail = format!("Environment variable {}", inline_code(name));
        if let Some(default) = default.as_deref().and_then(describe_config_value) {
            detail.push_str(&format!(", defaulting to {default}"));
        }
        if let Some(decl) = crate::virtual_members::laravel::env_declaration(self, name) {
            let set_to = if crate::virtual_members::laravel::env_name_is_sensitive(name) {
                "its value is hidden".to_string()
            } else if decl.value.is_empty() {
                "it is set to an empty value".to_string()
            } else {
                format!("it is set to {}", inline_code(&decl.value))
            };
            detail.push_str(&format!("\n\nIn `{}`, {set_to}", decl.file));
        }
        Some(detail)
    }

    /// Describe where the other half of a Blade section or stack name is:
    /// what renders the name a template fills, or what fills the name a
    /// layout renders.
    fn blade_block_detail(
        &self,
        uri: &str,
        kind: &crate::symbol_map::StringKeyKind,
        name: &str,
    ) -> String {
        use crate::blade::blocks::{BlockKind, BlockRole};
        use crate::symbol_map::StringKeyKind;

        let kind = match kind {
            StringKeyKind::Stack => BlockKind::Stack,
            _ => BlockKind::Section,
        };
        let (role, locations) = self.blade_block_pairing(uri, kind, name);
        let Some(first) = locations.first() else {
            return match role {
                BlockRole::Declare => "Nothing found that fills it".to_string(),
                BlockRole::Fill | BlockRole::Check => "Nothing found that renders it".to_string(),
            };
        };
        let path = self
            .workspace_relative_path(first.uri.as_str())
            .unwrap_or_else(|| first.uri.path().to_string());
        let opening = match role {
            BlockRole::Declare => "Filled by",
            BlockRole::Fill | BlockRole::Check => "Rendered by",
        };
        match locations.len() {
            1 => format!("{opening} `{path}`"),
            more => format!("{opening} `{path}` and {} more", more - 1),
        }
    }

    /// Describe where an authorization ability comes from: the
    /// `Gate::define()` call that registers it (with the callback's
    /// parameters) and every policy method that implements it.
    pub(super) fn gate_ability_detail(&self, ability: &str) -> String {
        let mut parts = Vec::new();

        let definition = self
            .laravel
            .gates
            .read()
            .definition(ability)
            .map(|target| (target.uri.clone(), target.signature.clone()));
        if let Some((uri, signature)) = definition {
            let mut line = match self.workspace_relative_path(&uri) {
                Some(rel) => format!("Defined by `Gate::define()` in `{rel}`"),
                None => "Defined by `Gate::define()`".to_string(),
            };
            if let Some(signature) = signature {
                line.push_str(&format!("\n\n```php\nfn ({signature})\n```"));
            }
            parts.push(line);
        }

        let policies = crate::virtual_members::laravel::gates::policy_methods_named(self, ability);
        if !policies.is_empty() {
            let mut lines = vec!["Policy methods:".to_string()];
            for (policy, method) in policies {
                let parameters: Vec<String> = method
                    .parameters
                    .iter()
                    .map(|param| match &param.type_hint {
                        Some(ty) => format!("{} {}", ty, param.name),
                        None => param.name.to_string(),
                    })
                    .collect();
                lines.push(format!(
                    "- `{}::{}({})`",
                    policy.fqn(),
                    method.name,
                    parameters.join(", ")
                ));
            }
            parts.push(lines.join("\n"));
        }

        if parts.is_empty() {
            "Authorization ability".to_string()
        } else {
            parts.join("\n\n")
        }
    }

    /// Try hover for a string literal inside a `model-property<Model>`
    /// parameter context.  Shows the same property info as hovering
    /// over `$model->property`.
    pub(super) fn try_model_property_hover(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Hover> {
        let cursor_offset = crate::text_position::position_to_offset(content, position);
        let symbol_map = self.symbol_map_for(uri)?;
        let call = symbol_map.find_enclosing_call_site(cursor_offset)?;

        // The string literal the cursor is on, written as an argument of
        // that call itself.
        let (string_start, full_string) =
            crate::parser::with_parsed_program(content, "model_property_hover", |program, _| {
                crate::parser::innermost_node_at(
                    mago_syntax::cst::Node::Program(program),
                    cursor_offset,
                    |node| match node {
                        mago_syntax::cst::Node::LiteralString(string) => Some((
                            mago_span::HasSpan::span(string).start.offset,
                            crate::atom::literal_bytes_to_str(string.value?)?.to_string(),
                        )),
                        _ => None,
                    },
                )
            })?;
        if full_string.is_empty() {
            return None;
        }
        let arg_index = call
            .comma_offsets
            .iter()
            .filter(|&&comma| comma < string_start)
            .count();

        let ctx = self.file_context_at(uri, cursor_offset);
        let resolved =
            self.resolve_callable_target(call.call_expression, content, position, &ctx)?;
        let named = call
            .named_arg_indices
            .iter()
            .position(|&index| index as usize == arg_index)
            .and_then(|k| call.named_arg_names.get(k));
        // A positional argument is the literal itself; a named one starts at
        // its name, which the literal follows.
        if named.is_none() && call.arg_offsets.get(arg_index) != Some(&string_start) {
            return None;
        }
        let param = match named {
            Some(name) => resolved
                .parameters
                .iter()
                .find(|p| p.name.strip_prefix('$').unwrap_or(&p.name) == name)?,
            None => resolved.parameters.get(arg_index)?,
        };
        let param_type = param.type_hint.as_ref()?;

        let model_name = extract_model_name_from_model_property_type(param_type)?;

        let class_loader = self.resolved_class_loader(&ctx);

        let model_class = class_loader(Fqn::new(&model_name))?;
        let resolved_model = crate::virtual_members::resolve_class_fully_cached(
            &model_class,
            &class_loader,
            &self.resolved_class_cache,
        );

        let property = resolved_model
            .properties
            .iter()
            .find(|p| *p.name == full_string)?;

        Some(self.hover_for_property(property, &resolved_model, &class_loader))
    }
}

/// "An array of 3 keys", with the noun agreeing with the count.
fn count_phrase(container: &str, count: usize, one: &str, many: &str) -> String {
    let noun = if count == 1 { one } else { many };
    format!("{container} of {count} {noun}")
}

/// A config value as an inline phrase: a literal as PHP spells it, a
/// `::class` as the class, and a ternary as its arms.  `None` when any part
/// of it is not statically known.
fn describe_config_value(value: &ConfigValue) -> Option<String> {
    Some(match value {
        ConfigValue::Str(text) => inline_code(&format!(
            "'{}'",
            text.replace('\\', "\\\\").replace('\'', "\\'")
        )),
        ConfigValue::Int(text) | ConfigValue::Float(text) => inline_code(text),
        ConfigValue::Bool(true) => "`true`".to_string(),
        ConfigValue::Bool(false) => "`false`".to_string(),
        ConfigValue::Null => "`null`".to_string(),
        ConfigValue::ClassString(name) => inline_code(&format!("{name}::class")),
        ConfigValue::OneOf(arms) => {
            let arms = arms
                .iter()
                .map(describe_config_value)
                .collect::<Option<Vec<_>>>()?;
            match arms.split_last() {
                Some((last, rest)) if !rest.is_empty() => {
                    format!("one of {} or {last}", rest.join(", "))
                }
                _ => return None,
            }
        }
        ConfigValue::Env {
            name: Some(name),
            default,
        } => {
            let mut phrase = format!("the {} environment variable", inline_code(name));
            if let Some(default) = default.as_deref().and_then(describe_config_value) {
                phrase.push_str(&format!(" (default {default})"));
            }
            phrase
        }
        ConfigValue::Env { name: None, .. } | ConfigValue::Dynamic => return None,
    })
}

/// Hover for `$this->argument('user')` / `$this->option('queue')`: resolve
/// the parameter against the enclosing command's parsed `$signature` and show
/// its description and shape.
pub(super) fn hover_command_own_param(
    content: &str,
    offset: usize,
    name: &str,
    is_option: bool,
) -> Option<Hover> {
    let signature = crate::virtual_members::laravel::command_signature_at_offset(content, offset)?;
    let param = if is_option {
        signature.option(name)
    } else {
        signature.argument(name)
    }?;

    let kind = if is_option { "Option" } else { "Argument" };
    let display = if is_option {
        format!("--{}", param.name)
    } else {
        param.name.clone()
    };

    let mut lines = vec![format!("**{} `{}`**", kind, display)];
    if let Some(desc) = &param.description {
        lines.push(desc.clone());
    }
    let mut traits: Vec<String> = Vec::new();
    if param.is_array {
        traits.push("array".to_string());
    }
    if !is_option && param.optional {
        traits.push("optional".to_string());
    }
    if is_option && param.takes_value {
        traits.push("takes a value".to_string());
    }
    if let Some(shortcut) = &param.shortcut {
        traits.push(format!("shortcut -{}", shortcut));
    }
    if let Some(default) = &param.default {
        traits.push(format!("default `{}`", default));
    }
    if !traits.is_empty() {
        lines.push(format!("_{}_", traits.join(", ")));
    }

    Some(make_hover(lines.join("\n\n")))
}

/// Extract a model name from a `model-property<Model>` type, including
/// when nested inside array/list generic arguments.
fn extract_model_name_from_model_property_type(ty: &PhpType) -> Option<String> {
    if let TypeKind::Generic(g) = ty.kind()
        && g.name.eq_ignore_ascii_case("model-property")
        && g.args.len() == 1
    {
        return g.args[0].base_name().map(|s| s.to_string());
    }
    if let TypeKind::Generic(g) = ty.kind()
        && (crate::php_type::is_array_like_name(&g.name) || g.name.eq_ignore_ascii_case("list"))
    {
        for arg in &g.args {
            if let TypeKind::Generic(inner) = arg.kind()
                && inner.name.eq_ignore_ascii_case("model-property")
                && inner.args.len() == 1
            {
                return inner.args[0].base_name().map(|s| s.to_string());
            }
        }
    }
    None
}
