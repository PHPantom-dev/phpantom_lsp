//! Laravel attributes whose arguments name a key: the container-injection
//! attributes (`#[Config('app.name')]`, `#[Storage('local')]`, …) and a form
//! request's `#[RedirectToRoute('login')]`.

/// Namespace prefix for Laravel's container-injection attributes.
const LARAVEL_CONTAINER_ATTR_NS: &str = "Illuminate\\Container\\Attributes\\";

/// The Laravel-specific meaning of a container-injection attribute.
pub(in crate::symbol_map::extraction) enum LaravelContainerAttribute {
    /// A full config key accepted by `#[Config]`.
    Config,
    /// A named config resource accepted by the matching container attribute.
    Resource(crate::symbol_map::laravel_resources::ResourceTriggerMatch),
}

/// Check whether an attribute class name refers to a Laravel container
/// attribute (`Config`, `Database`, `Cache`, `Log`, `Storage`, `Auth`,
/// `Authenticated`). Returns the corresponding Laravel-specific meaning.
///
/// FQN names (containing `\`) are matched directly against
/// `Illuminate\Container\Attributes\*`.  Short names require the file to
/// import from that namespace; the result of that check is cached in
/// `import_cache` to avoid repeated linear scans of the file content.
pub(in crate::symbol_map::extraction) fn resolve_laravel_container_attr(
    class_name: &str,
    allow_short_import_heuristic: bool,
    import_cache: &mut Option<bool>,
    content: &str,
) -> Option<LaravelContainerAttribute> {
    let short = if class_name.contains('\\') {
        let class_name = class_name.trim_start_matches('\\');
        let prefix = class_name.get(..LARAVEL_CONTAINER_ATTR_NS.len())?;
        if !prefix.eq_ignore_ascii_case(LARAVEL_CONTAINER_ATTR_NS) {
            return None;
        }
        class_name.get(LARAVEL_CONTAINER_ATTR_NS.len()..)?
    } else {
        if !allow_short_import_heuristic {
            return None;
        }
        let has_import = *import_cache
            .get_or_insert_with(|| content.contains("use Illuminate\\Container\\Attributes\\"));
        if !has_import {
            return None;
        }
        class_name
    };

    if short.eq_ignore_ascii_case("Config") {
        return Some(LaravelContainerAttribute::Config);
    }
    crate::symbol_map::laravel_resources::attribute_trigger(short)
        .map(LaravelContainerAttribute::Resource)
}

/// Namespace prefix for the attributes a form request is configured with.
const LARAVEL_HTTP_ATTR_NS: &str = "Illuminate\\Foundation\\Http\\Attributes\\";

/// Whether an attribute class name refers to `#[RedirectToRoute]`, whose one
/// argument names the route a failed form request bounces back to.
///
/// The short spelling requires the file to import from that namespace, cached
/// in `import_cache` the same way [`resolve_laravel_container_attr`] caches
/// its own: `RedirectToRoute` is a plausible name for an application's own
/// attribute.
pub(in crate::symbol_map::extraction) fn is_laravel_redirect_route_attr(
    class_name: &str,
    import_cache: &mut Option<bool>,
    content: &str,
) -> bool {
    if class_name.contains('\\') {
        return class_name.strip_prefix(LARAVEL_HTTP_ATTR_NS) == Some("RedirectToRoute");
    }
    if class_name != "RedirectToRoute" {
        return false;
    }
    *import_cache
        .get_or_insert_with(|| content.contains("use Illuminate\\Foundation\\Http\\Attributes\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_qualified_container_attributes_keep_their_distinct_meanings() {
        let mut import_cache = None;
        assert!(matches!(
            resolve_laravel_container_attr(
                "Illuminate\\Container\\Attributes\\Storage",
                false,
                &mut import_cache,
                ""
            ),
            Some(LaravelContainerAttribute::Resource(trigger))
                if trigger.kind == crate::symbol_map::LaravelConfigResource::StorageDisk
        ));
        assert!(matches!(
            resolve_laravel_container_attr(
                "Illuminate\\Container\\Attributes\\Config",
                false,
                &mut import_cache,
                ""
            ),
            Some(LaravelContainerAttribute::Config)
        ));
        assert!(
            resolve_laravel_container_attr("App\\Storage", false, &mut import_cache, "").is_none()
        );
    }

    #[test]
    fn storage_attribute_without_a_disk_argument_emits_no_config_key() {
        let php = r#"<?php
#[\Illuminate\Container\Attributes\Storage()]
class EmptyStorage {}
#[\Illuminate\Container\Attributes\Storage(config: 'archive')]
class WrongNamedArgument {}
"#;
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"storage-attributes.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let map = crate::symbol_map::extraction::extract_symbol_map(program, php);
        let archive = php.find("archive").expect("fixture should name a disk") as u32;
        assert!(map.lookup(archive).is_none());
    }
}
