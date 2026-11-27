//! The `$matches` shape a parsed pattern leaves behind on a successful match.
//!
//! The rules follow PHPStan's `RegexArrayShapeMatcher`, which is what users
//! compare against:
//!
//! - Key `0` is the whole match, then each capture group by number, a named
//!   group's name just before its number.
//! - Only a run of groups at the end that may not participate can be missing;
//!   an earlier group that did not participate is reported as `''` (or `null`
//!   under `PREG_UNMATCHED_AS_NULL`).
//! - A group that only ever matches fixed text is typed with those literals.
//! - A pattern whose groups all sit in the branches of one top-level
//!   alternation gives one shape per branch, since a match goes through
//!   exactly one of them; a pattern with a single optional top-level group
//!   gives one shape with it and one without.

use std::collections::BTreeMap;

use super::parse::{GroupKind, Node, Pattern};
use super::{PREG_OFFSET_CAPTURE, PREG_SET_ORDER, PREG_UNMATCHED_AS_NULL};
use crate::php_type::{PhpType, ShapeEntry, TypeKind};

/// How many literal values a group may have before it is typed `string`.
const MAX_LITERALS: usize = 64;

/// A group of the pattern: a capture group, or a non-capturing one whose
/// position decides whether the groups inside it participate.
struct GroupInfo {
    /// The capture group's number and name, `None` for a non-capturing group.
    capture: Option<(usize, Option<String>)>,
    parent: Option<usize>,
    /// The innermost alternation the group sits in, and which of its branches.
    alternation: Option<(usize, usize)>,
    /// The group itself carries a quantifier that allows zero repetitions.
    in_optional_quantification: bool,
    /// The group never participates in a match, or not in every one: it is
    /// inside a negative lookaround, or a branch reset numbers it in only
    /// some of its branches.
    always_optional: bool,
    /// The group's number is shared with groups in a branch reset's other
    /// branches, so its value is not one fixed text.
    in_branch_reset: bool,
    /// What a capture group holds when it participates.
    value: PhpType,
}

/// The groups of a parsed pattern.
pub(super) struct Analysis {
    groups: Vec<GroupInfo>,
    /// Indices into `groups` of the capture groups, in number order.
    captures: Vec<usize>,
    /// The number of branches of each alternation.
    alternation_branches: Vec<usize>,
    marks: Vec<String>,
}

#[derive(Clone, Copy)]
struct Context {
    alternation: Option<(usize, usize)>,
    in_optional_quantification: bool,
    parent: Option<usize>,
    repeated: bool,
    in_branch_reset: bool,
}

impl Analysis {
    /// Collect the groups of `pattern`. `None` when two groups share a name
    /// without the pattern allowing it, which PCRE rejects.
    pub(super) fn of(pattern: &Pattern) -> Option<Analysis> {
        let mut analysis = Analysis {
            groups: Vec::new(),
            captures: Vec::new(),
            alternation_branches: Vec::new(),
            marks: Vec::new(),
        };
        let mut next_number = 1;
        analysis.walk(
            &pattern.root,
            Context {
                alternation: None,
                in_optional_quantification: false,
                parent: None,
                repeated: false,
                in_branch_reset: false,
            },
            &mut next_number,
        );
        if !pattern.duplicate_names {
            let mut names: Vec<&str> = analysis
                .captures
                .iter()
                .filter_map(|&g| analysis.groups[g].capture.as_ref()?.1.as_deref())
                .collect();
            let count = names.len();
            names.sort_unstable();
            names.dedup();
            if names.len() != count {
                return None;
            }
        }
        Some(analysis)
    }

