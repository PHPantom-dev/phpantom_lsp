use super::*;

/// Every pair below was produced by running `Str::singular()` against the
/// framework in `examples/laravel`, so this pins the singularizer to the
/// real inflector rather than to what our own rules happen to do.  Laravel
/// derives a resource route's wildcard with `Str::singular()`, so a
/// disagreement here is a parameter name the editor offers that the
/// application does not have.
///
/// Some of these read oddly as English (`ties` → `ty`, `curves` → `curve`
/// only because it is in the irregular table).  That is Doctrine's
/// behaviour, and matching Laravel is what makes the feature correct.
#[test]
fn singularizes_exactly_as_laravel_does() {
    const CASES: &[(&str, &str)] = &[
        ("photos", "photo"),
        ("users", "user"),
        ("posts", "post"),
        ("comments", "comment"),
        ("categories", "category"),
        ("companies", "company"),
        ("cities", "city"),
        ("countries", "country"),
        ("tags", "tag"),
        ("orders", "order"),
        ("products", "product"),
        ("invoices", "invoice"),
        ("payments", "payment"),
        ("articles", "article"),
        ("tickets", "ticket"),
        ("reviews", "review"),
        ("replies", "reply"),
        ("entries", "entry"),
        ("libraries", "library"),
        ("roles", "role"),
        ("files", "file"),
        ("profiles", "profile"),
        ("teams", "team"),
        ("projects", "project"),
        ("settings", "setting"),
        ("menus", "menu"),
        ("addresses", "address"),
        ("boxes", "box"),
        ("taxes", "tax"),
        ("classes", "class"),
        ("matches", "match"),
        ("branches", "branch"),
        ("statuses", "status"),
        ("media", "medium"),
        ("series", "series"),
        ("news", "news"),
        ("bakeries", "bakery"),
        ("ovens", "oven"),
        ("blog-posts", "blog-post"),
        ("cookies", "cookie"),
        ("leaves", "leaf"),
        ("bonuses", "bonus"),
        ("viruses", "virus"),
        ("heroes", "hero"),
        ("quizzes", "quiz"),
        ("matrices", "matrix"),
        ("indices", "index"),
        ("vertices", "vertex"),
        ("oxen", "ox"),
        ("aliases", "alias"),
        ("potatoes", "potato"),
        ("tomatoes", "tomato"),
        ("volcanoes", "volcano"),
        ("echoes", "echo"),
        ("buffaloes", "buffalo"),
        ("fungi", "fungus"),
        ("nuclei", "nucleus"),
        ("syllabi", "syllabus"),
        ("cacti", "cactus"),
        ("radii", "radius"),
        ("faxes", "fax"),
        ("waxes", "wax"),
        ("analyses", "analysis"),
        ("crises", "crisis"),
        ("theses", "thesis"),
        ("testes", "testis"),
        ("diagnoses", "diagnosis"),
        ("parentheses", "parenthesis"),
        ("synopses", "synopsis"),
        ("bases", "basis"),
        ("prognoses", "prognosis"),
        ("shoes", "shoe"),
        ("slaves", "slave"),
        ("houses", "house"),
        ("buses", "bus"),
        ("campuses", "campus"),
        ("focuses", "focus"),
        ("octopuses", "octopus"),
        ("mice", "mouse"),
        ("lice", "louse"),
        ("dishes", "dish"),
        ("watches", "watch"),
        ("movies", "movie"),
        ("ties", "ty"),
        ("dies", "dy"),
        ("lies", "ly"),
        ("pies", "py"),
        ("soliloquies", "soliloquy"),
        ("nurseries", "nursery"),
        ("brownies", "brownie"),
        ("zombies", "zombie"),
        ("genies", "genie"),
        ("wolves", "wolf"),
        ("shelves", "shelf"),
        ("selves", "self"),
        ("halves", "half"),
        ("elves", "elf"),
        ("knives", "knife"),
        ("wives", "wife"),
        ("lives", "life"),
        ("thieves", "thief"),
        ("loaves", "loaf"),
        ("olives", "olive"),
        ("natives", "native"),
        ("drives", "drive"),
        ("hives", "hive"),
        ("curves", "curve"),
        ("valves", "valve"),
        ("waves", "wave"),
        ("caves", "cave"),
        ("graves", "grave"),
        ("moves", "move"),
        ("saves", "save"),
        ("sieves", "sieve"),
        ("safes", "safe"),
        ("criteria", "criterion"),
        ("memoranda", "memorandum"),
        ("curricula", "curriculum"),
        ("stadiums", "stadium"),
        ("people", "person"),
        ("children", "child"),
        ("men", "man"),
        ("women", "woman"),
        ("feet", "foot"),
        ("teeth", "tooth"),
        ("geese", "goose"),
        ("data", "data"),
        ("axes", "axe"),
        ("dice", "die"),
        ("monies", "money"),
        ("gases", "gas"),
        ("lenses", "lens"),
        ("irises", "iris"),
        ("atlases", "atlas"),
        ("canvases", "canvas"),
        ("corpuses", "corpus"),
        ("penises", "penis"),
        ("sexes", "sex"),
        ("hoaxes", "hoax"),
        ("niches", "niche"),
        ("caches", "cache"),
        ("cafes", "cafe"),
        ("demos", "demo"),
        ("epochs", "epoch"),
        ("humans", "human"),
        ("cows", "cow"),
        ("brothers", "brother"),
        ("foes", "foe"),
        ("turfs", "turf"),
        ("hoofs", "hoof"),
        ("beefs", "beef"),
        ("trilbys", "trilby"),
        ("plateaux", "plateau"),
        ("chateaux", "chateau"),
        ("niveaux", "niveau"),
        ("passersby", "passerby"),
        ("runners-up", "runner-up"),
        ("sons-in-law", "son-in-law"),
        ("graffiti", "graffito"),
        ("mythoi", "mythos"),
        ("numina", "numen"),
        ("genera", "genus"),
        ("larvae", "larva"),
        ("algae", "alga"),
        ("oases", "oasis"),
        ("neuroses", "neurosis"),
        ("emphases", "emphasis"),
        ("tornadoes", "tornado"),
        ("mottoes", "motto"),
        ("dominoes", "domino"),
        ("hippopotami", "hippopotamus"),
        ("mongooses", "mongoose"),
        ("opuses", "opus"),
        ("occiputs", "occiput"),
        ("ganglions", "ganglion"),
        ("avalanches", "avalanche"),
        ("blouses", "blouse"),
        ("abuses", "abuse"),
        ("multimedia", "multimedia"),
        ("equipment", "equipment"),
        ("fish", "fish"),
        ("sheep", "sheep"),
        ("species", "species"),
        ("information", "information"),
        ("money", "money"),
        ("rice", "rice"),
        ("clothes", "clothes"),
        ("pants", "pants"),
        ("trivia", "trivia"),
        ("aircraft", "aircraft"),
        ("police", "police"),
        ("progress", "progress"),
        ("business", "business"),
        ("photo", "photo"),
        ("address", "address"),
        ("status", "status"),
        ("campus", "campus"),
        ("alias", "alias"),
        ("s", ""),
        ("es", "e"),
        ("", ""),
    ];

    for (plural, singular) in CASES {
        assert_eq!(
            &singularize_english_word(plural),
            singular,
            "singular of `{plural}`"
        );
    }
}

