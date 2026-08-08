//! Shared helpers for the Laravel virtual-member modules: case
//! conversion, parent-chain ancestry checks, accessor/mutator name
//! mapping, route group prefix extraction, English singularization
//! (mirroring Doctrine's inflector), and a lightweight PHP expression
//! walker.

use std::ops::ControlFlow;
use std::sync::Arc;

use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};

use super::ELOQUENT_MODEL_FQN;

/// Walk the parent chain of `class` checking whether any ancestor
/// (including the class itself) satisfies `predicate`.
///
/// This is the shared implementation behind the module tree's base-class
/// checks (`extends_eloquent_model`, `extends_eloquent_factory`, …).  The
/// predicate sees the class's own `name` first (which may be a short name;
/// callers that need it check `fqn()` separately), then each ancestor's
/// `parent_class` name, which post-processing has already resolved to an
/// FQN without a leading backslash.
pub(in crate::virtual_members::laravel) fn walks_parent_chain(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    predicate: fn(&str) -> bool,
) -> bool {
    if predicate(&class.name) {
        return true;
    }

    // Walk the parent chain without cloning ClassInfo.  We only need
    // each parent's `name` and `parent_class` fields, so keep a
    // cheap Arc handle instead of cloning the entire struct (which
    // copies hundreds of methods/properties/constants).
    let mut current_parent = class.parent_class;
    let mut depth = 0u32;
    while let Some(ref parent_name) = current_parent {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }
        if predicate(parent_name) {
            return true;
        }
        match class_loader(parent_name) {
            Some(parent) => {
                current_parent = parent.parent_class;
            }
            None => break,
        }
    }

    false
}

/// Determine whether `class_name` is the Eloquent Model base class.
pub(in crate::virtual_members::laravel) fn is_eloquent_model(class_name: &str) -> bool {
    class_name == ELOQUENT_MODEL_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Model`.
///
/// Returns `true` if the class itself is `Model` or any ancestor is.
pub fn extends_eloquent_model(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_model)
}

/// Determine whether `class_name` is the Eloquent Builder base class.
pub(in crate::virtual_members::laravel) fn is_eloquent_builder(class_name: &str) -> bool {
    class_name == super::ELOQUENT_BUILDER_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Builder`.
pub fn extends_eloquent_builder(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_builder)
}

/// Convert a camelCase or PascalCase string to snake_case.
///
/// Inserts an underscore before each uppercase letter that follows a
/// lowercase letter or digit, and before an uppercase letter that is
/// followed by a lowercase letter when preceded by another uppercase
/// letter, so acronyms stay whole (`URLName` → `url_name`).
///
/// Note this deliberately differs from Laravel's `Str::snake`, which
/// underscores every uppercase letter (`URLName` → `u_r_l_name`).  Both
/// spellings reach the same accessor at runtime because Eloquent maps
/// attribute names to methods via `Str::studly` + `method_exists`, and
/// PHP method lookup is case-insensitive — this form just produces the
/// friendlier property name.
///
/// `FullName` → `full_name`
/// `firstName` → `first_name`
/// `isAdmin` → `is_admin`
pub(crate) fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                // Insert underscore when: lowercase/digit → uppercase,
                // or uppercase → uppercase followed by lowercase (acronym boundary).
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    result.push('_');
                } else if prev.is_uppercase() {
                    // Acronym boundary: the last capital of a run starts
                    // a new word when followed by lowercase ("URLName" →
                    // "url_name").
                    if let Some(&next) = chars.get(i + 1)
                        && next.is_lowercase()
                    {
                        result.push('_');
                    }
                }
            }
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to camelCase.
///
/// `full_name` → `fullName`
/// `avatar_url` → `avatarUrl`
/// `name` → `name`
pub(crate) fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to PascalCase.
///
/// `full_name` → `FullName`
/// `avatar_url` → `AvatarUrl`
/// `name` → `Name`
pub(in crate::virtual_members::laravel) fn snake_to_pascal(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Build the legacy accessor method name from a virtual property name.
///
/// `display_name` → `getDisplayNameAttribute`
/// `name` → `getNameAttribute`
pub(crate) fn legacy_accessor_method_name(property_name: &str) -> String {
    let pascal = snake_to_pascal(property_name);
    format!("get{pascal}Attribute")
}

pub(crate) fn legacy_mutator_method_name(property_name: &str) -> String {
    let pascal = snake_to_pascal(property_name);
    format!("set{pascal}Attribute")
}

/// Return candidate accessor method names for a virtual property name.
///
/// Go-to-definition uses this to map a snake_case virtual property back
/// to the method that produces it.  Returns the legacy accessor
/// (`getDisplayNameAttribute`), legacy mutator (`setDisplayNameAttribute`),
/// and modern (`displayName`) forms so the caller can try each one.
pub(crate) fn accessor_method_candidates(property_name: &str) -> Vec<String> {
    vec![
        legacy_accessor_method_name(property_name),
        legacy_mutator_method_name(property_name),
        snake_to_camel(property_name),
    ]
}

/// Extract a string-keyed option (e.g. `'as' => 'admin.'`) from a
/// `Route::group([…], fn(){})` argument list.
///
/// The array may be in any position; all non-array arguments are skipped.
fn extract_group_option_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
    option: &str,
) -> String {
    for arg in args {
        let elements: Vec<&ArrayElement<'_>> = match arg {
            Expression::Array(arr) => arr.elements.iter().collect(),
            Expression::LegacyArray(arr) => arr.elements.iter().collect(),
            _ => continue,
        };
        for element in elements {
            let ArrayElement::KeyValue(kv) = element else {
                continue;
            };
            let Some((key, _, _)) = extract_string_literal(kv.key, content) else {
                continue;
            };
            if key == option
                && let Some((val, _, _)) = extract_string_literal(kv.value, content)
            {
                return val.to_string();
            }
        }
    }
    String::new()
}