    fn walk(&mut self, node: &Node, ctx: Context, next_number: &mut usize) {
        let plain = Context {
            in_optional_quantification: false,
            ..ctx
        };
        match node {
            Node::Empty | Node::Char(_) | Node::Class(_) | Node::Other | Node::ResetMatchStart => {}
            Node::Mark(name) => {
                if !self.marks.contains(name) {
                    self.marks.push(name.clone());
                }
            }
            Node::Concat(items) => {
                for item in items {
                    self.walk(item, plain, next_number);
                }
            }
            Node::Alternation(branches) => {
                let id = self.alternation_branches.len();
                self.alternation_branches.push(branches.len());
                for (branch, node) in branches.iter().enumerate() {
                    let ctx = Context {
                        alternation: Some((id, branch)),
                        ..plain
                    };
                    self.walk(node, ctx, next_number);
                }
            }
            Node::Repeat { node, min, max } => {
                let ctx = Context {
                    in_optional_quantification: *min == 0,
                    repeated: ctx.repeated || max.is_none_or(|max| max > 1),
                    ..ctx
                };
                self.walk(node, ctx, next_number);
            }
            Node::Group(group) => {
                let capture = match &group.kind {
                    GroupKind::Capture { name } => {
                        let number = *next_number;
                        *next_number += 1;
                        Some((number, name.clone()))
                    }
                    _ => None,
                };
                let value = if capture.is_some() {
                    group_value(&group.body, !ctx.repeated && !ctx.in_branch_reset)
                } else {
                    PhpType::string()
                };
                let index = self.groups.len();
                self.groups.push(GroupInfo {
                    capture,
                    parent: ctx.parent,
                    alternation: ctx.alternation,
                    in_optional_quantification: ctx.in_optional_quantification,
                    always_optional: matches!(group.kind, GroupKind::NegativeLookaround),
                    in_branch_reset: ctx.in_branch_reset,
                    value,
                });
                if self.groups[index].capture.is_some() {
                    self.captures.push(index);
                }
                let inner = Context {
                    parent: Some(index),
                    in_optional_quantification: false,
                    ..ctx
                };
                match (&group.kind, &group.body) {
                    (GroupKind::BranchReset, Node::Alternation(branches)) => {
                        self.walk_branch_reset(branches, inner, next_number);
                    }
                    _ => self.walk(&group.body, inner, next_number),
                }
            }
        }
    }

    /// The branches of `(?|…)`, each numbering its groups from the same
    /// starting point. A number keeps the first group that has it, and is
    /// optional unless every branch has a group with it.
    fn walk_branch_reset(&mut self, branches: &[Node], ctx: Context, next_number: &mut usize) {
        let first_number = *next_number;
        let first_capture = self.captures.len();
        let mut end = first_number;
        let mut present: BTreeMap<usize, usize> = BTreeMap::new();
        for branch in branches {
            *next_number = first_number;
            let before = self.captures.len();
            let ctx = Context {
                in_branch_reset: true,
                ..ctx
            };
            self.walk(branch, ctx, next_number);
            for &g in &self.captures[before..] {
                if let Some((number, _)) = self.groups[g].capture {
                    *present.entry(number).or_default() += 1;
                }
            }
            end = end.max(*next_number);
        }
        *next_number = end;

        let mut kept: Vec<usize> = Vec::new();
        for g in self.captures.split_off(first_capture) {
            let number = self.groups[g].capture.as_ref().map(|c| c.0);
            if kept
                .iter()
                .any(|&k| self.groups[k].capture.as_ref().map(|c| c.0) == number)
            {
                continue;
            }
            if number.is_some_and(|n| present.get(&n).copied().unwrap_or(0) < branches.len()) {
                self.groups[g].always_optional = true;
            }
            kept.push(g);
        }
        kept.sort_by_key(|&g| self.groups[g].capture.as_ref().map(|c| c.0));
        self.captures.extend(kept);
    }

    fn is_capture(&self, g: usize) -> bool {
        self.groups[g].capture.is_some()
    }

    /// Whether group `g` may not participate in a match that succeeds, with
    /// the groups in `forced` known to participate.
    fn is_optional(&self, g: usize, forced: &[usize]) -> bool {
        if forced.contains(&g) {
            return false;
        }
        let group = &self.groups[g];
        if group.alternation.is_some() || group.in_optional_quantification || group.always_optional
        {
            return true;
        }
        // A capture group known to participate takes its nested groups along.
        let mut parent = group.parent;
        while let Some(p) = parent {
            if self.is_capture(p) && forced.contains(&p) {
                return false;
            }
            parent = self.groups[p].parent;
        }
        group.parent.is_some_and(|p| self.is_optional(p, forced))
    }

    fn is_top_level(&self, g: usize) -> bool {
        match self.groups[g].parent {
            None => true,
            Some(p) => !self.is_capture(p) && self.is_top_level(p),
        }
    }

    /// The one top-level capture group, when it is optional.
    fn only_optional_top_level_group(&self) -> Option<usize> {
        let mut found = None;
        for &g in &self.captures {
            if !self.is_top_level(g) {
                continue;
            }
            if !self.is_optional(g, &[]) || found.is_some() {
                return None;
            }
            found = Some(g);
        }
        found
    }