#[test]
fn singularizing_short_and_empty_words_does_not_panic() {
    assert_eq!(singularize_english_word(""), "");
    assert_eq!(singularize_english_word("s"), "");
    assert_eq!(singularize_english_word("es"), "e");
}

/// A hyphenated resource name singularizes only its last word; turning the
/// hyphens into underscores is the caller's job.
#[test]
fn singularizes_only_the_final_word_of_a_compound() {
    assert_eq!(singularize_english_word("blog-posts"), "blog-post");
}

/// `Pluralizer` carries the input's capitalization over to the result, so a
/// capitalized resource name keeps its shape in the derived wildcard.
#[test]
fn singularizing_preserves_the_case_of_the_input() {
    assert_eq!(singularize_english_word("Photos"), "Photo");
    assert_eq!(singularize_english_word("PHOTOS"), "PHOTO");
    assert_eq!(singularize_english_word("Categories"), "Category");
    assert_eq!(singularize_english_word("People"), "Person");
    assert_eq!(singularize_english_word("CHILDREN"), "CHILD");
    assert_eq!(singularize_english_word("Blog-Posts"), "Blog-Post");
}

/// Both tables are searched with `binary_search`, which silently returns the
/// wrong answer if they are not sorted.
#[test]
fn lookup_tables_are_sorted() {
    assert!(UNINFLECTED_WORDS.windows(2).all(|w| w[0] < w[1]));
    assert!(IRREGULAR_SINGULARS.windows(2).all(|w| w[0].0 < w[1].0));
}