/// Extract the `'as' => 'prefix.'` name prefix from a `Route::group([…], fn(){})` argument list.
pub(crate) fn extract_as_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
) -> String {
    extract_group_option_from_args(args, content, "as")
}

/// Extract the `'prefix' => 'admin'` URI prefix from a `Route::group([…], fn(){})`
/// argument list.
pub(crate) fn extract_uri_prefix_from_args<'a>(
    args: impl Iterator<Item = &'a Expression<'a>>,
    content: &str,
) -> String {
    extract_group_option_from_args(args, content, "prefix")
}

/// Join two URI segments the way Laravel's route prefixing does: both sides
/// lose their surrounding slashes and are joined with a single `/`.
///
/// An empty side is dropped, so `join_uri_segments("admin", "")` is `"admin"`.
pub(crate) fn join_uri_segments(left: &str, right: &str) -> String {
    let left = left.trim_matches('/');
    let right = right.trim_matches('/');
    if left.is_empty() {
        right.to_string()
    } else if right.is_empty() {
        left.to_string()
    } else {
        format!("{left}/{right}")
    }
}
/// Words Doctrine's inflector leaves alone when singularizing.
///
/// Checked before anything else, so an already-singular word ending in `s`
/// (`status`, `campus`) and a word that is its own plural (`series`,
/// `sheep`) both survive the trailing-`s` rule.
const UNINFLECTED_WORDS: &[&str] = &[
    "advice",
    "aircraft",
    "amoyese",
    "art",
    "audio",
    "baggage",
    "bison",
    "borghese",
    "bream",
    "breeches",
    "britches",
    "buffalo",
    "butter",
    "cantus",
    "carp",
    "cattle",
    "chassis",
    "clippers",
    "clothes",
    "clothing",
    "coal",
    "cod",
    "coitus",
    "compensation",
    "congoese",
    "contretemps",
    "coreopsis",
    "corps",
    "cotton",
    "data",
    "debris",
    "deer",
    "diabetes",
    "djinn",
    "education",
    "eland",
    "elk",
    "emoji",
    "equipment",
    "evidence",
    "faroese",
    "fascia",
    "feedback",
    "fish",
    "flounder",
    "flour",
    "foochowese",
    "food",
    "fuchsia",
    "furniture",
    "galleria",
    "gallows",
    "genevese",
    "genoese",
    "gilbertese",
    "gold",
    "headquarters",
    "herpes",
    "hijinks",
    "homework",
    "hottentotese",
    "impatience",
    "information",
    "innings",
    "jackanapes",
    "jeans",
    "jedi",
    "kin",
    "kiplingese",
    "knowledge",
    "kongoese",
    "leather",
    "love",
    "lucchese",
    "luggage",
    "mackerel",
    "mafia",
    "maltese",
    "management",
    "metadata",
    "mews",
    "militia",
    "money",
    "moose",
    "mumps",
    "music",
    "nankingese",
    "news",
    "nexus",
    "niasese",
    "nutrition",
    "offspring",
    "oil",
    "pants",
    "patience",
    "pekingese",
    "petunia",
    "piedmontese",
    "pincers",
    "pistoiese",
    "plankton",
    "pliers",
    "pokemon",
    "police",
    "polish",
    "portuguese",
    "proceedings",
    "progress",
    "rabies",
    "rain",
    "research",
    "rhinoceros",
    "rice",
    "salmon",
    "sand",
    "sarawakese",
    "scissors",
    "sepia",
    "series",
    "shavese",
    "shears",
    "sheep",
    "siemens",
    "silk",
    "sms",
    "soap",
    "spam",
    "species",
    "staff",
    "sugar",
    "swine",
    "talent",
    "toothpaste",
    "traffic",
    "travel",
    "trivia",
    "trousers",
    "trout",
    "tuna",
    "us",
    "utopia",
    "vermontese",
    "vinegar",
    "weather",
    "wenchowese",
    "wheat",
    "whiting",
    "wildebeest",
    "wood",
    "wool",
    "yengeese",
];

