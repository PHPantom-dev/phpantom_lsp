//! What a service ID or a container parameter resolves to: the class a
//! service is built from, and the type of a parameter's value.
//!
//! Each declaration records what it gives beside its span, as a
//! [`ContainerValueSite`] (see [`super::resources`] for YAML and XML, and
//! symbol-map extraction for the PHP configurators).  The answers here
//! follow those declarations through aliases and `%name%` placeholders, so
//! `$container->get('app.mailer')` and `$this->getParameter('app.sender')`
//! are typed by what the project's own configuration declares.

use std::collections::HashSet;

use crate::Backend;
use crate::atom::Atom;
use crate::php_type::PhpType;
use crate::symbol_map::{ContainerValue, StringKeyKind};

/// What a string value gives a service or a parameter as: the parameter a
/// whole `%name%` placeholder copies, a class when the string is spelled as
/// a namespaced one, the type of the environment variable a whole
/// `%env(…)%` reads, and a `string` otherwise.
pub(crate) fn string_value(text: &str) -> ContainerValue {
    if let Some(ty) = env_placeholder_type(text) {
        return ContainerValue::Type(ty);
    }
    if placeholder_name(text).is_some() {
        return ContainerValue::Class(crate::atom::atom(text));
    }
    let class = text.strip_prefix('\\').unwrap_or(text);
    if class.contains('\\') && is_class_name(class) {
        return ContainerValue::Class(crate::atom::atom(class));
    }
    ContainerValue::Type(PhpType::string())
}

/// The value of a YAML parameter written inline after its key, as Symfony's
/// YAML parser reads it.  `None` for a value whose type the text does not
/// settle: a tag such as `!php/const`, or an anchor or alias.
pub(crate) fn yaml_scalar_value(raw: &str) -> Option<ContainerValue> {
    let raw = raw.trim();
    if raw.starts_with(['[', '{']) {
        return Some(ContainerValue::Type(PhpType::array()));
    }
    if let Some(inner) = raw
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            raw.strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
    {
        return Some(string_value(inner));
    }
    if raw.starts_with(['!', '&', '*']) {
        return None;
    }
    // A block scalar (`|` or `>`) is a string, whatever its lines say.
    if raw.starts_with(['|', '>']) {
        return Some(ContainerValue::Type(PhpType::string()));
    }
    Some(match scalar_type(raw, true) {
        Some(ty) => ContainerValue::Type(ty),
        None => string_value(raw),
    })
}

/// The value of an XML `<parameter>`, from its text and its `type`
/// attribute.  Symfony reads the text the way `XmlUtils::phpize()` does
/// unless the type says it is a string.
pub(crate) fn xml_parameter_value(
    text: &str,
    type_attribute: Option<&str>,
) -> Option<ContainerValue> {
    match type_attribute {
        Some("collection") => Some(ContainerValue::Type(PhpType::array())),
        Some("string" | "binary") => Some(string_value(text)),
        Some(_) => None,
        None => {
            let text = text.trim();
            Some(match scalar_type(text, false) {
                Some(ty) => ContainerValue::Type(ty),
                None => string_value(text),
            })
        }
    }
}

/// The type of an unquoted scalar that is not a string: a null, a boolean,
/// an integer, or a float.  YAML also reads `~` as null and the `.inf` and
/// `.nan` spellings as floats.
fn scalar_type(text: &str, yaml: bool) -> Option<PhpType> {
    let lower = text.to_ascii_lowercase();
    match lower.as_str() {
        "null" => return Some(PhpType::null()),
        "~" if yaml => return Some(PhpType::null()),
        "true" | "false" => return Some(PhpType::bool()),
        ".inf" | "-.inf" | "+.inf" | ".nan" if yaml => return Some(PhpType::float()),
        _ => {}
    }
    let digits = lower.trim_start_matches(['-', '+']);
    if digits.is_empty() {
        return None;
    }
    if digits
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'_')
        || digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0o"))
            .or_else(|| digits.strip_prefix("0b"))
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Some(PhpType::int());
    }
    let is_float = digits.bytes().any(|byte| matches!(byte, b'.' | b'e'))
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'-' | b'+' | b'_'))
        && digits.parse::<f64>().is_ok();
    is_float.then(PhpType::float)
}

