//! Declarative Laravel config-resource families and their string triggers.

use super::LaravelConfigResource;
use crate::names::OwnedResolvedNames;

/// Maximum instance-method links inspected before a facade root is considered
/// too far away. Shared by AST extraction and the live completion scanner so
/// the two paths classify the same receiver spines.
pub(crate) const FACADE_CHAIN_DEPTH: usize = 4;

/// Short facade names accepted by direct config-resource triggers. Kept as a
/// compact static slice so live completion can resolve imports without
/// rebuilding the descriptor-derived set on every keystroke.
pub(crate) const RESOURCE_FACADES: &[&str] = &[
    "Auth",
    "Broadcast",
    "Cache",
    "DB",
    "Log",
    "Mail",
    "Queue",
    "Storage",
];

/// Container attributes that select a config-backed resource.
pub(crate) const RESOURCE_ATTRIBUTES: &[&str] = &[
    "Auth",
    "Authenticated",
    "Cache",
    "DB",
    "Database",
    "Log",
    "Storage",
];

/// How a trigger accepts resource names in its selected argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceArgumentShape {
    /// One scalar string literal.
    Scalar,
    /// A scalar string or the string values of an array.
    ScalarOrArray,
    /// Only the string values of an array.
    Array,
}

impl ResourceArgumentShape {
    /// Whether the trigger accepts an array of names.
    pub(crate) const fn accepts_array(self) -> bool {
        matches!(self, Self::ScalarOrArray | Self::Array)
    }

    /// Whether the trigger accepts one scalar name.
    pub(crate) const fn accepts_scalar(self) -> bool {
        matches!(self, Self::Scalar | Self::ScalarOrArray)
    }
}

/// Whether a resource-name occurrence reads, defines, or optionally removes
/// the resource it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceAccess {
    Read,
    Write,
    OptionalRead,
}

impl ResourceAccess {
    pub(crate) const fn is_write(self) -> bool {
        matches!(self, Self::Write)
    }

    pub(crate) const fn is_optional(self) -> bool {
        matches!(self, Self::OptionalRead)
    }
}

/// One syntactic place that accepts a member of a config-resource family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigResourceTrigger {
    Function {
        name: &'static str,
        argument: &'static str,
        shape: ResourceArgumentShape,
        access: ResourceAccess,
    },
    StaticMethod {
        facade: &'static str,
        method: &'static str,
        argument: &'static str,
        shape: ResourceArgumentShape,
        access: ResourceAccess,
    },
    Attribute {
        name: &'static str,
        argument: &'static str,
    },
    Middleware {
        prefix: &'static str,
    },
}

/// One named-resource family. The table is authoritative metadata; compact
/// lookup matches below mirror its names to keep request-time dispatch O(1).
/// Exhaustive tests fail if the two representations drift.
#[derive(Debug)]
pub(crate) struct ConfigResourceDescriptor {
    pub kind: LaravelConfigResource,
    pub config_prefix: &'static str,
    pub label: &'static str,
    pub hover_label: &'static str,
    pub diagnostic_code: &'static str,
    pub triggers: &'static [ConfigResourceTrigger],
}

/// The semantic payload shared by completion and symbol extraction after a
/// declarative trigger matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceTriggerMatch {
    pub kind: LaravelConfigResource,
    pub argument: &'static str,
    pub shape: ResourceArgumentShape,
    pub access: ResourceAccess,
}

use ConfigResourceTrigger::{Attribute, Function, Middleware, StaticMethod};
use ResourceAccess::{OptionalRead, Read, Write};
use ResourceArgumentShape::{Array, Scalar, ScalarOrArray};