/// Plural → singular pairs Doctrine substitutes whole-word, before any
/// suffix rule runs.
///
/// The suffix rules alone would mangle most of these (`leaves` → `leave`,
/// `cookies` → `cooky`, `viruses` → `viruse`), which is why the inflector
/// consults this table first.  Transcribed from Doctrine's own irregular
/// list so the two cannot drift; `axes` maps to `axe` because Doctrine
/// registers `axis`/`axes` first and `axe`/`axes` second, and the later
/// entry wins when the pairs are flipped for singularization.
const IRREGULAR_SINGULARS: &[(&str, &str)] = &[
    ("abuses", "abuse"),
    ("algae", "alga"),
    ("atlases", "atlas"),
    ("avalanches", "avalanche"),
    ("axes", "axe"),
    ("beefs", "beef"),
    ("blouses", "blouse"),
    ("brothers", "brother"),
    ("brownies", "brownie"),
    ("caches", "cache"),
    ("cafes", "cafe"),
    ("canvases", "canvas"),
    ("caves", "cave"),
    ("chateaux", "chateau"),
    ("children", "child"),
    ("cookies", "cookie"),
    ("corpuses", "corpus"),
    ("cows", "cow"),
    ("criteria", "criterion"),
    ("curricula", "curriculum"),
    ("curves", "curve"),
    ("demos", "demo"),
    ("dice", "die"),
    ("dominoes", "domino"),
    ("echoes", "echo"),
    ("emphases", "emphasis"),
    ("epochs", "epoch"),
    ("feet", "foot"),
    ("foes", "foe"),
    ("fungi", "fungus"),
    ("ganglions", "ganglion"),
    ("gases", "gas"),
    ("geese", "goose"),
    ("genera", "genus"),
    ("genies", "genie"),
    ("graffiti", "graffito"),
    ("graves", "grave"),
    ("hippopotami", "hippopotamus"),
    ("hoaxes", "hoax"),
    ("hoofs", "hoof"),
    ("humans", "human"),
    ("irises", "iris"),
    ("larvae", "larva"),
    ("leaves", "leaf"),
    ("lenses", "lens"),
    ("loaves", "loaf"),
    ("media", "medium"),
    ("memoranda", "memorandum"),
    ("men", "man"),
    ("mongooses", "mongoose"),
    ("monies", "money"),
    ("mottoes", "motto"),
    ("moves", "move"),
    ("mythoi", "mythos"),
    ("neuroses", "neurosis"),
    ("niches", "niche"),
    ("niveaux", "niveau"),
    ("nuclei", "nucleus"),
    ("numina", "numen"),
    ("nurseries", "nursery"),
    ("oases", "oasis"),
    ("occiputs", "occiput"),
    ("octopuses", "octopus"),
    ("opuses", "opus"),
    ("oxen", "ox"),
    ("passersby", "passerby"),
    ("penises", "penis"),
    ("people", "person"),
    ("plateaux", "plateau"),
    ("runners-up", "runner-up"),
    ("safes", "safe"),
    ("saves", "save"),
    ("sexes", "sex"),
    ("sieves", "sieve"),
    ("soliloquies", "soliloquy"),
    ("sons-in-law", "son-in-law"),
    ("stadiums", "stadium"),
    ("syllabi", "syllabus"),
    ("teeth", "tooth"),
    ("testes", "testis"),
    ("thieves", "thief"),
    ("tornadoes", "tornado"),
    ("trilbys", "trilby"),
    ("turfs", "turf"),
    ("valves", "valve"),
    ("volcanoes", "volcano"),
    ("waves", "wave"),
    ("zombies", "zombie"),
];
/// Reduce an English plural to its singular form, as `Str::singular()` does.
///
/// Laravel derives a resource route's wildcard from the resource name with
/// this, so `Route::resource('photos', …)` produces `photos/{photo}`.  Any
/// disagreement here becomes a parameter name the editor offers that the
/// application does not actually have, so this mirrors the three stages of
/// Doctrine's inflector rather than approximating them: the uninflected
/// list, then the irregular table, then the suffix rules in Doctrine's own
/// order, first match winning.
///
/// The capitalization of the input is carried over, as `Pluralizer` does, so
/// a resource written `Photos` yields the wildcard `{Photo}`.
pub(crate) fn singularize_english_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let singular = if is_uninflected(&lower) {
        lower
    } else if let Ok(index) =
        IRREGULAR_SINGULARS.binary_search_by_key(&lower.as_str(), |(plural, _)| plural)
    {
        IRREGULAR_SINGULARS[index].1.to_string()
    } else {
        singular_by_suffix(lower)
    };
    match_case(singular, word)
}