    /// The alternation every top-level capture group is a branch of.
    fn only_top_level_alternation(&self) -> Option<usize> {
        let mut alternation = None;
        for &g in &self.captures {
            if !self.is_top_level(g) {
                continue;
            }
            let group = &self.groups[g];
            let (id, _) = group.alternation?;
            if group.in_optional_quantification || alternation.is_some_and(|a| a != id) {
                return None;
            }
            alternation = Some(id);
        }
        alternation
    }

    /// Whether `g` is a branch of an alternation that sits in an optional
    /// group, so the alternation as a whole may be skipped.
    fn in_optional_alternation(&self, g: usize, forced: &[usize]) -> bool {
        let Some((id, _)) = self.groups[g].alternation else {
            return false;
        };
        let mut parent = self.groups[g].parent;
        while let Some(p) = parent {
            if self.groups[p].alternation.map(|a| a.0) != Some(id) {
                break;
            }
            if self.is_capture(p) {
                return false;
            }
            parent = self.groups[p].parent;
        }
        parent.is_some_and(|p| self.is_optional(p, forced))
    }

    /// The shapes a successful match may leave, before PHPStan's union
    /// folding: one per way through the pattern that the tagged unions tell
    /// apart.
    pub(super) fn matched_shapes(&self, flags: i64, matches_all: bool) -> Vec<Vec<ShapeEntry>> {
        let unmatched_as_null = flags & PREG_UNMATCHED_AS_NULL != 0;
        let trailing_optionals = self
            .captures
            .iter()
            .rev()
            .take_while(|&&g| self.is_optional(g, &[]))
            .count();
        let build = |list: &[usize], forced: &[usize], overrides: &[(usize, PhpType)]| {
            self.build(list, forced, overrides, trailing_optionals, flags, matches_all)
        };
        // A match that took none of the optional parts: the whole match only.
        let subject_only = || {
            vec![ShapeEntry {
                key: Some("0".to_string()),
                value_type: subject_value_type(flags, false),
                optional: false,
            }]
        };

        if matches_all {
            return vec![build(&self.captures, &[], &[])];
        }

        if let Some(g) = self.only_optional_top_level_group() {
            let with_group = build(&self.captures, &[g], &[]);
            if unmatched_as_null {
                return vec![with_group];
            }
            return vec![subject_only(), with_group];
        }

        if let Some(alternation) = self.only_top_level_alternation() {
            let mut combinations: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for &g in &self.captures {
                if let Some((id, branch)) = self.groups[g].alternation
                    && id == alternation
                {
                    combinations.entry(branch).or_default().push(g);
                }
            }
            let unmatched = if unmatched_as_null {
                PhpType::null()
            } else {
                PhpType::literal_string_value("")
            };
            let mut shapes = Vec::new();
            let mut optional_alternation = false;
            for combination in combinations.values() {
                let mut forced: Vec<usize> = Vec::new();
                let mut overrides: Vec<(usize, PhpType)> = Vec::new();
                let mut list: Vec<usize> = Vec::new();
                let mut before_combination = true;
                for &g in &self.captures {
                    if combination.contains(&g) {
                        optional_alternation = self.in_optional_alternation(g, &forced);
                        forced.push(g);
                        before_combination = false;
                    } else if before_combination && !self.groups[g].in_branch_reset {
                        forced.push(g);
                        overrides.push((g, unmatched.clone()));
                    } else if self.groups[g].alternation.map(|a| a.0) == Some(alternation)
                        && !unmatched_as_null
                    {
                        continue;
                    }
                    list.push(g);
                }
                shapes.push(build(&list, &forced, &overrides));
            }
            if !unmatched_as_null
                && (self.alternation_branches[alternation] != combinations.len()
                    || optional_alternation)
            {
                shapes.push(subject_only());
            }
            return shapes;
        }

        vec![build(&self.captures, &[], &[])]
    }