/// Every direct, config-backed Laravel string family PHPantom understands.
pub(crate) const CONFIG_RESOURCES: &[ConfigResourceDescriptor] = &[
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::AuthGuard,
        config_prefix: "auth.guards.",
        label: "auth guard",
        hover_label: "Auth guard",
        diagnostic_code: "invalid_laravel_auth_guard",
        triggers: &[
            Function {
                name: "auth",
                argument: "guard",
                shape: Scalar,
                access: Read,
            },
            StaticMethod {
                facade: "Auth",
                method: "guard",
                argument: "name",
                shape: Scalar,
                access: Read,
            },
            Attribute {
                name: "Auth",
                argument: "guard",
            },
            Attribute {
                name: "Authenticated",
                argument: "guard",
            },
            Middleware { prefix: "auth:" },
        ],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::CacheStore,
        config_prefix: "cache.stores.",
        label: "cache store",
        hover_label: "Cache store",
        diagnostic_code: "invalid_laravel_cache_store",
        triggers: &[
            StaticMethod {
                facade: "Cache",
                method: "store",
                argument: "name",
                shape: Scalar,
                access: Read,
            },
            Attribute {
                name: "Cache",
                argument: "store",
            },
        ],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::LogChannel,
        config_prefix: "logging.channels.",
        label: "log channel",
        hover_label: "Log channel",
        diagnostic_code: "invalid_laravel_log_channel",
        triggers: &[
            StaticMethod {
                facade: "Log",
                method: "channel",
                argument: "channel",
                shape: Scalar,
                access: Read,
            },
            StaticMethod {
                facade: "Log",
                method: "stack",
                argument: "channels",
                shape: Array,
                access: Read,
            },
            Attribute {
                name: "Log",
                argument: "channel",
            },
        ],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::StorageDisk,
        config_prefix: "filesystems.disks.",
        label: "storage disk",
        hover_label: "Storage disk",
        diagnostic_code: "invalid_laravel_storage_disk",
        triggers: &[
            StaticMethod {
                facade: "Storage",
                method: "disk",
                argument: "name",
                shape: Scalar,
                access: Read,
            },
            StaticMethod {
                facade: "Storage",
                method: "fake",
                argument: "disk",
                shape: Scalar,
                access: Write,
            },
            StaticMethod {
                facade: "Storage",
                method: "persistentFake",
                argument: "disk",
                shape: Scalar,
                access: Write,
            },
            StaticMethod {
                facade: "Storage",
                method: "forgetDisk",
                argument: "disk",
                shape: ScalarOrArray,
                access: OptionalRead,
            },
            Attribute {
                name: "Storage",
                argument: "disk",
            },
        ],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::DatabaseConnection,
        config_prefix: "database.connections.",
        label: "database connection",
        hover_label: "Database connection",
        diagnostic_code: "invalid_laravel_database_connection",
        triggers: &[
            StaticMethod {
                facade: "DB",
                method: "connection",
                argument: "name",
                shape: Scalar,
                access: Read,
            },
            Attribute {
                name: "Database",
                argument: "connection",
            },
            Attribute {
                name: "DB",
                argument: "connection",
            },
        ],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::QueueConnection,
        config_prefix: "queue.connections.",
        label: "queue connection",
        hover_label: "Queue connection",
        diagnostic_code: "invalid_laravel_queue_connection",
        triggers: &[StaticMethod {
            facade: "Queue",
            method: "connection",
            argument: "name",
            shape: Scalar,
            access: Read,
        }],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::Mailer,
        config_prefix: "mail.mailers.",
        label: "mailer",
        hover_label: "Mailer",
        diagnostic_code: "invalid_laravel_mailer",
        triggers: &[StaticMethod {
            facade: "Mail",
            method: "mailer",
            argument: "name",
            shape: Scalar,
            access: Read,
        }],
    },
    ConfigResourceDescriptor {
        kind: LaravelConfigResource::BroadcastConnection,
        config_prefix: "broadcasting.connections.",
        label: "broadcast connection",
        hover_label: "Broadcast connection",
        diagnostic_code: "invalid_laravel_broadcast_connection",
        triggers: &[StaticMethod {
            facade: "Broadcast",
            method: "connection",
            argument: "name",
            shape: Scalar,
            access: Read,
        }],
    },
];

pub(crate) fn descriptor(kind: LaravelConfigResource) -> &'static ConfigResourceDescriptor {
    match kind {
        LaravelConfigResource::AuthGuard => &CONFIG_RESOURCES[0],
        LaravelConfigResource::CacheStore => &CONFIG_RESOURCES[1],
        LaravelConfigResource::LogChannel => &CONFIG_RESOURCES[2],
        LaravelConfigResource::StorageDisk => &CONFIG_RESOURCES[3],
        LaravelConfigResource::DatabaseConnection => &CONFIG_RESOURCES[4],
        LaravelConfigResource::QueueConnection => &CONFIG_RESOURCES[5],
        LaravelConfigResource::Mailer => &CONFIG_RESOURCES[6],
        LaravelConfigResource::BroadcastConnection => &CONFIG_RESOURCES[7],
    }
}

