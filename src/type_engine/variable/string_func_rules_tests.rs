use super::*;

use crate::php_type::PhpType;

/// Stand-in for a call's arguments: each slot is either a resolved type
/// or absent, plus the indices written as a bare constant name — the two
/// things the rules ask about.
struct Args(Vec<Option<PhpType>>, Vec<usize>);

impl ArrayFuncArgs for Args {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        self.0.get(index)?.clone()
    }
    fn bool_literal(&self, _index: usize) -> Option<bool> {
        None
    }
    fn has_arg(&self, index: usize) -> bool {
        self.0.get(index).is_some_and(Option::is_some)
    }
    fn is_spread(&self, _index: usize) -> bool {
        false
    }
    fn is_named(&self, _index: usize) -> bool {
        false
    }
    fn callback_declared_return_type(&self, _index: usize) -> Option<PhpType> {
        None
    }
    fn callback_inferred_return_type(&self, _index: usize, _param: &PhpType) -> Option<PhpType> {
        None
    }
    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool {
        inferred.is_subtype_of(declared)
    }
    fn arg_atom_text(&self, index: usize) -> Option<String> {
        self.1.contains(&index).then(|| "SOME_CONSTANT".to_string())
    }
    fn callback_param_narrowing(
        &self,
        _index: usize,
        _param_index: usize,
        _subject: &PhpType,
    ) -> Option<PhpType> {
        None
    }
}

fn args(types: &[Option<PhpType>]) -> Args {
    Args(types.to_vec(), Vec::new())
}

fn lit(value: &str) -> Option<PhpType> {
    Some(PhpType::literal_string_value(value))
}

fn fold(func: &str, types: &[Option<PhpType>]) -> Option<String> {
    string_func_return_type(func, &args(types)).map(|ty| ty.to_string())
}

#[test]
fn a_case_transform_folds_every_alternative_of_a_literal_union() {
    let subject = PhpType::union(vec![
        PhpType::literal_string_value("Interface"),
        PhpType::literal_string_value("Trait"),
        PhpType::literal_string_value("Class"),
    ]);
    assert_eq!(
        fold("strtolower", &[Some(subject)]).as_deref(),
        Some("'interface'|'trait'|'class'")
    );
}

#[test]
fn a_fully_qualified_call_folds_like_the_bare_name() {
    assert_eq!(fold("\\strtoupper", &[lit("ab")]).as_deref(), Some("'AB'"));
}

#[test]
fn the_first_character_transforms_leave_the_rest_alone() {
    assert_eq!(fold("ucfirst", &[lit("aBc")]).as_deref(), Some("'ABc'"));
    assert_eq!(fold("lcfirst", &[lit("ABC")]).as_deref(), Some("'aBC'"));
    assert_eq!(
        fold("ucwords", &[lit("hello big world")]).as_deref(),
        Some("'Hello Big World'")
    );
}

#[test]
fn the_trim_family_strips_its_default_characters() {
    assert_eq!(fold("trim", &[lit("  a  ")]).as_deref(), Some("'a'"));
    assert_eq!(fold("ltrim", &[lit("  a  ")]).as_deref(), Some("'a  '"));
    assert_eq!(fold("rtrim", &[lit("  a  ")]).as_deref(), Some("'  a'"));
}

#[test]
fn a_character_list_argument_declines_rather_than_stripping_the_default_set() {
    assert_eq!(fold("trim", &[lit("xax"), lit("x")]), None);
    assert_eq!(fold("ucwords", &[lit("a-b"), lit("-")]), None);
}

#[test]
fn str_replace_folds_a_plain_string_for_string_substitution() {
    assert_eq!(
        fold("str_replace", &[lit("_"), lit("-"), lit("a_b_c")]).as_deref(),
        Some("'a-b-c'")
    );
}

#[test]
fn str_repeat_folds_a_literal_count_and_declines_an_absurd_one() {
    assert_eq!(
        fold("str_repeat", &[lit("ab"), Some(PhpType::literal_int("3"))]).as_deref(),
        Some("'ababab'")
    );
    assert_eq!(
        fold(
            "str_repeat",
            &[lit("ab"), Some(PhpType::literal_int("100000"))]
        ),
        None
    );
    assert_eq!(fold("str_repeat", &[lit("ab"), None]), None);
}

#[test]
fn a_non_literal_argument_is_only_known_by_its_case() {
    assert_eq!(
        fold("strtolower", &[Some(PhpType::string())]).as_deref(),
        Some("lowercase-string")
    );
    assert_eq!(fold("strtolower", &[]), None);
}

#[test]
fn a_union_with_one_non_literal_member_does_not_fold() {
    let subject = PhpType::union(vec![
        PhpType::literal_string_value("Interface"),
        PhpType::string(),
    ]);
    assert_eq!(
        fold("strtolower", &[Some(subject)]).as_deref(),
        Some("lowercase-string")
    );
}

#[test]
fn a_multibyte_subject_does_not_fold_to_a_guessed_unicode_mapping() {
    assert_eq!(
        fold("strtoupper", &[lit("Ä")]).as_deref(),
        Some("non-empty-uppercase-string&non-falsy-string")
    );
}

#[test]
fn alternatives_that_fold_to_the_same_value_collapse() {
    let subject = PhpType::union(vec![
        PhpType::literal_string_value("Class"),
        PhpType::literal_string_value("CLASS"),
    ]);
    assert_eq!(
        fold("strtolower", &[Some(subject)]).as_deref(),
        Some("'class'")
    );
}