/// Recapitalize `singular` to match the word it was derived from.
///
/// Doctrine rewrites only a word's suffix and leaves the rest of the original
/// untouched, so the characters the two words still share keep the case they
/// were written with (`Blog-Posts` → `Blog-Post`, not `Blog-post`) and only
/// the rewritten tail comes back lowercased.  Everything up to the first
/// difference is therefore taken from the original.
fn match_case(singular: String, original: &str) -> String {
    if !original.chars().any(char::is_uppercase) {
        return singular;
    }
    let shared = original
        .chars()
        .zip(singular.chars())
        .take_while(|(from, to)| from.to_lowercase().eq(to.to_lowercase()))
        .count();
    original
        .chars()
        .take(shared)
        .chain(singular.chars().skip(shared))
        .collect()
}

/// Whether Doctrine's uninflected patterns cover this word.
///
/// Most of the table is plain words; the handful that are written as
/// patterns (`.*ss`, `\w+media`, `social media`) are matched here.  The
/// `sea[- ]bass` pattern needs no case of its own, `.*ss` already covers it.
fn is_uninflected(word: &str) -> bool {
    if word.ends_with("ss") || word == "social media" {
        return true;
    }
    // `\w+media`: at least one word character before the suffix, and no
    // separator anywhere, so `multimedia` matches but `social media` (handled
    // above) and `mixed-media` do not.
    if let Some(prefix) = word.strip_suffix("media")
        && !prefix.is_empty()
        && prefix.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return true;
    }
    UNINFLECTED_WORDS.binary_search(&word).is_ok()
}

/// Doctrine's ordered singular suffix rules, first match winning.
///
/// Each arm mirrors one `Inflectible::getSingular()` transformation, in the
/// same order, because a later rule regularly undoes an earlier one — the
/// trailing-`s` rule at the end would turn `viruses` into `viruse` if the
/// `([^a])uses` rule above it had not already produced `virus`.
fn singular_by_suffix(mut word: String) -> String {
    /// Replace `suffix` (already known to match) with `replacement`.
    fn rewrite(word: &mut String, suffix_len: usize, replacement: &str) {
        let stem = word.len() - suffix_len;
        word.truncate(stem);
        word.push_str(replacement);
    }

    // `(s)tatuses` / `(s)tatus` / `(c)ampus` — kept whole so the trailing-`s`
    // rule cannot bite a word that legitimately ends in `us`.
    if word.ends_with("statuses") {
        rewrite(&mut word, 2, "");
        return word;
    }
    if word.ends_with("status") || word.ends_with("campus") {
        return word;
    }
    if word.ends_with("menus") {
        rewrite(&mut word, 1, "");
        return word;
    }
    if word.ends_with("aliases") {
        rewrite(&mut word, 2, "");
        return word;
    }
    if word.ends_with("alias") {
        return word;
    }
    if word.ends_with("quizzes") {
        rewrite(&mut word, 3, "");
        return word;
    }
    if word.ends_with("matrices") {
        rewrite(&mut word, 4, "ix");
        return word;
    }
    if word.ends_with("vertices") || word.ends_with("indices") {
        rewrite(&mut word, 4, "ex");
        return word;
    }
    if word.starts_with("oxen") {
        word.replace_range(2..4, "");
        return word;
    }
    if let Some(stem) = word.strip_suffix("oes")
        && matches!(
            stem,
            s if s.ends_with("buffal")
                || s.ends_with("her")
                || s.ends_with("potat")
                || s.ends_with("tomat")
                || s.ends_with("volcan")
        )
    {
        rewrite(&mut word, 2, "");
        return word;
    }
    if let Some(stem) = word.strip_suffix('i')
        && [
            "alumn", "bacill", "cact", "foc", "fung", "nucle", "radi", "stimul", "syllab",
            "termin", "viri", "vir",
        ]
        .iter()
        .any(|s| stem.ends_with(s))
    {
        rewrite(&mut word, 1, "us");
        return word;
    }
    if let Some(stem) = word.strip_suffix("es")
        && (stem.ends_with("fax") || stem.ends_with("tax") || stem.ends_with("wax"))
    {
        rewrite(&mut word, 2, "");
        return word;
    }
    if let Some(stem) = word.strip_suffix("es")
        && ["analys", "ax", "cris", "test", "thes"]
            .iter()
            .any(|s| stem.ends_with(s))
    {
        rewrite(&mut word, 2, "is");
        return word;
    }
    if word.ends_with("shoes") || word.ends_with("slaves") {
        rewrite(&mut word, 1, "");
        return word;
    }
    if word.ends_with("oes") {
        rewrite(&mut word, 2, "");
        return word;
    }
    // `houses` keeps its `e`; every other `-uses` drops back to `-us`.
    if word.ends_with("ouses") {
        rewrite(&mut word, 1, "");
        return word;
    }
    if let Some(stem) = word.strip_suffix("uses")
        && !stem.ends_with('a')
    {
        rewrite(&mut word, 4, "us");
        return word;
    }
    if let Some(stem) = word.strip_suffix("ice")
        && (stem.ends_with('m') || stem.ends_with('l'))
    {
        rewrite(&mut word, 3, "ouse");
        return word;
    }
    if let Some(stem) = word.strip_suffix("es")
        && (stem.ends_with('x')
            || stem.ends_with("ch")
            || stem.ends_with("ss")
            || stem.ends_with("sh"))
    {
        rewrite(&mut word, 2, "");
        return word;
    }
    if word.ends_with("movies") {
        rewrite(&mut word, 1, "");
        return word;
    }
    if word.ends_with("series") {
        return word;
    }
    // `categories` → `category`, and `ties` → `ty`: Doctrine applies this to
    // any consonant, however short the stem, so the words where that reads
    // wrong (`cookies`, `brownies`) are held in the irregular table instead.
    if let Some(stem) = word.strip_suffix("ies")
        && (stem.ends_with("qu")
            || !matches!(stem.chars().last(), Some('a' | 'e' | 'i' | 'o' | 'u' | 'y')))
    {
        rewrite(&mut word, 3, "y");
        return word;
    }
    if let Some(stem) = word.strip_suffix("ves")
        && (stem.ends_with('l') || stem.ends_with('r'))
    {
        rewrite(&mut word, 3, "f");
        return word;
    }
    if word.ends_with("tives")
        || word.ends_with("hives")
        || word.ends_with("drives")
        || word.ends_with("dives")
        || word.ends_with("olives")
    {
        rewrite(&mut word, 1, "");
        return word;
    }
    if let Some(stem) = word.strip_suffix("ves")
        && !stem.ends_with('f')
        && !stem.ends_with('o')
    {
        rewrite(&mut word, 3, "fe");
        return word;
    }
    if let Some(stem) = word.strip_suffix("ses")
        && [
            "analy", "diagno", "ba", "parenthe", "progno", "synop", "the",
        ]
        .iter()
        .any(|s| stem.ends_with(s))
    {
        rewrite(&mut word, 3, "sis");
        return word;
    }
    if word.ends_with("taxa") {
        rewrite(&mut word, 1, "on");
        return word;
    }
    if word.ends_with("criteria") {
        rewrite(&mut word, 1, "on");
        return word;
    }
    if (word.ends_with("ia") || word.ends_with("ta")) && !word.ends_with("regatta") {
        rewrite(&mut word, 1, "um");
        return word;
    }
    if word.ends_with("people") {
        rewrite(&mut word, 5, "erson");
        return word;
    }
    if word.ends_with("men") {
        rewrite(&mut word, 2, "an");
        return word;
    }
    if word.ends_with("children") {
        rewrite(&mut word, 3, "");
        return word;
    }
    if word.ends_with("feet") {
        rewrite(&mut word, 3, "oot");
        return word;
    }
    if word.ends_with("news") || word == "tights" || word == "shorts" {
        return word;
    }
    if word.ends_with("eaus") {
        rewrite(&mut word, 1, "");
        return word;
    }
    if word.ends_with('s') {
        rewrite(&mut word, 1, "");
    }
    word
}

