use crate::ClassCompletionOrigin;

pub(crate) fn origin_sort_tier(origin: ClassCompletionOrigin) -> char {
    origin.sort_tier()
}

pub(crate) fn flat_symbol_sort_text(
    short_name: &str,
    prefix: &str,
    origin_tier: char,
    source_tier: char,
) -> String {
    let quality = super::class_completion::match_quality(short_name, prefix);
    // source_tier before origin_tier: imported symbols always rank above
    // non-imported ones, regardless of provenance.
    format!(
        "{}{}{}_{}",
        quality,
        source_tier,
        origin_tier,
        short_name.to_lowercase()
    )
}
