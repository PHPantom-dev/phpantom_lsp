//! Ranking class name completions: how well a name matches the typed
//! prefix, and how close its namespace sits to the ones the file already
//! uses.

use std::collections::HashMap;

/// Build a table mapping namespace prefixes to their occurrence count.
///
/// The table is derived from the file's `use` imports and its own
/// namespace declaration.  Each FQN's namespace portion is exploded
/// into all ancestor prefixes, and the count records how many sources
/// (imports + file namespace) contribute to each prefix.
///
/// Used once per completion request to score candidates by namespace
/// affinity.
pub(crate) fn build_affinity_table(
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> HashMap<String, u32> {
    let mut table: HashMap<String, u32> = HashMap::new();

    let mut namespaces: Vec<&str> = Vec::new();

    if let Some(ns) = file_namespace {
        namespaces.push(ns.as_str());
    }

    for fqn in use_map.values() {
        if let Some(pos) = fqn.rfind('\\') {
            namespaces.push(&fqn[..pos]);
        }
    }

    for ns in namespaces {
        let parts: Vec<&str> = ns.split('\\').collect();
        for depth in 1..=parts.len() {
            let prefix = parts[..depth].join("\\");
            *table.entry(prefix).or_insert(0) += 1;
        }
    }

    table
}

/// Score a candidate FQN against the affinity table.
///
/// Extracts the candidate's namespace, explodes it into ancestor
/// prefixes, and sums the counts from the table for every matching
/// prefix.  Higher scores indicate the candidate lives in a namespace
/// that is heavily used by the current file.
pub(crate) fn affinity_score(fqn: &str, table: &HashMap<String, u32>) -> u32 {
    let ns = match fqn.rfind('\\') {
        Some(pos) => &fqn[..pos],
        None => return 0,
    };
    let parts: Vec<&str> = ns.split('\\').collect();
    let mut score = 0u32;
    for depth in 1..=parts.len() {
        let prefix = parts[..depth].join("\\");
        if let Some(&count) = table.get(&prefix) {
            score += count;
        }
    }
    score
}

/// Classify how well a short name matches the typed prefix.
///
/// Returns `'a'` for exact match, `'b'` for starts-with (or empty
/// prefix), `'c'` for substring-only.  The character is used as the
/// highest-weight dimension of the sort_text so exact matches always
/// appear before prefix matches, which always appear before substring
/// matches.
pub(in crate::completion) fn match_quality(short_name: &str, prefix: &str) -> char {
    if prefix.is_empty() {
        return 'b';
    }
    let sn = short_name.to_lowercase();
    let p = prefix.to_lowercase();
    if sn == p {
        'a'
    } else if sn.starts_with(&p) {
        'b'
    } else {
        'c'
    }
}

/// Assemble a sort_text string for a class name completion item.
///
/// The format is `{match_quality}{source_tier}{origin_tier}{affinity}{demote}{gap}_{short_name_lower}`
/// where:
/// - `match_quality`: `'a'` exact, `'b'` starts-with, `'c'` contains
/// - `source_tier`: `'0'` use-imported, `'1'` same-namespace, `'2'` everything else
/// - `origin_tier`: `'0'` project, `'1'` core/stub, `'2'` explicit vendor,
///   `'3'` transitive vendor
/// - `affinity`: 4-digit inverted score (`9999 - score`, so higher scores sort first)
/// - `demote`: `'0'` normal, `'1'` heuristically demoted
/// - `gap`: 3-digit distance between short name length and prefix length
///   (`short_name.len() - prefix.len()`).  Within the same affinity and
///   demotion group, names closer in length to what the user typed sort
///   first.  This smooths the visual transition as a prefix match
///   narrows toward an exact match (e.g. typing "Pro" ranks `Product`
///   above `ProductFilterTerm` when both share the same affinity).
///
/// **`source_tier` sorts before `origin_tier`** so that an already-imported
/// class always ranks above a non-imported one, regardless of where it
/// comes from.  A vendor class that's already in the file's `use` map
/// is more relevant than a project class the user hasn't imported yet.
pub(in crate::completion) fn class_sort_text(
    short_name: &str,
    fqn: &str,
    prefix: &str,
    source_tier: char,
    origin_tier: char,
    demoted: bool,
    affinity_table: &HashMap<String, u32>,
) -> String {
    let quality = match_quality(short_name, prefix);
    let score = affinity_score(fqn, affinity_table);
    let affinity = format!("{:04}", 9999_u32.saturating_sub(score.min(9999)));
    let gap = format!(
        "{:03}",
        short_name.len().saturating_sub(prefix.len()).min(999)
    );
    let demote = if demoted { '1' } else { '0' };
    format!(
        "{}{}{}{}{}{}_{}",
        quality,
        source_tier,
        origin_tier,
        affinity,
        demote,
        gap,
        short_name.to_lowercase()
    )
}