/// Build the dot key used by Laravel's config index for one short resource
/// name. This allocation is paid only at a config boundary, never per stored
/// symbol span.
pub(crate) fn config_key(kind: LaravelConfigResource, short_name: &str) -> String {
    let prefix = descriptor(kind).config_prefix;
    let child = configured_child_name(kind, short_name);
    let mut key = String::with_capacity(prefix.len() + child.len());
    key.push_str(prefix);
    key.push_str(child);
    key
}

/// The config-array child selected by a source spelling.
///
/// Laravel database connections accept a role suffix while still reading the
/// base connection's configuration (`mysql::read`, `::write`, or `::direct`).
pub(crate) fn configured_child_name(kind: LaravelConfigResource, source_name: &str) -> &str {
    if kind == LaravelConfigResource::DatabaseConnection {
        for suffix in DATABASE_ROLE_SUFFIXES {
            if let Some(base) = source_name.strip_suffix(suffix) {
                return base;
            }
        }
    }
    source_name
}

/// Database connection role suffixes recognized by Laravel's manager.
pub(crate) const DATABASE_ROLE_SUFFIXES: &[&str] = &["::read", "::write", "::direct"];

/// Runtime-provided names that are valid without a matching config child.
pub(crate) fn is_implicit_resource_name(kind: LaravelConfigResource, name: &str) -> bool {
    name == "null"
        && matches!(
            kind,
            LaravelConfigResource::CacheStore | LaravelConfigResource::QueueConnection
        )
}

/// Whether two direct spellings select the same configured resource.
pub(crate) fn same_resource_name(kind: LaravelConfigResource, left: &str, right: &str) -> bool {
    configured_child_name(kind, left) == configured_child_name(kind, right)
}

/// Whether `full_key` is the config address of `short_name` in `kind`.
pub(crate) fn matches_config_key(
    kind: LaravelConfigResource,
    short_name: &str,
    full_key: &str,
) -> bool {
    if is_implicit_resource_name(kind, short_name) {
        return false;
    }
    full_key
        .strip_prefix(descriptor(kind).config_prefix)
        .is_some_and(|rest| rest == configured_child_name(kind, short_name))
}

/// Interpret a generic config key as a direct resource child.
pub(crate) fn resource_from_config_key(full_key: &str) -> Option<(LaravelConfigResource, &str)> {
    let root = full_key.split_once('.')?.0;
    let kind = config_root_resource(root)?;
    let short = full_key.strip_prefix(descriptor(kind).config_prefix)?;
    (!short.is_empty() && !short.contains('.') && !is_implicit_resource_name(kind, short))
        .then_some((kind, short))
}

pub(crate) fn function_trigger(name: &str) -> Option<ResourceTriggerMatch> {
    trigger_match(
        descriptor(LaravelConfigResource::AuthGuard),
        |trigger| match trigger {
            Function {
                name: expected,
                argument,
                shape,
                access,
            } if name.eq_ignore_ascii_case(expected) => Some((*argument, *shape, *access)),
            _ => None,
        },
    )
}

/// Cheap method-name prefilter for the live completion path.
pub(crate) fn static_method_may_trigger(method: &str) -> bool {
    match method.len() {
        4 => method.eq_ignore_ascii_case("disk") || method.eq_ignore_ascii_case("fake"),
        5 => {
            method.eq_ignore_ascii_case("guard")
                || method.eq_ignore_ascii_case("store")
                || method.eq_ignore_ascii_case("stack")
        }
        6 => method.eq_ignore_ascii_case("mailer"),
        7 => method.eq_ignore_ascii_case("channel"),
        10 => {
            method.eq_ignore_ascii_case("connection") || method.eq_ignore_ascii_case("forgetDisk")
        }
        14 => method.eq_ignore_ascii_case("persistentFake"),
        _ => false,
    }
}