/// The first argument of a call, when it is a plain string literal.
pub(crate) fn first_string_arg<'c>(args: &ArgumentList<'_>, content: &'c str) -> Option<&'c str> {
    args.arguments
        .iter()
        .next()
        .and_then(|a| extract_string_literal(a.value(), content))
        .map(|(value, _, _)| value)
}

/// Collect the value a call chain accumulates for one group modifier.
///
/// Walking `Route::name('admin.')->middleware(…)` with `method = b"name"`
/// yields `"admin."`; the same walk with `method = b"prefix"` recovers the URI
/// prefix of `Route::prefix('admin')->…`.  Values found on nested chain links
/// are combined outermost-first by `join`.
fn chain_modifier_value(
    expr: &Expression<'_>,
    content: &str,
    method: &[u8],
    join: &dyn Fn(&str, &str) -> String,
) -> String {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return chain_modifier_value(mc.object, content, method, join);
            };
            let parent = chain_modifier_value(mc.object, content, method, join);
            if ident.value.eq_ignore_ascii_case(method) {
                let own = first_string_arg(&mc.argument_list, content).unwrap_or("");
                join(&parent, own)
            } else {
                parent
            }
        }
        // Route::name('prefix.') — static entry point of the chain.
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return String::new();
            };
            if ident.value.eq_ignore_ascii_case(method) {
                first_string_arg(&sc.argument_list, content)
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// Collect all `->name('...')` values from the call chain that precedes `->group()`.
///
/// Handles both instance method chains (`->name('prefix.')`) and the static
/// entry point (`Route::name('prefix.')`).
pub(crate) fn chain_name_prefix<'a>(expr: &Expression<'a>, content: &str) -> String {
    chain_modifier_value(expr, content, b"name", &|parent, own| {
        format!("{parent}{own}")
    })
}

