//! English singularization, mirroring the Doctrine inflector Laravel's
//! `Str::singular()` uses, for the wildcard a resource route derives from
//! its name.

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

#[cfg(test)]
#[path = "inflector_tests.rs"]
mod tests;
