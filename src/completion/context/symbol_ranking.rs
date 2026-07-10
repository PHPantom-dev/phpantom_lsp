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
    format!(
        "{}{}{}_{}",
        quality,
        origin_tier,
        source_tier,
        short_name.to_lowercase()
    )
}