/// Collect all `->prefix('...')` URI segments from the call chain that
/// precedes `->group()`, joined into a single prefix.
pub(crate) fn chain_uri_prefix<'a>(expr: &Expression<'a>, content: &str) -> String {
    chain_modifier_value(expr, content, b"prefix", &join_uri_segments)
}

/// The route-name prefix a chain sets ahead of a `resource()` registration.
///
/// `RouteRegistrar` aliases `->name()` onto `->as()`, so both spellings feed
/// the same attribute, and unlike the group modifiers they *replace* rather
/// than accumulate: `Route::as('a')->as('b')->resource(…)` names its routes
/// `b.photos.index`.  `None` means the chain never set one, which is not the
/// same as setting it to the empty string.
pub(crate) fn chain_as_prefix<'a>(expr: &Expression<'a>, content: &str) -> Option<String> {
    let is_as =
        |ident: &[u8]| ident.eq_ignore_ascii_case(b"as") || ident.eq_ignore_ascii_case(b"name");
    match expr {
        Expression::Call(Call::Method(mc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &mc.method else {
                return chain_as_prefix(mc.object, content);
            };
            // The outermost link is applied last, so it wins over anything
            // the rest of the chain set.
            if is_as(ident.value) {
                return Some(first_string_arg(&mc.argument_list, content)?.to_string());
            }
            chain_as_prefix(mc.object, content)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return None;
            };
            is_as(ident.value)
                .then(|| first_string_arg(&sc.argument_list, content).map(str::to_string))
                .flatten()
        }
        _ => None,
    }
}

// ─── Shared PHP AST walker ───────────────────────────────────────────────────

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_span::{HasSpan, Span};
use mago_syntax::cst::*;

/// Parse `content` as PHP and call `visitor` for every expression node
/// (pre-order, depth-first).  Used by navigation modules to find specific
/// function and static-method call patterns without duplicating the full
/// statement-walker boilerplate.
///
/// The visitor returns `ControlFlow::Continue(())` to keep walking or
/// `ControlFlow::Break(())` to stop early (e.g. after finding a match).
pub(crate) fn walk_all_php_expressions(
    content: &str,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    walk_program_expressions(program, visitor);
}

/// Like [`walk_all_php_expressions`], but for a `Program` the caller has
/// already parsed and needs to keep around afterwards (e.g. to resolve a
/// local variable back to its assignment).
pub(crate) fn walk_program_expressions(
    program: &Program<'_>,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    for stmt in program.statements.iter() {
        if walk_stmt_exprs(stmt, visitor).is_break() {
            return;
        }
    }
}

/// Like [`walk_program_expressions`], but rooted at a single expression the
/// caller already holds (e.g. one argument of a call it matched), descending
/// into closure and arrow-function bodies the same way.
pub(crate) fn walk_expression_tree(
    expr: &Expression<'_>,
    visitor: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) {
    let _ = walk_expr_depth(expr, visitor);
}

// ─── Scope walking ──────────────────────────────────────────────────────────

/// Whether `span` covers `offset`, inclusive at both ends.
pub(in crate::virtual_members::laravel) fn covers(span: Span, offset: u32) -> bool {
    offset >= span.start.offset && offset <= span.end.offset
}

/// The outermost function-like body containing `offset`.
///
/// Outermost rather than innermost so that a construct in a controller
/// action still applies inside a closure nested in that action — "earlier
/// in the same method" covers everything the method wraps. A sibling
/// method's body never contains the offset, so a lookup stays scoped to the
/// one being edited.
///
/// Spans nest, so only the cursor's own ancestors are descended into: a
/// subtree that does not cover the offset cannot hold the body that does.
/// That keeps the search proportional to nesting depth rather than to file
/// size.
pub(in crate::virtual_members::laravel) fn enclosing_body<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    offset: u32,
) -> Option<Node<'ast, 'arena>> {
    let body = match node {
        Node::Method(m) => match &m.body {
            MethodBody::Concrete(block) => Some(Node::Block(block)),
            MethodBody::Abstract(_) => None,
        },
        Node::Function(f) => Some(Node::Block(&f.body)),
        Node::Closure(c) => Some(Node::Block(&c.body)),
        _ => None,
    };
    if let Some(body) = body
        && covers(body.span(), offset)
    {
        return Some(body);
    }

    let mut found = None;
    node.visit_children(|child| {
        if found.is_none() && covers(child.span(), offset) {
            found = enclosing_body(child, offset);
        }
    });
    found
}

/// Hand every node of `node`'s subtree that starts before `cursor` to
/// `visit`.
///
/// Callers are looking for a construct that *completes* before the cursor,
/// and one cannot end before the cursor without starting before it, so
/// subtrees that begin at or after the cursor are skipped rather than walked.
pub(in crate::virtual_members::laravel) fn walk_before_cursor<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    visit: &mut impl FnMut(Node<'ast, 'arena>),
) {
    visit(node);
    node.visit_children(|child| {
        if child.span().start.offset < cursor {
            walk_before_cursor(child, cursor, visit);
        }
    });
}

