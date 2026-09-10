//! PHP call hierarchy support built on the existing definition and reference
//! pipelines.
//!
//! The hierarchy stores only stable declaration coordinates in LSP item data.
//! Incoming calls reuse Find References; outgoing calls reuse Go to Definition
//! for call-like symbol spans inside the callable body. This keeps call
//! hierarchy aligned with every improvement made to the shared type engine.
//!
//! Everything in here works in virtual-PHP coordinates, the same as the rest
//! of the engine; each entry point translates the ranges it hands back to
//! Blade coordinates as its last step, while the byte offset carried in the
//! item's `data` stays virtual so a follow-up request lands where it should.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Location, Position,
    Range, SymbolKind as LspSymbolKind, Url,
};

use crate::Backend;
use crate::symbol_map::{ClassRefContext, SymbolKind, SymbolMap, SymbolSpan};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{ClassInfo, FunctionInfo, MethodInfo};

#[derive(Clone)]
struct PhpCallable {
    item: CallHierarchyItem,
    body: Option<(u32, u32)>,
}

impl PhpCallable {
    /// How much source the callable covers, used to pick the innermost
    /// candidate when an offset sits inside a method of an anonymous class
    /// nested in another method's body.
    fn width(&self) -> u32 {
        self.body
            .map_or(0, |(start, end)| end.saturating_sub(start))
    }
}

/// A file's source text paired with its symbol map.
type ParsedFile = (Arc<String>, Arc<SymbolMap>);

/// The text and symbol map of every file a single request touched.
///
/// A widely called method has its call sites spread over a handful of files,
/// and naming the caller at each one needs that file's source. Fetching it per
/// call site would re-read the same file from disk once per site, so each file
/// is fetched at most once per request.
#[derive(Default)]
struct FileCache {
    files: HashMap<String, Option<ParsedFile>>,
}