    fn build(
        &self,
        list: &[usize],
        forced: &[usize],
        overrides: &[(usize, PhpType)],
        trailing_optionals: usize,
        flags: i64,
        matches_all: bool,
    ) -> Vec<ShapeEntry> {
        let unmatched_as_null = flags & PREG_UNMATCHED_AS_NULL != 0;
        let set_order = flags & PREG_SET_ORDER != 0;
        let mut entries: Vec<ShapeEntry> = Vec::with_capacity(list.len() + 2);
        set_entry(
            &mut entries,
            "0".to_string(),
            subject_value_type(flags, matches_all),
            false,
            false,
        );
        let count = list.len();
        for (i, &g) in list.iter().enumerate() {
            let is_trailing = i + trailing_optionals >= count;
            let is_last = i + 1 == count;
            let optional = self.is_optional(g, forced);
            let base = overrides
                .iter()
                .find(|(o, _)| *o == g)
                .map_or_else(|| self.groups[g].value.clone(), |(_, t)| t.clone());
            let value = if matches_all {
                match_all_group_value(base, flags, optional, is_trailing)
            } else {
                let base = if !is_last && !unmatched_as_null && optional {
                    or_unmatched(base)
                } else {
                    base
                };
                let value = with_flags(base.clone(), flags);
                if !is_trailing && unmatched_as_null && !optional && flags & PREG_OFFSET_CAPTURE == 0 {
                    base
                } else {
                    value
                }
            };
            let key_optional = if matches_all {
                is_trailing && !unmatched_as_null && set_order
            } else {
                is_trailing && !unmatched_as_null && optional
            };
            if let Some((_, Some(name))) = &self.groups[g].capture {
                // Across every match, a shared name holds either group's text.
                set_entry(
                    &mut entries,
                    name.clone(),
                    value.clone(),
                    key_optional,
                    matches_all,
                );
            }
            set_entry(&mut entries, (i + 1).to_string(), value, key_optional, false);
        }
        if !self.marks.is_empty() {
            let marks = self
                .marks
                .iter()
                .map(PhpType::literal_string_value)
                .collect();
            set_entry(
                &mut entries,
                "MARK".to_string(),
                PhpType::union(marks),
                true,
                false,
            );
        }
        entries
    }
}

/// The shape type of `entries`: a list of them per match for
/// `preg_match_all` in set order.
fn finish(entries: Vec<ShapeEntry>, flags: i64, matches_all: bool) -> PhpType {
    let is_list = entries
        .iter()
        .all(|e| e.key.as_deref().is_some_and(|k| k.parse::<usize>().is_ok()));
    if matches_all && flags & PREG_SET_ORDER != 0 {
        PhpType::list(PhpType::array_shape(entries))
    } else if is_list && entries.iter().any(|e| e.optional) {
        // Without optional keys the shape is a list already; with them, the
        // keys a match leaves out are still the trailing ones.
        PhpType::list_shape(entries)
    } else {
        PhpType::array_shape(entries)
    }
}

/// The union of the shapes, with a shape folded into another that has the
/// same keys plus at most one more required one, as PHPStan's union does:
/// `array{0: string}|array{0: string, 1: string}` reads as
/// `array{0: string, 1?: string}`.
pub(super) fn finish_union(
    mut shapes: Vec<Vec<ShapeEntry>>,
    flags: i64,
    matches_all: bool,
) -> PhpType {
    if matches_all {
        return PhpType::union(
            shapes
                .into_iter()
                .map(|entries| finish(entries, flags, true))
                .collect(),
        );
    }
    let mut i = 0;
    'outer: while i < shapes.len() {
        let mut j = i + 1;
        while j < shapes.len() {
            if keys_superset_of(&shapes[j], &shapes[i]) {
                let small = shapes.remove(i);
                merge_into(&mut shapes[j - 1], &small, true);
                continue 'outer;
            }
            if keys_superset_of(&shapes[i], &shapes[j]) {
                let small = shapes.remove(j);
                merge_into(&mut shapes[i], &small, false);
                continue;
            }
            j += 1;
        }
        i += 1;
    }
    PhpType::union(
        shapes
            .into_iter()
            .map(|entries| finish(entries, flags, false))
            .collect(),
    )
}

/// Whether `big` has every key of `small` with a value that may agree, and
/// at most one further required key (PHPStan's `isKeysSupersetOf`).
fn keys_superset_of(big: &[ShapeEntry], small: &[ShapeEntry]) -> bool {
    if big.len() < small.len() || small.is_empty() {
        return false;
    }
    let mut fail_on_different_value = big.len() != small.len() || big.len() < 2;
    for entry in small {
        let Some(other) = big.iter().find(|e| e.key == entry.key) else {
            return false;
        };
        if !values_may_agree(&other.value_type, &entry.value_type) {
            if fail_on_different_value {
                return false;
            }
            fail_on_different_value = true;
        }
    }
    big.iter()
        .filter(|e| !e.optional && !small.iter().any(|s| s.key == e.key))
        .count()
        <= 1
}