/// Hand every node of `node`'s subtree that starts before `cursor` to
/// `visit`, stopping at anything that has variables of its own.
///
/// The file-scope counterpart to [`walk_before_cursor`], for an offset no
/// function-like body encloses: a route file registers its routes at the top
/// level of the script, so a `$path = …` written inside a function or closure
/// further up the file says nothing about the value in force there.
pub(in crate::virtual_members::laravel) fn walk_file_scope_before_cursor<'ast, 'arena>(
    node: Node<'ast, 'arena>,
    cursor: u32,
    visit: &mut impl FnMut(Node<'ast, 'arena>),
) {
    visit(node);
    node.visit_children(|child| {
        if child.span().start.offset < cursor && !opens_variable_scope(child) {
            walk_file_scope_before_cursor(child, cursor, visit);
        }
    });
}

/// Whether a node's body runs with a fresh set of local variables rather than
/// the ones its surroundings hold.
fn opens_variable_scope(node: Node<'_, '_>) -> bool {
    matches!(
        node,
        Node::Function(_)
            | Node::Closure(_)
            | Node::ArrowFunction(_)
            | Node::Method(_)
            | Node::PropertyHook(_)
    )
}

/// Whether a construct ending at `end` is a better candidate than the one
/// already in `best`: it has to finish before the cursor, and later beats
/// earlier so the nearest preceding construct wins.
pub(in crate::virtual_members::laravel) fn beats_best<T>(
    best: &Option<(u32, T)>,
    end: u32,
    cursor: u32,
) -> bool {
    end <= cursor && best.as_ref().is_none_or(|(seen, _)| end >= *seen)
}

/// Extract the raw string value and inner byte offsets from a PHP string
/// literal expression.  Returns `(value, inner_start, inner_end)` where
/// `content[inner_start..inner_end]` is the string content without quotes.
pub(crate) fn extract_string_literal<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'c str, usize, usize)> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start >= end || end > content.len() {
        return None;
    }
    Some((&content[start..end], start, end))
}