impl Backend {
    pub(crate) fn prepare_call_hierarchy_impl(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<CallHierarchyItem>> {
        let mut cache = FileCache::default();
        let offset = position_to_offset(content, position);

        let mut item = match self
            .symbol_map_for(uri)
            .and_then(|symbol_map| self.php_callable_at(uri, content, &symbol_map, offset))
        {
            Some(callable) => callable.item,
            None => {
                self.resolve_definition(uri, content, position)
                    .into_iter()
                    .find_map(|location| self.php_callable_at_location(&mut cache, &location))?
                    .item
            }
        };

        self.translate_item_to_blade(&mut item);
        Some(vec![item])
    }

    pub(crate) fn incoming_calls_impl(
        &self,
        item: &CallHierarchyItem,
    ) -> Option<Vec<CallHierarchyIncomingCall>> {
        let mut cache = FileCache::default();
        let target = self.php_callable_from_item(&mut cache, item)?;
        let (content, _) = self.cached_file(&mut cache, target.item.uri.as_str())?;
        let references = self
            .find_references(
                target.item.uri.as_str(),
                &content,
                target.item.selection_range.start,
                false,
            )
            .unwrap_or_default();

        let mut grouped: HashMap<String, (CallHierarchyItem, Vec<Range>)> = HashMap::new();
        for reference in references {
            let uri = reference.uri.as_str();
            let Some((ref_content, symbol_map)) = self.cached_file(&mut cache, uri) else {
                continue;
            };
            let offset = position_to_offset(&ref_content, reference.range.start);
            // Find References answers for the whole member, so a same-named
            // property read and a `@see` tag naming the method come back
            // alongside the calls.  Only the calls belong in a hierarchy.
            if !symbol_map.lookup(offset).is_some_and(is_call_site) {
                continue;
            }
            let Some(caller) = self.php_callable_at(uri, &ref_content, &symbol_map, offset) else {
                continue;
            };
            let key = php_item_key(&caller.item);
            grouped
                .entry(key)
                .and_modify(|(_, ranges)| push_unique_range(ranges, reference.range))
                .or_insert_with(|| (caller.item, vec![reference.range]));
        }

        let mut calls: Vec<_> = grouped
            .into_values()
            .map(|(mut from, mut from_ranges)| {
                self.translate_ranges_to_blade(from.uri.as_str(), &mut from_ranges);
                self.translate_item_to_blade(&mut from);
                CallHierarchyIncomingCall { from, from_ranges }
            })
            .collect();
        calls.sort_by_cached_key(|call| php_item_key(&call.from));
        Some(calls)
    }

    pub(crate) fn outgoing_calls_impl(
        &self,
        item: &CallHierarchyItem,
    ) -> Option<Vec<CallHierarchyOutgoingCall>> {
        let mut cache = FileCache::default();
        let callable = self.php_callable_from_item(&mut cache, item)?;
        let Some((body_start, body_end)) = callable.body else {
            return Some(Vec::new());
        };
        let uri = callable.item.uri.as_str();
        let (content, symbol_map) = self.cached_file(&mut cache, uri)?;

        let call_sites: Vec<SymbolSpan> = symbol_map
            .spans
            .iter()
            .filter(|span| span.start >= body_start && span.start <= body_end && is_call_site(span))
            .cloned()
            .collect();

        let mut grouped: HashMap<String, (CallHierarchyItem, Vec<Range>)> = HashMap::new();
        for span in call_sites {
            let position = offset_to_position(&content, span.start as usize);
            let from_range = Range::new(position, offset_to_position(&content, span.end as usize));

            // `new Foo(...)` resolves to the class, not to the constructor the
            // hierarchy wants, so the constructor is looked up directly.
            let callees: Vec<PhpCallable> =
                if matches!(span.kind, SymbolKind::ClassReference { .. }) {
                    self.constructor_callable(&mut cache, uri, &span)
                        .into_iter()
                        .collect()
                } else {
                    self.resolve_definition(uri, &content, position)
                        .into_iter()
                        .filter_map(|location| self.php_callable_at_location(&mut cache, &location))
                        .collect()
                };

            for callee in callees {
                let key = php_item_key(&callee.item);
                grouped
                    .entry(key)
                    .and_modify(|(_, ranges)| push_unique_range(ranges, from_range))
                    .or_insert_with(|| (callee.item, vec![from_range]));
            }
        }

        let mut calls: Vec<_> = grouped
            .into_values()
            .map(|(mut to, mut from_ranges)| {
                // `from_ranges` are call sites in the callable being
                // inspected, not in the callee's file.
                self.translate_ranges_to_blade(uri, &mut from_ranges);
                self.translate_item_to_blade(&mut to);
                CallHierarchyOutgoingCall { to, from_ranges }
            })
            .collect();
        calls.sort_by_cached_key(|call| php_item_key(&call.to));
        Some(calls)
    }

    /// Resolve the constructor a `new Foo(...)` span invokes.
    ///
    /// A class that declares no constructor of its own still runs the nearest
    /// one it inherits, so the parent chain is walked until a declaration
    /// turns up.  The visited set stops a `class A extends B` / `class B
    /// extends A` cycle in broken source from spinning.
    fn constructor_callable(
        &self,
        cache: &mut FileCache,
        uri: &str,
        span: &SymbolSpan,
    ) -> Option<PhpCallable> {
        let SymbolKind::ClassReference { name, is_fqn, .. } = &span.kind else {
            return None;
        };
        let mut next = Some(if *is_fqn {
            name.trim_start_matches('\\').to_string()
        } else {
            self.file_context(uri).resolve_name_at(name, span.start)
        });

        let mut visited: HashSet<String> = HashSet::new();
        while let Some(fqn) = next.take() {
            if !visited.insert(fqn.clone()) {
                return None;
            }
            let class = self.find_or_load_class(&fqn)?;
            // A constructor the class inherited carries the declaring file's
            // offsets, so feeding it back through the callable lookup finds
            // nothing here and the walk moves on to the parent.
            if let Some(constructor) = class.get_method("__construct")
                && constructor.name_offset != 0
                && !constructor.is_virtual
                && let Some((class_uri, _)) = self.find_class_file_content(&fqn, "", "")
                && let Some((content, symbol_map)) = self.cached_file(cache, &class_uri)
                && let Some(callable) =
                    self.php_callable_at(&class_uri, &content, &symbol_map, constructor.name_offset)
                && callable.item.name.eq_ignore_ascii_case("__construct")
            {
                return Some(callable);
            }
            next = class.parent_class.as_ref().map(|parent| parent.to_string());
        }
        None
    }

    fn php_callable_from_item(
        &self,
        cache: &mut FileCache,
        item: &CallHierarchyItem,
    ) -> Option<PhpCallable> {
        let data = item.data.as_ref()?;
        if data.get("kind")?.as_str()? != "php" {
            return None;
        }
        let offset = data.get("offset")?.as_u64()? as u32;
        let uri = item.uri.as_str();
        let (content, symbol_map) = self.cached_file(cache, uri)?;
        self.php_callable_at(uri, &content, &symbol_map, offset)
    }

    fn php_callable_at_location(
        &self,
        cache: &mut FileCache,
        location: &Location,
    ) -> Option<PhpCallable> {
        let uri = location.uri.as_str();
        let (content, symbol_map) = self.cached_file(cache, uri)?;
        let offset = position_to_offset(&content, location.range.start);
        self.php_callable_at(uri, &content, &symbol_map, offset)
    }

    /// The innermost function or method whose name token or body contains
    /// `offset`.
    fn php_callable_at(
        &self,
        uri: &str,
        content: &str,
        symbol_map: &SymbolMap,
        offset: u32,
    ) -> Option<PhpCallable> {
        let classes = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();

        // An anonymous class declared inside a method body puts its own
        // methods inside that method's span, so both contain the offset and
        // the narrower one is the answer.
        let mut best: Option<PhpCallable> = None;
        let mut consider = |candidate: Option<PhpCallable>| {
            if let Some(candidate) = candidate
                && best
                    .as_ref()
                    .is_none_or(|current| candidate.width() < current.width())
            {
                best = Some(candidate);
            }
        };

        for class in &classes {
            consider(method_callable_at(uri, content, symbol_map, class, offset));
        }

        let function_names = self
            .symbols
            .uri_globals_index
            .read()
            .get(uri)
            .map(|(functions, _)| functions.clone())
            .unwrap_or_default();
        let functions = self.symbols.global_functions.read();
        for fqn in function_names {
            let Some((declaring_uri, function)) = functions.get(&fqn) else {
                continue;
            };
            if declaring_uri == uri {
                consider(function_callable_at(
                    uri, content, symbol_map, &fqn, function, offset,
                ));
            }
        }
        best
    }

    fn cached_file(&self, cache: &mut FileCache, uri: &str) -> Option<ParsedFile> {
        if let Some(cached) = cache.files.get(uri) {
            return cached.clone();
        }
        let entry = self.call_hierarchy_file(uri);
        cache.files.insert(uri.to_string(), entry.clone());
        entry
    }

    fn call_hierarchy_file(&self, uri: &str) -> Option<ParsedFile> {
        let content = self.reference_file_content_arc(uri)?;
        if let Some(symbol_map) = self.symbol_map_for(uri) {
            return Some((content, symbol_map));
        }
        // A call site can sit in a file the background indexer has not reached
        // yet.  Parse it now, the way the first request on a freshly opened
        // file does, so the caller can still be named.  A Blade template is
        // excluded: its map belongs to the virtual PHP the Blade pipeline
        // publishes, not to a raw parse of the template.
        if self.is_blade_file(uri) {
            return None;
        }
        self.update_ast(uri, &content);
        Some((content, self.symbol_map_for(uri)?))
    }

    fn translate_item_to_blade(&self, item: &mut CallHierarchyItem) {
        let uri = item.uri.to_string();
        if !self.is_blade_file(&uri) {
            return;
        }
        item.range = self.blade_range(&uri, item.range);
        item.selection_range = self.blade_range(&uri, item.selection_range);
    }

    fn translate_ranges_to_blade(&self, uri: &str, ranges: &mut [Range]) {
        if !self.is_blade_file(uri) {
            return;
        }
        for range in ranges {
            *range = self.blade_range(uri, *range);
        }
    }

    fn blade_range(&self, uri: &str, range: Range) -> Range {
        Range::new(
            self.translate_php_to_blade(uri, range.start),
            self.translate_php_to_blade(uri, range.end),
        )
    }
}

/// Whether a symbol occurrence is a call the hierarchy should report.
///
/// Reading a same-named property (`$this->leaf`), naming the method in a
/// `@see` tag, and calling it (`$this->leaf()`) all reference the same member,
/// so both directions filter down to the occurrences that actually invoke it.
fn is_call_site(span: &SymbolSpan) -> bool {
    match &span.kind {
        SymbolKind::FunctionCall {
            is_definition,
            is_docblock_reference,
            ..
        } => !is_definition && !is_docblock_reference,
        SymbolKind::MemberAccess {
            is_method_call,
            docblock_ref,
            ..
        } => *is_method_call && !docblock_ref.is_reference(),
        SymbolKind::ClassReference { context, .. } => matches!(context, ClassRefContext::New),
        _ => false,
    }
}

fn method_callable_at(
    uri: &str,
    content: &str,
    symbol_map: &SymbolMap,
    class: &ClassInfo,
    offset: u32,
) -> Option<PhpCallable> {
    for method in class.methods.iter() {
        // A member merged in from a parent or a trait carries the declaring
        // file's offsets, which say nothing about this class' own body.
        if method.is_virtual
            || method.name_offset <= class.start_offset
            || method.name_offset >= class.end_offset
        {
            continue;
        }
        let upper = class
            .methods
            .iter()
            .map(|other| other.name_offset)
            .filter(|&other| other > method.name_offset && other < class.end_offset)
            .min()
            .unwrap_or(class.end_offset);
        let body = declaration_body(symbol_map, method.name_offset, upper);
        let name_end = method.name_offset.saturating_add(method.name.len() as u32);
        let contains = (method.name_offset..=name_end).contains(&offset)
            || body.is_some_and(|(start, end)| start <= offset && offset <= end);
        if contains {
            return build_method_callable(uri, content, class, method, body);
        }
    }
    None
}

fn function_callable_at(
    uri: &str,
    content: &str,
    symbol_map: &SymbolMap,
    fqn: &str,
    function: &FunctionInfo,
    offset: u32,
) -> Option<PhpCallable> {
    if function.name_offset == 0 {
        return None;
    }
    let body = declaration_body(symbol_map, function.name_offset, content.len() as u32);
    let name_end = function
        .name_offset
        .saturating_add(function.name.len() as u32);
    if !(function.name_offset..=name_end).contains(&offset)
        && !body.is_some_and(|(start, end)| start <= offset && offset <= end)
    {
        return None;
    }
    build_function_callable(uri, content, fqn, function, body)
}

/// The body of the declaration whose name token sits at `name_offset`.
///
/// The first scope to open after the name and before `upper` (the next
/// declaration, or the end of the enclosing class) is the body.  A declaration
/// without one — an interface method, an `abstract` method — has no scope in
/// that window and reports `None`.
fn declaration_body(symbol_map: &SymbolMap, name_offset: u32, upper: u32) -> Option<(u32, u32)> {
    symbol_map
        .scopes
        .iter()
        .copied()
        .filter(|(start, _)| *start > name_offset && *start < upper)
        .min_by_key(|(start, _)| *start)
}

fn build_method_callable(
    uri: &str,
    content: &str,
    class: &ClassInfo,
    method: &MethodInfo,
    body: Option<(u32, u32)>,
) -> Option<PhpCallable> {
    let uri = Url::parse(uri).ok()?;
    let selection_range = offset_range(content, method.name_offset, method.name.len() as u32);
    let range = Range::new(
        selection_range.start,
        body.map_or(selection_range.end, |(_, end)| {
            offset_to_position(content, end as usize)
        }),
    );
    let class_fqn = class.fqn().to_string();
    Some(PhpCallable {
        item: CallHierarchyItem {
            name: method.name.to_string(),
            kind: LspSymbolKind::METHOD,
            tags: None,
            detail: Some(class_fqn.clone()),
            uri,
            range,
            selection_range,
            data: Some(serde_json::json!({
                "kind": "php",
                "owner": class_fqn,
                "method": method.name.as_str(),
                "offset": method.name_offset,
            })),
        },
        body,
    })
}

fn build_function_callable(
    uri: &str,
    content: &str,
    fqn: &str,
    function: &FunctionInfo,
    body: Option<(u32, u32)>,
) -> Option<PhpCallable> {
    let uri = Url::parse(uri).ok()?;
    let selection_range = offset_range(content, function.name_offset, function.name.len() as u32);
    let range = Range::new(
        selection_range.start,
        body.map_or(selection_range.end, |(_, end)| {
            offset_to_position(content, end as usize)
        }),
    );
    Some(PhpCallable {
        item: CallHierarchyItem {
            name: function.name.to_string(),
            kind: LspSymbolKind::FUNCTION,
            tags: None,
            detail: function.namespace.clone(),
            uri,
            range,
            selection_range,
            data: Some(serde_json::json!({
                "kind": "php",
                "function": fqn,
                "offset": function.name_offset,
            })),
        },
        body,
    })
}

fn offset_range(content: &str, start: u32, len: u32) -> Range {
    Range::new(
        offset_to_position(content, start as usize),
        offset_to_position(content, start.saturating_add(len) as usize),
    )
}

fn php_item_key(item: &CallHierarchyItem) -> String {
    format!(
        "{}:{}:{}:{}",
        item.uri, item.selection_range.start.line, item.selection_range.start.character, item.name
    )
}

fn push_unique_range(ranges: &mut Vec<Range>, range: Range) {
    if !ranges.contains(&range) {
        ranges.push(range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "file:///call_hierarchy.php";

    fn parse(content: &str) -> Backend {
        let backend = Backend::new_test();
        backend
            .open_files
            .write()
            .insert(URI.to_string(), std::sync::Arc::new(content.to_string()));
        backend.update_ast(URI, content);
        backend
    }

    /// Prepare the hierarchy one character into the first line containing
    /// `needle`, which for a `function <name>` needle lands on the keyword and
    /// exercises the declaration-name path.
    fn prepare_at(backend: &Backend, content: &str, needle: &str) -> CallHierarchyItem {
        let line = content
            .lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line containing {needle:?}"));
        let character = content.lines().nth(line).unwrap().find(needle).unwrap()
            + needle.rfind(' ').map_or(0, |space| space + 2);
        backend
            .prepare_call_hierarchy_impl(URI, content, Position::new(line as u32, character as u32))
            .unwrap_or_else(|| panic!("no callable at {needle:?}"))
            .remove(0)
    }

    fn outgoing_names(backend: &Backend, item: &CallHierarchyItem) -> Vec<String> {
        let mut names: Vec<String> = backend
            .outgoing_calls_impl(item)
            .unwrap()
            .into_iter()
            .map(|call| call.to.name)
            .collect();
        names.sort();
        names
    }

    #[test]
    fn prepares_methods_and_resolves_outgoing_calls() {
        let content = r#"<?php
class Worker {
    public function leaf(): void {}
    public function run(): void { $this->leaf(); }
}
"#;
        let backend = parse(content);
        let run = prepare_at(&backend, content, "function run");
        let outgoing = backend.outgoing_calls_impl(&run).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.name, "leaf");
        assert_eq!(outgoing[0].from_ranges.len(), 1);
    }

    #[test]
    fn resolves_incoming_calls_through_find_references() {
        let content = r#"<?php
class Worker {
    public function leaf(): void {}
    public function run(): void { $this->leaf(); }
}
"#;
        let backend = parse(content);
        let leaf = prepare_at(&backend, content, "function leaf");
        let incoming = backend.incoming_calls_impl(&leaf).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "run");
    }

    #[test]
    fn ignores_occurrences_that_are_not_calls() {
        let content = r#"<?php
class Worker {
    public string $leaf = 'x';
    public function leaf(): void {}
    public function run(): void {
        $this->leaf();
        /** @see Worker::leaf() */
        $read = $this->leaf;
    }
}
"#;
        let backend = parse(content);
        let leaf = prepare_at(&backend, content, "function leaf");
        let incoming = backend.incoming_calls_impl(&leaf).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "run");
        // Only the `$this->leaf()` call, not the property read or the `@see`
        // tag that Find References also reports for the member.
        assert_eq!(incoming[0].from_ranges.len(), 1);
        assert_eq!(incoming[0].from_ranges[0].start.line, 5);

        let run = prepare_at(&backend, content, "function run");
        let outgoing = backend.outgoing_calls_impl(&run).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].from_ranges.len(), 1);
    }

    #[test]
    fn instantiation_reports_the_constructor() {
        let content = r#"<?php
class Dep {
    public function __construct(int $x) {}
}
class Worker {
    public function run(): void { $dep = new Dep(1); }
}
"#;
        let backend = parse(content);
        let run = prepare_at(&backend, content, "function run");
        let outgoing = backend.outgoing_calls_impl(&run).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.name, "__construct");
        assert_eq!(outgoing[0].to.detail.as_deref(), Some("Dep"));

        let constructor = prepare_at(&backend, content, "function __construct");
        let incoming = backend.incoming_calls_impl(&constructor).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "run");
    }

    #[test]
    fn instantiation_walks_to_an_inherited_constructor() {
        let content = r#"<?php
class Base {
    public function __construct() {}
}
class Dep extends Base {
    public function ping(): void {}
}
class Worker {
    public function run(): void { $dep = new Dep(); }
}
"#;
        let backend = parse(content);
        let run = prepare_at(&backend, content, "function run");
        let outgoing = backend.outgoing_calls_impl(&run).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.name, "__construct");
        assert_eq!(outgoing[0].to.detail.as_deref(), Some("Base"));
    }

    #[test]
    fn global_functions_participate_in_both_directions() {
        let content = r#"<?php
function helper(): void {}
function caller(): void { helper(); }
"#;
        let backend = parse(content);
        let caller = prepare_at(&backend, content, "function caller");
        assert_eq!(caller.kind, LspSymbolKind::FUNCTION);
        assert_eq!(
            outgoing_names(&backend, &caller),
            vec!["helper".to_string()]
        );

        let helper = prepare_at(&backend, content, "function helper");
        let incoming = backend.incoming_calls_impl(&helper).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "caller");
    }

    #[test]
    fn a_bodyless_method_has_no_outgoing_calls_but_keeps_its_callers() {
        let content = r#"<?php
abstract class Partial {
    abstract public function todo(): void;
    public function driver(): void { $this->todo(); }
}
"#;
        let backend = parse(content);
        let todo = prepare_at(&backend, content, "function todo");
        assert!(backend.outgoing_calls_impl(&todo).unwrap().is_empty());
        let incoming = backend.incoming_calls_impl(&todo).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "driver");
    }

    #[test]
    fn a_method_of_a_nested_anonymous_class_wins_over_its_host() {
        let content = r#"<?php
class Outer {
    public function make() {
        return new class {
            public function inner(): void { $this->helper(); }
            public function helper(): void {}
        };
    }
}
"#;
        let backend = parse(content);
        let inner = prepare_at(&backend, content, "function inner");
        assert_eq!(inner.name, "inner");
        assert_eq!(outgoing_names(&backend, &inner), vec!["helper".to_string()]);
    }

    #[test]
    fn static_and_trait_calls_resolve() {
        let content = r#"<?php
trait Greets {
    public function greet(): void {}
}
class Worker {
    use Greets;
    public static function stat(): void {}
    public function run(): void {
        self::stat();
        $this->greet();
    }
}
"#;
        let backend = parse(content);
        let run = prepare_at(&backend, content, "function run");
        assert_eq!(
            outgoing_names(&backend, &run),
            vec!["greet".to_string(), "stat".to_string()]
        );

        let greet = prepare_at(&backend, content, "function greet");
        assert_eq!(greet.detail.as_deref(), Some("Greets"));
        let incoming = backend.incoming_calls_impl(&greet).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "run");
    }
    #[test]
    fn a_parent_constructor_call_is_reported_in_both_directions() {
        let content = r#"<?php
class Base {
    public function __construct() {}
    public function shared(): void {}
}
class Child extends Base {
    public function __construct() { parent::__construct(); }
    public function go(): void {
        $this->shared();
        $fcc = $this->shared(...);
    }
}
"#;
        let backend = parse(content);
        let child_constructor =
            prepare_at(&backend, content, "public function __construct() { parent");
        assert_eq!(child_constructor.detail.as_deref(), Some("Child"));
        assert_eq!(
            outgoing_names(&backend, &child_constructor),
            vec!["__construct".to_string()]
        );

        let base_constructor = prepare_at(&backend, content, "public function __construct() {}");
        let incoming = backend.incoming_calls_impl(&base_constructor).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.detail.as_deref(), Some("Child"));

        // A first-class callable (`$this->shared(...)`) names the method the
        // same way the call above it does.
        let shared = prepare_at(&backend, content, "function shared");
        let incoming = backend.incoming_calls_impl(&shared).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "go");
        assert_eq!(incoming[0].from_ranges.len(), 2);
    }
}