/// Whether two entry values can hold a common value: they share a member.
///
/// A plain `string` stands for the refined string PHPStan would have there
/// (`numeric-string` for `\d+`), which a literal may or may not fit, so it
/// is not taken to agree with one; keeping two shapes apart is never wrong.
fn values_may_agree(a: &PhpType, b: &PhpType) -> bool {
    let members_agree = |x: &PhpType, y: &PhpType| {
        x == y
            || match (shape_entries(x), shape_entries(y)) {
                (Some(xs), Some(ys)) => {
                    xs.len() == ys.len()
                        && xs.iter().zip(ys).all(|(x, y)| {
                            x.key == y.key && values_may_agree(&x.value_type, &y.value_type)
                        })
                }
                _ => false,
            }
    };
    a.union_members()
        .into_iter()
        .any(|x| b.union_members().into_iter().any(|y| members_agree(x, y)))
}

/// The element type of `list<T>`.
fn list_element(ty: &PhpType) -> Option<&PhpType> {
    match ty.kind() {
        TypeKind::Generic(generic) if generic.name == "list" && generic.args.len() == 1 => {
            generic.args.first()
        }
        _ => None,
    }
}

fn shape_entries(ty: &PhpType) -> Option<&[ShapeEntry]> {
    match ty.kind() {
        TypeKind::ArrayShape(entries) => Some(entries),
        _ => None,
    }
}

/// Two entry values as one: shapes with the same keys entry by entry
/// (an offset pair stays one pair), anything else as a union.
fn merge_values(a: &PhpType, b: &PhpType) -> PhpType {
    if let (Some(xs), Some(ys)) = (shape_entries(a), shape_entries(b))
        && xs.len() == ys.len()
        && xs.iter().zip(ys).all(|(x, y)| x.key == y.key)
    {
        return PhpType::array_shape(
            xs.iter()
                .zip(ys)
                .map(|(x, y)| ShapeEntry {
                    key: x.key.clone(),
                    value_type: merge_values(&x.value_type, &y.value_type),
                    optional: x.optional || y.optional,
                })
                .collect(),
        );
    }
    if let (Some(x), Some(y)) = (list_element(a), list_element(b)) {
        return PhpType::list(merge_values(x, y));
    }
    join_strings(vec![a.clone(), b.clone()])
}

/// Fold `small` into `big`: shared keys take both values, listed in the
/// order the shapes came (`small_first`), and a key only `big` has becomes
/// optional.
fn merge_into(big: &mut [ShapeEntry], small: &[ShapeEntry], small_first: bool) {
    for entry in big.iter_mut() {
        match small.iter().find(|s| s.key == entry.key) {
            Some(other) => {
                entry.value_type = if small_first {
                    merge_values(&other.value_type, &entry.value_type)
                } else {
                    merge_values(&entry.value_type, &other.value_type)
                };
                entry.optional |= other.optional;
            }
            None => entry.optional = true,
        }
    }
}

/// The union of `members`, flattened, where a plain `string` covers every
/// string literal.
fn join_strings(members: Vec<PhpType>) -> PhpType {
    let string = PhpType::string();
    let mut flat: Vec<PhpType> = Vec::new();
    for member in &members {
        for member in member.union_members() {
            if !flat.contains(member) {
                flat.push(member.clone());
            }
        }
    }
    if flat.contains(&string) {
        flat.retain(|member| *member == string || !member.is_string_literal());
    }
    PhpType::union(flat)
}

/// `value`, or `''` for a group that did not participate.
fn or_unmatched(value: PhpType) -> PhpType {
    join_strings(vec![value, PhpType::literal_string_value("")])
}

/// Write `key` into the shape. A key already there (two groups sharing a
/// name) keeps its position and takes the new value, or both values when the
/// new one may be missing or `merge` says the key collects either group's.
fn set_entry(
    entries: &mut Vec<ShapeEntry>,
    key: String,
    value: PhpType,
    optional: bool,
    merge: bool,
) {
    match entries.iter_mut().find(|e| e.key.as_deref() == Some(key.as_str())) {
        Some(entry) if optional || merge => {
            entry.value_type = merge_values(&entry.value_type, &value);
        }
        Some(entry) => {
            entry.value_type = value;
            entry.optional = false;
        }
        None => entries.push(ShapeEntry {
            key: Some(key),
            value_type: value,
            optional,
        }),
    }
}