/// Resolve a written function call to Laravel's global `auth()` helper.
///
/// PHP falls back to a global function only when the current namespace has no
/// same-named function. Semantic names settle aliases and same-file shadows;
/// the optional index membership check handles a shadow declared elsewhere.
pub(crate) fn auth_helper_trigger(
    content: &str,
    written_name: &str,
    offset: u32,
    resolved_names: Option<&OwnedResolvedNames>,
    indexed_function_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<ResourceTriggerMatch> {
    if resolved_names
        .and_then(|names| names.get(offset))
        .is_some_and(|resolved| resolved.eq_ignore_ascii_case("auth"))
    {
        return function_trigger("auth");
    }
    if !written_name.eq_ignore_ascii_case("auth") {
        return None;
    }
    function_trigger(written_name).filter(|_| {
        matches_laravel_auth_helper(content, offset, resolved_names, indexed_function_exists)
    })
}

fn matches_laravel_auth_helper(
    content: &str,
    offset: u32,
    resolved_names: Option<&OwnedResolvedNames>,
    indexed_function_exists: Option<&dyn Fn(&str) -> bool>,
) -> bool {
    let Some(names) = resolved_names else {
        return true;
    };
    let Some(resolved) = names.get(offset) else {
        return false;
    };
    if resolved.eq_ignore_ascii_case("auth") {
        return true;
    }
    if names.is_imported(offset) {
        return false;
    }
    if names.iter().any(|(declaration_offset, name, _)| {
        name.eq_ignore_ascii_case(resolved)
            && is_named_function_declaration(content, declaration_offset)
    }) {
        return false;
    }
    !indexed_function_exists.is_some_and(|exists| exists(resolved))
}

fn is_named_function_declaration(content: &str, offset: u32) -> bool {
    let Some(before_name) = content.as_bytes().get(..offset as usize) else {
        return false;
    };
    let mut end = skip_php_trivia_backwards(before_name, before_name.len());
    if end > 0 && before_name[end - 1] == b'&' {
        end = skip_php_trivia_backwards(before_name, end - 1);
    }
    let start = end.saturating_sub("function".len());
    before_name[start..end].eq_ignore_ascii_case(b"function")
        && (start == 0
            || !(before_name[start - 1].is_ascii_alphanumeric() || before_name[start - 1] == b'_'))
}

fn skip_php_trivia_backwards(bytes: &[u8], mut end: usize) -> usize {
    loop {
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }

        if end >= 2
            && &bytes[end - 2..end] == b"*/"
            && let Some(start) = bytes[..end - 2].windows(2).rposition(|pair| pair == b"/*")
        {
            end = start;
            continue;
        }

        let line_start = bytes[..end]
            .iter()
            .rposition(|byte| *byte == b'\n' || *byte == b'\r')
            .map_or(0, |index| index + 1);
        let line = &bytes[line_start..end];
        if let Some(comment) = line.windows(2).rposition(|pair| pair == b"//") {
            end = line_start + comment;
            continue;
        }
        if let Some(comment) = line.iter().rposition(|byte| *byte == b'#') {
            end = line_start + comment;
            continue;
        }

        return end;
    }
}

pub(crate) fn static_method_trigger(receiver: &str, method: &str) -> Option<ResourceTriggerMatch> {
    let receiver = receiver.trim_start_matches('\\');
    let short = if let Some((namespace, short)) = receiver.rsplit_once('\\') {
        if !namespace.eq_ignore_ascii_case("Illuminate\\Support\\Facades") {
            return None;
        }
        short
    } else {
        receiver
    };
    let resource = descriptor(static_facade_resource(short)?);
    trigger_match(resource, |trigger| match trigger {
        StaticMethod {
            facade,
            method: expected,
            argument,
            shape,
            access,
        } if short.eq_ignore_ascii_case(facade) && method.eq_ignore_ascii_case(expected) => {
            Some((*argument, *shape, *access))
        }
        _ => None,
    })
}

pub(crate) fn attribute_trigger(name: &str) -> Option<ResourceTriggerMatch> {
    let name = name.trim_start_matches('\\');
    let short = if let Some((namespace, short)) = name.rsplit_once('\\') {
        if !namespace.eq_ignore_ascii_case("Illuminate\\Container\\Attributes") {
            return None;
        }
        short
    } else {
        name
    };
    let resource = descriptor(attribute_resource(short)?);
    trigger_match(resource, |trigger| match trigger {
        Attribute {
            name: expected,
            argument,
        } if short.eq_ignore_ascii_case(expected) => Some((
            *argument,
            ResourceArgumentShape::Scalar,
            ResourceAccess::Read,
        )),
        _ => None,
    })
}