/// The parameter a whole `%name%` placeholder names.
fn placeholder_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix('%')?.strip_suffix('%')?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')))
    .then_some(name)
}

/// The type a whole `%env(processor:NAME)%` placeholder reads the variable
/// as, by its outermost processor; a variable read without one is a string.
fn env_placeholder_type(text: &str) -> Option<PhpType> {
    let inner = text.strip_prefix("%env(")?.strip_suffix(")%")?;
    let Some((processor, _)) = inner.split_once(':') else {
        return Some(PhpType::string());
    };
    let provided = match processor {
        "base64" | "file" | "resolve" | "string" | "trim" | "urlencode" => "string",
        "bool" | "not" | "defined" => "bool",
        "int" => "int",
        "float" => "float",
        "csv" | "json" | "url" | "query_string" | "shuffle" => "array",
        "enum" => "\\BackedEnum",
        _ => "bool|int|float|string|array",
    };
    Some(PhpType::parse(provided))
}

/// Whether `name` is spelled as a class name.
fn is_class_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !name.ends_with('\\')
        && !name.contains("\\\\")
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\\' || byte >= 0x80
        })
}

/// The parameters the kernel sets on every application, and their types.
fn kernel_parameter_type(name: &str) -> Option<PhpType> {
    Some(match name {
        "kernel.project_dir"
        | "kernel.environment"
        | "kernel.runtime_environment"
        | "kernel.build_dir"
        | "kernel.cache_dir"
        | "kernel.logs_dir"
        | "kernel.charset"
        | "kernel.container_class"
        | "kernel.secret"
        | "kernel.share_dir"
        | "kernel.runtime_mode" => PhpType::string(),
        "kernel.debug" | "kernel.runtime_mode.web" | "kernel.runtime_mode.cli" => PhpType::bool(),
        "kernel.bundles" | "kernel.bundles_metadata" | "kernel.enabled_locales" => PhpType::array(),
        _ => return None,
    })
}

impl Backend {
    /// What each of the project's declarations of the `kind` name `key`
    /// gives it as, `None` for one that gives nothing.
    fn symfony_declared_values(
        &self,
        kind: StringKeyKind,
        key: &str,
    ) -> Vec<Option<ContainerValue>> {
        let mut values = Vec::new();
        self.visit_symfony_declarations(kind, |_, map, start, _, declared| {
            if declared != key {
                return;
            }
            let at = map
                .container_values
                .partition_point(|site| site.start < start);
            let mut found = false;
            for site in map.container_values[at..]
                .iter()
                .take_while(|site| site.start == start)
            {
                values.push(Some(site.value.clone()));
                found = true;
            }
            if !found {
                values.push(None);
            }
        });
        values
    }

    /// The classes the service `id` is built from, following aliases and
    /// a class given as a parameter.  An ID spelled as a class is that class
    /// when no declaration says otherwise.  Empty when nothing the project
    /// declares says.
    pub(crate) fn symfony_service_classes(&self, id: &str) -> Vec<Atom> {
        let mut classes = Vec::new();
        self.collect_service_classes(id, &mut HashSet::new(), &mut classes);
        classes
    }

    fn collect_service_classes(&self, id: &str, seen: &mut HashSet<String>, out: &mut Vec<Atom>) {
        if !seen.insert(id.to_string()) {
            return;
        }
        let mut found = false;
        for value in self.symfony_declared_values(StringKeyKind::ServiceId, id) {
            match value {
                Some(ContainerValue::Class(class)) => match placeholder_name(&class) {
                    Some(parameter) => {
                        for value in self.parameter_values(parameter) {
                            if let Some(ContainerValue::Class(class)) = value
                                && placeholder_name(&class).is_none()
                            {
                                found = true;
                                push_unique(out, class);
                            }
                        }
                    }
                    None => {
                        found = true;
                        push_unique(out, class);
                    }
                },
                Some(ContainerValue::Alias(target)) => {
                    found = true;
                    self.collect_service_classes(&target, seen, out);
                }
                Some(ContainerValue::Type(_)) | None => {}
            }
        }
        if !found && id.contains('\\') {
            let class = id.strip_prefix('\\').unwrap_or(id);
            if is_class_name(class) {
                push_unique(out, crate::atom::atom(class));
            }
        }
    }