/// The whole match. Under `PREG_OFFSET_CAPTURE` with
/// `PREG_UNMATCHED_AS_NULL` the pair still admits `null`, as PHPStan types it.
fn subject_value_type(flags: i64, matches_all: bool) -> PhpType {
    let value = if flags & PREG_OFFSET_CAPTURE != 0 {
        with_flags(PhpType::string(), flags)
    } else {
        PhpType::string()
    };
    if matches_all && flags & PREG_SET_ORDER == 0 {
        PhpType::list(value)
    } else {
        value
    }
}

/// One `preg_match_all` group entry.
fn match_all_group_value(base: PhpType, flags: i64, optional: bool, is_trailing: bool) -> PhpType {
    let unmatched_as_null = flags & PREG_UNMATCHED_AS_NULL != 0;
    let set_order = flags & PREG_SET_ORDER != 0;
    let mut value = if !unmatched_as_null && optional && (!set_order || !is_trailing) {
        with_flags(or_unmatched(base), flags)
    } else if !is_trailing
        && unmatched_as_null
        && !optional
        && flags & PREG_OFFSET_CAPTURE == 0
    {
        base
    } else {
        with_flags(base, flags)
    };
    if !set_order {
        value = PhpType::list(value);
    }
    value
}

/// A matched value as the flags report it: `null` for an unmatched group,
/// paired with its offset.
fn with_flags(value: PhpType, flags: i64) -> PhpType {
    let value = if flags & PREG_UNMATCHED_AS_NULL != 0 {
        join_strings(vec![value, PhpType::null()])
    } else {
        value
    };
    if flags & PREG_OFFSET_CAPTURE != 0 {
        offset_pair(value)
    } else {
        value
    }
}

/// `[value, offset]`; an unmatched group reports `-1` as its offset.
fn offset_pair(value: PhpType) -> PhpType {
    PhpType::array_shape(vec![
        ShapeEntry {
            key: None,
            value_type: value,
            optional: false,
        },
        ShapeEntry {
            key: None,
            value_type: PhpType::int_range("-1", "max"),
            optional: false,
        },
    ])
}

/// What a capture group holds: its fixed texts when `allow_literals` and it
/// has only a few, `string` otherwise. A group made of an alternation is
/// typed branch by branch, so a branch with fixed text keeps it even when
/// another does not.
fn group_value(body: &Node, allow_literals: bool) -> PhpType {
    let branch_value = |node: &Node| match literals(node).filter(|_| allow_literals) {
        Some(texts) => PhpType::union(texts.iter().map(PhpType::literal_string_value).collect()),
        None => PhpType::string(),
    };
    match body {
        Node::Alternation(branches) => join_strings(branches.iter().map(branch_value).collect()),
        _ => branch_value(body),
    }
}

/// Every text `node` can match, when it is one of a few fixed ones.
///
/// A quantifier gives up even where the repeat count is fixed, as PHPStan
/// does, so the types agree.
fn literals(node: &Node) -> Option<Vec<String>> {
    Some(match node {
        Node::Empty | Node::ResetMatchStart | Node::Mark(_) => vec![String::new()],
        Node::Char(c) => vec![c.to_string()],
        Node::Class(Some(members)) => members.iter().map(char::to_string).collect(),
        Node::Class(None) | Node::Other | Node::Repeat { .. } => return None,
        Node::Group(group) => match group.kind {
            GroupKind::Lookaround | GroupKind::NegativeLookaround => vec![String::new()],
            _ => literals(&group.body)?,
        },
        Node::Concat(items) => {
            let mut texts = vec![String::new()];
            for item in items {
                let tails = literals(item)?;
                if texts.len() * tails.len() > MAX_LITERALS {
                    return None;
                }
                texts = texts
                    .iter()
                    .flat_map(|head| tails.iter().map(move |tail| format!("{head}{tail}")))
                    .collect();
            }
            texts
        }
        Node::Alternation(branches) => {
            let mut texts: Vec<String> = Vec::new();
            for branch in branches {
                for text in literals(branch)? {
                    if !texts.contains(&text) {
                        texts.push(text);
                    }
                }
                if texts.len() > MAX_LITERALS {
                    return None;
                }
            }
            texts
        }
    })
}