pub(crate) fn middleware_resource(prefix: &str) -> Option<LaravelConfigResource> {
    if prefix != "auth:" {
        return None;
    }
    let resource = descriptor(LaravelConfigResource::AuthGuard);
    resource.triggers.iter().find_map(|trigger| match trigger {
        Middleware { prefix: expected } if prefix == *expected => Some(resource.kind),
        _ => None,
    })
}

fn trigger_match(
    resource: &ConfigResourceDescriptor,
    mut select: impl FnMut(
        &ConfigResourceTrigger,
    ) -> Option<(&'static str, ResourceArgumentShape, ResourceAccess)>,
) -> Option<ResourceTriggerMatch> {
    resource.triggers.iter().find_map(|trigger| {
        let (argument, shape, access) = select(trigger)?;
        Some(ResourceTriggerMatch {
            kind: resource.kind,
            argument,
            shape,
            access,
        })
    })
}

fn config_root_resource(root: &str) -> Option<LaravelConfigResource> {
    use LaravelConfigResource::*;
    match root.len() {
        4 if root == "auth" => Some(AuthGuard),
        4 if root == "mail" => Some(Mailer),
        5 if root == "cache" => Some(CacheStore),
        5 if root == "queue" => Some(QueueConnection),
        7 if root == "logging" => Some(LogChannel),
        8 if root == "database" => Some(DatabaseConnection),
        11 if root == "filesystems" => Some(StorageDisk),
        12 if root == "broadcasting" => Some(BroadcastConnection),
        _ => None,
    }
}

fn static_facade_resource(name: &str) -> Option<LaravelConfigResource> {
    use LaravelConfigResource::*;
    match name.len() {
        2 if name.eq_ignore_ascii_case("DB") => Some(DatabaseConnection),
        3 if name.eq_ignore_ascii_case("Log") => Some(LogChannel),
        4 if name.eq_ignore_ascii_case("Auth") => Some(AuthGuard),
        4 if name.eq_ignore_ascii_case("Mail") => Some(Mailer),
        5 if name.eq_ignore_ascii_case("Cache") => Some(CacheStore),
        5 if name.eq_ignore_ascii_case("Queue") => Some(QueueConnection),
        7 if name.eq_ignore_ascii_case("Storage") => Some(StorageDisk),
        9 if name.eq_ignore_ascii_case("Broadcast") => Some(BroadcastConnection),
        _ => None,
    }
}

fn attribute_resource(name: &str) -> Option<LaravelConfigResource> {
    use LaravelConfigResource::*;
    match name.len() {
        2 if name.eq_ignore_ascii_case("DB") => Some(DatabaseConnection),
        3 if name.eq_ignore_ascii_case("Log") => Some(LogChannel),
        4 if name.eq_ignore_ascii_case("Auth") => Some(AuthGuard),
        5 if name.eq_ignore_ascii_case("Cache") => Some(CacheStore),
        7 if name.eq_ignore_ascii_case("Storage") => Some(StorageDisk),
        8 if name.eq_ignore_ascii_case("Database") => Some(DatabaseConnection),
        13 if name.eq_ignore_ascii_case("Authenticated") => Some(AuthGuard),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_names(content: &str) -> OwnedResolvedNames {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
        let resolver = mago_names::resolver::NameResolver::new(&arena);
        OwnedResolvedNames::from_resolved(&resolver.resolve(program))
    }

    #[test]
    fn every_resource_has_one_unique_prefix_and_descriptor() {
        let mut family_bits = 0_u8;
        for (index, resource) in CONFIG_RESOURCES.iter().enumerate() {
            assert!(resource.config_prefix.ends_with('.'));
            assert_eq!(descriptor(resource.kind).kind, resource.kind);
            assert!(CONFIG_RESOURCES[..index].iter().all(|seen| {
                seen.kind != resource.kind && seen.config_prefix != resource.config_prefix
            }));
            assert_eq!(family_bits & resource.kind.bit(), 0);
            family_bits |= resource.kind.bit();
        }
        assert_eq!(family_bits, u8::MAX);
    }

    #[test]
    fn every_declarative_trigger_is_reachable_through_its_fast_lookup() {
        for resource in CONFIG_RESOURCES {
            let root = resource.config_prefix.split('.').next().unwrap();
            assert_eq!(config_root_resource(root), Some(resource.kind));
            for trigger in resource.triggers {
                let (found, expected) = match trigger {
                    Function {
                        name,
                        argument,
                        shape,
                        access,
                    } => (
                        function_trigger(name),
                        ResourceTriggerMatch {
                            kind: resource.kind,
                            argument,
                            shape: *shape,
                            access: *access,
                        },
                    ),
                    StaticMethod {
                        facade,
                        method,
                        argument,
                        shape,
                        access,
                    } => {
                        assert!(
                            RESOURCE_FACADES
                                .iter()
                                .any(|candidate| candidate.eq_ignore_ascii_case(facade))
                        );
                        assert!(static_method_may_trigger(method));
                        let expected = ResourceTriggerMatch {
                            kind: resource.kind,
                            argument,
                            shape: *shape,
                            access: *access,
                        };
                        let short = static_method_trigger(facade, method);
                        let fqn = format!("Illuminate\\Support\\Facades\\{facade}");
                        assert_eq!(static_method_trigger(&fqn, method), Some(expected));
                        (short, expected)
                    }
                    Attribute { name, argument } => {
                        assert!(
                            RESOURCE_ATTRIBUTES
                                .iter()
                                .any(|candidate| candidate.eq_ignore_ascii_case(name))
                        );
                        (
                            attribute_trigger(name),
                            ResourceTriggerMatch {
                                kind: resource.kind,
                                argument,
                                shape: ResourceArgumentShape::Scalar,
                                access: ResourceAccess::Read,
                            },
                        )
                    }
                    Middleware { prefix } => {
                        assert_eq!(middleware_resource(prefix), Some(resource.kind));
                        continue;
                    }
                };
                assert_eq!(found, Some(expected));
            }
        }
    }

    #[test]
    fn trigger_metadata_preserves_argument_shapes_and_access_modes() {
        for (method, access) in [
            ("disk", ResourceAccess::Read),
            ("fake", ResourceAccess::Write),
            ("persistentFake", ResourceAccess::Write),
            ("forgetDisk", ResourceAccess::OptionalRead),
        ] {
            assert_eq!(
                static_method_trigger("Storage", method).map(|found| found.access),
                Some(access)
            );
        }
        assert_eq!(
            static_method_trigger("Log", "stack").map(|found| found.shape),
            Some(ResourceArgumentShape::Array)
        );
        assert_eq!(
            static_method_trigger("Storage", "forgetDisk").map(|found| found.shape),
            Some(ResourceArgumentShape::ScalarOrArray)
        );
    }

    #[test]
    fn trigger_lookups_are_case_insensitive_and_context_specific() {
        assert_eq!(
            function_trigger("AUTH").map(|found| found.kind),
            Some(LaravelConfigResource::AuthGuard)
        );
        assert_eq!(
            static_method_trigger("ILLUMINATE\\SUPPORT\\FACADES\\CACHE", "STORE")
                .map(|found| found.kind),
            Some(LaravelConfigResource::CacheStore)
        );
        assert_eq!(
            static_method_trigger("\\Illuminate\\Support\\Facades\\DB", "connection")
                .map(|found| found.kind),
            Some(LaravelConfigResource::DatabaseConnection)
        );
        assert_eq!(
            attribute_trigger("ILLUMINATE\\CONTAINER\\ATTRIBUTES\\AUTHENTICATED")
                .map(|found| found.kind),
            Some(LaravelConfigResource::AuthGuard)
        );
        assert_eq!(
            attribute_trigger("\\Illuminate\\Container\\Attributes\\Log").map(|found| found.kind),
            Some(LaravelConfigResource::LogChannel)
        );
        assert!(middleware_resource("AUTH:").is_none());
        assert!(function_trigger("guard").is_none());
        assert!(static_method_trigger("Queue", "mailer").is_none());
        assert!(static_method_trigger("Acme\\Log", "stack").is_none());
        assert!(attribute_trigger("Acme\\Cache").is_none());
        assert!(attribute_trigger("Unknown").is_none());
        assert!(middleware_resource("throttle:").is_none());
    }

    #[test]
    fn auth_helper_resolution_honours_aliases_and_namespace_shadows() {
        let global = "<?php namespace App; \\auth();";
        let names = resolved_names(global);
        let offset = global.find("\\auth").unwrap() as u32;
        assert!(auth_helper_trigger(global, "auth", offset, Some(&names), None).is_some());
        assert!(matches_laravel_auth_helper(
            global,
            offset,
            Some(&names),
            None,
        ));
        assert!(auth_helper_trigger(global, "auth", u32::MAX, Some(&names), None).is_none());

        let alias = "<?php namespace App; use function auth as laravel_auth; laravel_auth();";
        let names = resolved_names(alias);
        let offset = alias.rfind("laravel_auth").unwrap() as u32;
        assert!(auth_helper_trigger(alias, "laravel_auth", offset, Some(&names), None).is_some());

        let imported = "<?php namespace App; use function Vendor\\auth; auth();";
        let names = resolved_names(imported);
        let offset = imported.rfind("auth").unwrap() as u32;
        assert!(auth_helper_trigger(imported, "auth", offset, Some(&names), None).is_none());

        let local = "<?php namespace App; function & auth() {} auth();";
        let names = resolved_names(local);
        let offset = local.rfind("auth").unwrap() as u32;
        assert!(auth_helper_trigger(local, "auth", offset, Some(&names), None).is_none());

        let commented =
            "<?php namespace App; function /* first */ & /* second */ auth() {} auth();";
        let names = resolved_names(commented);
        let declaration = commented.find("auth").unwrap() as u32;
        let offset = commented.rfind("auth").unwrap() as u32;
        assert!(is_named_function_declaration(commented, declaration));
        assert!(auth_helper_trigger(commented, "auth", offset, Some(&names), None).is_none());

        let line_comment = "<?php namespace App; function // comment\n auth() {} auth();";
        let declaration = line_comment.find("auth").unwrap() as u32;
        assert!(is_named_function_declaration(line_comment, declaration));

        let hash_comment = "<?php namespace App; function # comment\n auth() {} auth();";
        let declaration = hash_comment.find("auth").unwrap() as u32;
        assert!(is_named_function_declaration(hash_comment, declaration));

        let fallback = "<?php namespace App; auth();";
        let names = resolved_names(fallback);
        let offset = fallback.rfind("auth").unwrap() as u32;
        assert!(auth_helper_trigger(fallback, "auth", offset, Some(&names), None).is_some());
        assert!(
            auth_helper_trigger(
                fallback,
                "auth",
                offset,
                Some(&names),
                Some(&|name| name == "App\\auth"),
            )
            .is_none()
        );

        assert!(auth_helper_trigger("<?php auth();", "auth", 6, None, None).is_some());
        assert!(auth_helper_trigger("<?php helper();", "helper", 6, None, None).is_none());
        assert!(!is_named_function_declaration("<?php auth();", u32::MAX));
    }

    #[test]
    fn canonical_config_conversion_is_exact_and_symmetric() {
        for resource in CONFIG_RESOURCES {
            let full = config_key(resource.kind, "named");
            assert_eq!(
                resource_from_config_key(&full),
                Some((resource.kind, "named"))
            );
            assert!(matches_config_key(resource.kind, "named", &full));
            assert!(!matches_config_key(resource.kind, "other", &full));
            assert!(resource_from_config_key(&format!("{full}.option")).is_none());
        }
        assert!(resource_from_config_key("app.name").is_none());
        assert!(resource_from_config_key("auth").is_none());
        assert!(resource_from_config_key("auth.guards.").is_none());

        for suffix in DATABASE_ROLE_SUFFIXES {
            let source = format!("mysql{suffix}");
            assert_eq!(
                config_key(LaravelConfigResource::DatabaseConnection, &source),
                "database.connections.mysql"
            );
            assert!(matches_config_key(
                LaravelConfigResource::DatabaseConnection,
                &source,
                "database.connections.mysql",
            ));
            assert!(same_resource_name(
                LaravelConfigResource::DatabaseConnection,
                &source,
                "mysql",
            ));
        }
        assert_eq!(
            configured_child_name(LaravelConfigResource::DatabaseConnection, "mysql::replica"),
            "mysql::replica"
        );

        for resource in [
            LaravelConfigResource::CacheStore,
            LaravelConfigResource::QueueConnection,
        ] {
            assert!(is_implicit_resource_name(resource, "null"));
            assert!(!matches_config_key(
                resource,
                "null",
                &format!("{}null", descriptor(resource).config_prefix),
            ));
        }
        assert!(!is_implicit_resource_name(
            LaravelConfigResource::DatabaseConnection,
            "null"
        ));
        assert!(resource_from_config_key("cache.stores.null").is_none());
        assert!(resource_from_config_key("queue.connections.null").is_none());
    }
}