#[test]
fn an_argument_written_as_a_constant_declines() {
    let subject = [PhpType::literal_string_value("5.3.6")];
    let from_source = string_func_return_type(
        "strtolower",
        &Args(subject.iter().cloned().map(Some).collect(), Vec::new()),
    );
    assert_eq!(
        from_source.map(|ty| ty.to_string()).as_deref(),
        Some("'5.3.6'")
    );
    let from_constant = string_func_return_type(
        "strtolower",
        &Args(subject.into_iter().map(Some).collect(), vec![0]),
    );
    assert_eq!(
        from_constant.map(|ty| ty.to_string()).as_deref(),
        Some("lowercase-string")
    );
}

#[test]
fn a_function_outside_the_table_is_not_folded() {
    assert_eq!(fold("str_pad", &[lit("a")]), None);
    assert!(!has_string_func_rule("str_pad"));
    assert!(has_string_func_rule("\\strtolower"));
}

fn named(name: &str) -> Option<PhpType> {
    Some(PhpType::parse(name))
}

#[test]
fn a_case_mapping_keeps_how_far_from_empty_its_subject_was() {
    assert_eq!(
        fold("strtolower", &[named("non-empty-string")]).as_deref(),
        Some("non-empty-lowercase-string")
    );
    assert_eq!(
        fold("mb_strtoupper", &[named("non-falsy-string")]).as_deref(),
        Some("non-empty-uppercase-string&non-falsy-string")
    );
    assert_eq!(
        fold("strtolower", &[named("numeric-string")]).as_deref(),
        Some("non-empty-lowercase-string&numeric-string")
    );
}

#[test]
fn recasing_the_first_character_keeps_the_case_the_rest_had() {
    assert_eq!(
        fold("lcfirst", &[named("lowercase-string")]).as_deref(),
        Some("lowercase-string")
    );
    assert_eq!(
        fold("ucfirst", &[named("non-empty-lowercase-string")]).as_deref(),
        Some("non-empty-string")
    );
    assert_eq!(fold("ucfirst", &[Some(PhpType::string())]), None);
}

#[test]
fn sprintf_with_text_outside_its_placeholders_is_non_empty() {
    let s = Some(PhpType::string());
    assert_eq!(
        fold("sprintf", &[lit("key-%s"), s.clone()]).as_deref(),
        Some("non-falsy-string")
    );
    assert_eq!(
        fold("sprintf", &[lit("0%s"), s.clone()]).as_deref(),
        Some("non-empty-string")
    );
    assert_eq!(fold("sprintf", &[lit("%s%s"), s.clone(), s.clone()]), None);
    assert_eq!(
        fold("sprintf", &[lit("%%%s"), s.clone()]).as_deref(),
        Some("non-falsy-string")
    );
    assert_eq!(
        fold("vsprintf", &[lit("[%s]"), named("array")]).as_deref(),
        Some("non-falsy-string")
    );
}

#[test]
fn sprintf_placeholders_that_always_write_count_towards_the_length() {
    let int = Some(PhpType::int());
    assert_eq!(
        fold("sprintf", &[lit("%d%d"), int.clone(), int.clone()]).as_deref(),
        Some("non-falsy-string")
    );
    assert_eq!(
        fold("sprintf", &[lit("%5s"), Some(PhpType::string())]).as_deref(),
        Some("non-falsy-string")
    );
    // `%c` ignores its width, and a lone byte can be `"0"`.
    assert_eq!(
        fold("sprintf", &[lit("%5c"), int.clone()]).as_deref(),
        Some("non-empty-string")
    );
}

#[test]
fn sprintf_with_a_lone_placeholder_reads_its_value() {
    assert_eq!(
        fold("sprintf", &[lit("%d"), Some(PhpType::int())]).as_deref(),
        Some("numeric-string")
    );
    assert_eq!(
        fold("sprintf", &[lit("%s"), named("non-empty-string")]).as_deref(),
        Some("non-empty-string")
    );
    assert_eq!(
        fold(
            "sprintf",
            &[lit("%2$s"), Some(PhpType::int()), named("lowercase-string")]
        )
        .as_deref(),
        Some("lowercase-string")
    );
    assert_eq!(fold("sprintf", &[lit("%s"), Some(PhpType::int())]), None);
    assert_eq!(
        fold("sprintf", &[lit("%x"), Some(PhpType::int())]).as_deref(),
        Some("non-empty-string")
    );
}

#[test]
fn a_malformed_or_unknown_format_declines() {
    let s = Some(PhpType::string());
    assert_eq!(fold("sprintf", &[lit("key-%y"), s.clone()]), None);
    assert_eq!(fold("sprintf", &[lit("key-%0$s"), s.clone()]), None);
    assert_eq!(fold("sprintf", &[Some(PhpType::string()), s]), None);
}

#[test]
fn number_format_is_numeric_without_a_thousands_separator() {
    let float = Some(PhpType::float());
    let two = Some(PhpType::literal_int("2"));
    assert_eq!(
        fold(
            "number_format",
            &[float.clone(), two.clone(), lit("."), lit("")]
        )
        .as_deref(),
        Some("numeric-string")
    );
    assert_eq!(
        fold(
            "number_format",
            &[float.clone(), two.clone(), named("null"), lit("")]
        )
        .as_deref(),
        Some("numeric-string")
    );
    assert_eq!(
        fold(
            "number_format",
            &[float.clone(), two.clone(), lit(","), lit("")]
        ),
        None
    );
    assert_eq!(fold("number_format", &[float, two]), None);
}
