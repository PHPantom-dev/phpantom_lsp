//! Keeps `config-schema.json` in step with the [`Config`] structs.
//!
//! The schema is written by hand, so a key added to a struct and not to
//! the schema leaves editors without completion or hover for it, and a
//! key removed from a struct leaves the schema offering one that does
//! nothing.  The test below collects the dotted key paths on both sides
//! and fails on any difference.
//!
//! The struct side is read from serde itself: a derived `Deserialize`
//! hands the deserializer the full list of field names (after renames)
//! in `deserialize_struct`, so [`KeyTracer`] records that list and then
//! feeds each field a tracer of its own to reach nested sections.

use std::cell::RefCell;
use std::collections::BTreeSet;

use serde::de::value::Error;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use super::Config;

/// Path segment standing for "an element of this array", on both sides.
const ARRAY_ITEM: &str = "[]";

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Deserializer that produces no data and records the field paths of
/// every struct it is asked to build.
///
/// `Option` fields are answered with `None`, so the leaf types behind
/// them (including the hand-written enum deserializers, which would
/// reject an invented string) are never reached.  An array is answered
/// with a single element so the struct inside `Vec<Struct>` is traced.
struct KeyTracer<'a> {
    path: String,
    keys: &'a RefCell<BTreeSet<String>>,
}

impl<'de> Deserializer<'de> for KeyTracer<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(de::Error::custom(format!(
            "`{}` has a type the schema test does not know how to trace; \
             teach KeyTracer about it",
            self.path
        )))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_none()
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_bool(false)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u64(0)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str("")
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str("")
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(OneItem {
            path: Some(join(&self.path, ARRAY_ITEM)),
            keys: self.keys,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.keys
            .borrow_mut()
            .extend(fields.iter().map(|f| join(&self.path, f)));
        visitor.visit_map(Fields {
            path: self.path,
            fields: fields.iter(),
            current: "",
            keys: self.keys,
        })
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i32 i64 i128 u8 u16 u32 u128 f32 f64 char bytes byte_buf
        unit unit_struct newtype_struct tuple tuple_struct map enum
        identifier ignored_any
    }
}

/// Map access over a struct's field names, each value traced in turn.
struct Fields<'a> {
    path: String,
    fields: std::slice::Iter<'static, &'static str>,
    current: &'static str,
    keys: &'a RefCell<BTreeSet<String>>,
}

impl<'de> MapAccess<'de> for Fields<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some(&field) = self.fields.next() else {
            return Ok(None);
        };
        self.current = field;
        seed.deserialize(de::value::BorrowedStrDeserializer::new(field))
            .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(KeyTracer {
            path: join(&self.path, self.current),
            keys: self.keys,
        })
    }
}

/// Sequence access yielding exactly one traced element.
struct OneItem<'a> {
    path: Option<String>,
    keys: &'a RefCell<BTreeSet<String>>,
}

impl<'de> SeqAccess<'de> for OneItem<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(path) = self.path.take() else {
            return Ok(None);
        };
        seed.deserialize(KeyTracer {
            path,
            keys: self.keys,
        })
        .map(Some)
    }
}

/// Dotted paths of every key the config structs accept.
fn struct_keys() -> BTreeSet<String> {
    let keys = RefCell::new(BTreeSet::new());
    <Config as de::Deserialize>::deserialize(KeyTracer {
        path: String::new(),
        keys: &keys,
    })
    .expect("tracing the config structs should not fail");
    keys.into_inner()
}

/// Dotted paths of every key `config-schema.json` describes.
fn schema_keys() -> BTreeSet<String> {
    fn walk(node: &serde_json::Value, path: &str, out: &mut BTreeSet<String>) {
        if let Some(properties) = node.get("properties").and_then(|p| p.as_object()) {
            for (key, child) in properties {
                let child_path = join(path, key);
                walk(child, &child_path, out);
                out.insert(child_path);
            }
        }
        if let Some(items) = node.get("items") {
            walk(items, &join(path, ARRAY_ITEM), out);
        }
    }

    let schema: serde_json::Value = serde_json::from_str(include_str!("../../config-schema.json"))
        .expect("config-schema.json should be valid JSON");
    let mut keys = BTreeSet::new();
    walk(&schema, "", &mut keys);
    keys
}

#[test]
fn schema_and_config_structs_describe_the_same_keys() {
    let structs = struct_keys();
    let schema = schema_keys();

    let missing_from_schema: Vec<_> = structs.difference(&schema).collect();
    let missing_from_structs: Vec<_> = schema.difference(&structs).collect();

    assert!(
        missing_from_schema.is_empty() && missing_from_structs.is_empty(),
        "config-schema.json and the config structs disagree.\n\
         Accepted by the structs but missing from the schema: {missing_from_schema:?}\n\
         In the schema but not accepted by the structs: {missing_from_structs:?}"
    );
}

/// Guards the tracer itself: if it stopped descending into nested
/// sections or array items, the comparison above would pass on two
/// equally truncated sets of top-level keys.
#[test]
fn struct_keys_reach_nested_sections_and_array_items() {
    let keys = struct_keys();
    for expected in [
        "diagnostics.unresolved-member-access",
        "diagnostics.ignore.[].identifier",
        "laravel.migrations.paths",
        "semantic-tokens.mode",
    ] {
        assert!(keys.contains(expected), "missing `{expected}` in {keys:?}");
    }
}