/// Walk statements, returning `Break` as soon as the visitor signals early exit.
fn walk_stmt_exprs(
    stmt: &Statement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match stmt {
        Statement::Expression(e) => walk_expr_depth(e.expression, f)?,
        Statement::Return(r) => {
            if let Some(v) = r.value {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Echo(e) => {
            for v in e.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Block(b) => {
            for s in b.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::If(if_stmt) => {
            walk_expr_depth(if_stmt.condition, f)?;
            for s in if_stmt.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
            for stmts in if_stmt.body.else_if_statements() {
                for s in stmts {
                    walk_stmt_exprs(s, f)?;
                }
            }
            if let Some(else_stmts) = if_stmt.body.else_statements() {
                for s in else_stmts {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        Statement::While(w) => {
            walk_expr_depth(w.condition, f)?;
            for s in w.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::DoWhile(dw) => {
            walk_expr_depth(dw.condition, f)?;
            walk_stmt_exprs(dw.statement, f)?;
        }
        Statement::For(fs) => {
            for init in fs.initializations.iter() {
                walk_expr_depth(init, f)?;
            }
            for cond in fs.conditions.iter() {
                walk_expr_depth(cond, f)?;
            }
            for update in fs.increments.iter() {
                walk_expr_depth(update, f)?;
            }
            for s in fs.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Foreach(fe) => {
            walk_expr_depth(fe.expression, f)?;
            for s in fe.body.statements() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Try(t) => {
            for s in t.block.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
            for catch in t.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
            if let Some(ref fin) = t.finally_clause {
                for s in fin.block.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        Statement::Switch(sw) => {
            walk_expr_depth(sw.expression, f)?;
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => {
                        walk_expr_depth(c.expression, f)?;
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                    SwitchCase::Default(c) => {
                        for s in c.statements.iter() {
                            walk_stmt_exprs(s, f)?;
                        }
                    }
                }
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Trait(t) => {
            for member in t.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }
        Statement::Enum(e) => {
            for member in e.members.iter() {
                walk_class_member_exprs(member, f)?;
            }
        }

        Statement::Static(s) => {
            for item in s.items.iter() {
                if let Some(init) = item.value() {
                    walk_expr_depth(init, f)?;
                }
            }
        }
        Statement::Unset(u) => {
            for v in u.values.iter() {
                walk_expr_depth(v, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_class_member_exprs(
    member: &ClassLikeMember<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match member {
        ClassLikeMember::Method(method) => {
            if let MethodBody::Concrete(body) = &method.body {
                for s in body.statements.iter() {
                    walk_stmt_exprs(s, f)?;
                }
            }
        }
        ClassLikeMember::Property(Property::Plain(prop)) => {
            for item in prop.items.iter() {
                if let PropertyItem::Concrete(concrete) = item {
                    walk_expr_depth(concrete.value, f)?;
                }
            }
        }
        ClassLikeMember::Constant(c) => {
            for item in c.items.iter() {
                walk_expr_depth(item.value, f)?;
            }
        }
        ClassLikeMember::EnumCase(ec) => {
            if let EnumCaseItem::Backed(backed) = &ec.item {
                walk_expr_depth(backed.value, f)?;
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_expr_depth(
    expr: &Expression<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    f(expr)?;
    match expr {
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                walk_expr_depth(fc.function, f)?;
                for arg in fc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::StaticMethod(sc) => {
                for arg in sc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::Method(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
            Call::NullSafeMethod(mc) => {
                walk_expr_depth(mc.object, f)?;
                for arg in mc.argument_list.arguments.iter() {
                    walk_expr_depth(arg.value(), f)?;
                }
            }
        },
        Expression::Binary(b) => {
            walk_expr_depth(b.lhs, f)?;
            walk_expr_depth(b.rhs, f)?;
        }
        Expression::UnaryPrefix(u) => walk_expr_depth(u.operand, f)?,
        Expression::UnaryPostfix(u) => walk_expr_depth(u.operand, f)?,
        Expression::Parenthesized(p) => walk_expr_depth(p.expression, f)?,
        Expression::Assignment(a) => {
            walk_expr_depth(a.lhs, f)?;
            walk_expr_depth(a.rhs, f)?;
        }
        Expression::Conditional(c) => {
            walk_expr_depth(c.condition, f)?;
            if let Some(then) = c.then {
                walk_expr_depth(then, f)?;
            }
            walk_expr_depth(c.r#else, f)?;
        }
        Expression::Array(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::LegacyArray(arr) => {
            for el in arr.elements.iter() {
                walk_array_el_depth(el, f)?;
            }
        }
        Expression::ArrayAccess(a) => {
            walk_expr_depth(a.array, f)?;
            walk_expr_depth(a.index, f)?;
        }
        Expression::Closure(c) => {
            for s in c.body.statements.iter() {
                walk_stmt_exprs(s, f)?;
            }
        }
        Expression::ArrowFunction(af) => walk_expr_depth(af.expression, f)?,
        Expression::Match(m) => {
            walk_expr_depth(m.expression, f)?;
            for arm in m.arms.iter() {
                match arm {
                    MatchArm::Expression(ea) => {
                        for cond in ea.conditions.iter() {
                            walk_expr_depth(cond, f)?;
                        }
                        walk_expr_depth(ea.expression, f)?;
                    }
                    MatchArm::Default(da) => walk_expr_depth(da.expression, f)?,
                }
            }
        }
        Expression::Throw(t) => walk_expr_depth(t.exception, f)?,
        Expression::Yield(y) => match y {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    walk_expr_depth(val, f)?;
                }
            }
            Yield::Pair(yp) => {
                walk_expr_depth(yp.key, f)?;
                walk_expr_depth(yp.value, f)?;
            }
            Yield::From(yf) => walk_expr_depth(yf.iterator, f)?,
        },
        Expression::Clone(c) => walk_expr_depth(c.object, f)?,
        Expression::Instantiation(inst) => {
            if let Some(args) = &inst.argument_list {
                for a in args.arguments.iter() {
                    walk_expr_depth(a.value(), f)?;
                }
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

fn walk_array_el_depth(
    el: &ArrayElement<'_>,
    f: &mut impl FnMut(&Expression<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    match el {
        ArrayElement::KeyValue(kv) => {
            walk_expr_depth(kv.key, f)?;
            walk_expr_depth(kv.value, f)?;
        }
        ArrayElement::Value(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Variadic(v) => walk_expr_depth(v.value, f)?,
        ArrayElement::Missing(_) => {}
    }
    ControlFlow::Continue(())
}

/// Try to extract a relative path from a `__DIR__ . '/path.php'` expression.
///
/// Returns the string literal portion (e.g. `"/routes/api.php"`).
pub(crate) fn extract_dir_concat_path<'a>(
    expr: &Expression<'a>,
    content: &'a str,
) -> Option<&'a str> {
    let Expression::Binary(bin) = expr else {
        return None;
    };
    let is_dir = matches!(
        bin.lhs,
        Expression::MagicConstant(MagicConstant::Directory { .. })
    );
    if !is_dir {
        return None;
    }
    let Expression::Literal(literal::Literal::String(s)) = bin.rhs else {
        return None;
    };
    if let Some(value) = s.value {
        Some(crate::atom::bytes_to_str(value))
    } else {
        let start = s.span.start.offset as usize + 1;
        let end = s.span.end.offset as usize - 1;
        if start < end && end <= content.len() {
            Some(&content[start..end])
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