    /// The values the parameter `name` is declared with, following a value
    /// that copies another parameter.
    fn parameter_values(&self, name: &str) -> Vec<Option<ContainerValue>> {
        let mut values = Vec::new();
        let mut seen = HashSet::new();
        let mut pending = vec![name.to_string()];
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            for value in self.symfony_declared_values(StringKeyKind::ContainerParameter, &name) {
                if let Some(ContainerValue::Class(class)) = &value
                    && let Some(copied) = placeholder_name(class)
                {
                    pending.push(copied.to_string());
                    continue;
                }
                values.push(value);
            }
        }
        values
    }

    /// The type of the container parameter `name`'s value, when every
    /// declaration of it says, or when it is one the kernel sets.
    pub(crate) fn symfony_parameter_type(&self, name: &str) -> Option<PhpType> {
        let values = self.parameter_values(name);
        if values.is_empty() {
            return kernel_parameter_type(name);
        }
        let mut types = Vec::with_capacity(values.len());
        for value in values {
            types.push(match value? {
                ContainerValue::Type(ty) => ty,
                ContainerValue::Class(_) | ContainerValue::Alias(_) => PhpType::string(),
            });
        }
        Some(PhpType::union(types))
    }
}

fn push_unique(out: &mut Vec<Atom>, class: Atom) {
    if !out.contains(&class) {
        out.push(class);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_of(value: Option<ContainerValue>) -> String {
        match value {
            Some(ContainerValue::Type(ty)) => ty.to_string(),
            Some(ContainerValue::Class(class)) => format!("class {class}"),
            Some(ContainerValue::Alias(id)) => format!("alias {id}"),
            None => "none".to_string(),
        }
    }

    #[test]
    fn yaml_scalars_are_read_as_symfony_reads_them() {
        let cases = [
            ("'PHPantom'", "string"),
            ("PHPantom", "string"),
            ("'123'", "string"),
            ("123", "int"),
            ("-1_000", "int"),
            ("0x1F", "int"),
            ("1.5", "float"),
            ("1e3", "float"),
            (".inf", "float"),
            ("true", "bool"),
            ("FALSE", "bool"),
            ("~", "null"),
            ("null", "null"),
            ("[a, b]", "array"),
            ("{ a: 1 }", "array"),
            ("'%env(int:PORT)%'", "int"),
            ("'%env(APP_SECRET)%'", "string"),
            ("'%env(json:CONFIG)%'", "array"),
            ("'%app.other%'", "class %app.other%"),
            ("App\\Service\\Mailer", "class App\\Service\\Mailer"),
            ("'\\App\\Service\\Mailer'", "class App\\Service\\Mailer"),
            ("!php/const PHP_INT_MAX", "none"),
            ("|", "string"),
        ];
        for (raw, expected) in cases {
            assert_eq!(type_of(yaml_scalar_value(raw)), expected, "{raw}");
        }
    }

    #[test]
    fn xml_parameters_are_phpized_unless_typed() {
        assert_eq!(type_of(xml_parameter_value("123", None)), "int");
        assert_eq!(
            type_of(xml_parameter_value("123", Some("string"))),
            "string"
        );
        assert_eq!(type_of(xml_parameter_value("true", None)), "bool");
        assert_eq!(type_of(xml_parameter_value("PHPantom", None)), "string");
        assert_eq!(
            type_of(xml_parameter_value("", Some("collection"))),
            "array"
        );
        assert_eq!(
            type_of(xml_parameter_value("X::Y", Some("constant"))),
            "none"
        );
    }
}
