//! The array `range()` builds.
//!
//! Literal bounds and step decide the values themselves, so the call is
//! run the way PHP runs it (see `PHP_FUNCTION(range)` in
//! `ext/standard/array.c`): two strings that are not numbers, or are single
//! digits, count through their first bytes, a fractional bound or step makes
//! every value a float, and a step pointing away from the end, a zero step
//! or one longer than the range throws. Each combination of a union's
//! literals is run on its own. Bounds that are not literals still decide
//! what kind of value the list holds.

use crate::php_type::{LiteralValue, PhpType, ShapeEntry};

use super::ArrayFuncArgs;

/// How many values a literal `range()` builds into a shape before it is
/// typed as a list of them instead.
const MAX_RANGE_ENTRIES: usize = 50;

/// How many combinations of literal arguments are run before the call is
/// typed from the arguments' kinds instead.
const MAX_RANGE_RUNS: usize = 32;

pub(super) fn range_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let start = args.arg_raw_type(0)?;
    let end = args.arg_raw_type(1)?;
    let step = if args.has_arg(2) {
        Some(args.arg_raw_type(2)?)
    } else {
        None
    };
    if let Some(folded) = literal_ranges(&start, &end, step.as_ref()) {
        return Some(folded);
    }
    Some(non_empty_list(range_value_type(
        &start,
        &end,
        step.as_ref(),
    )))
}

fn non_empty_list(value: PhpType) -> PhpType {
    PhpType::list(value).non_empty_array_form()
}

/// The values a range over bounds that are not all literals holds.
fn range_value_type(start: &PhpType, end: &PhpType, step: Option<&PhpType>) -> PhpType {
    use crate::type_engine::type_ops::int_interval::IntInterval;

    let int_ty = PhpType::int();
    let float_ty = PhpType::float();
    let int_step = step.is_none_or(|step| step.is_subtype_of(&int_ty));
    if int_step && start.is_subtype_of(&int_ty) && end.is_subtype_of(&int_ty) {
        // Every value lies between the bounds, whichever way the range runs.
        let hull = |ty: &PhpType| {
            let members = ty.union_members();
            let intervals: Option<Vec<IntInterval>> =
                members.iter().map(|m| IntInterval::of_int(m)).collect();
            intervals.map(|intervals| IntInterval::hull_of(&intervals))
        };
        return match (hull(start), hull(end)) {
            (Some(a), Some(b)) => IntInterval::hull_of(&[a, b]).into_type(),
            _ => int_ty,
        };
    }
    if start.is_subtype_of(&float_ty) && end.is_subtype_of(&float_ty) {
        return float_ty;
    }
    let number = PhpType::union(vec![int_ty.clone(), float_ty.clone()]);
    if start.is_subtype_of(&number)
        && end.is_subtype_of(&number)
        && step.is_none_or(|step| step.is_subtype_of(&number))
    {
        return number;
    }
    // A string bound counts through characters, or through numbers when it
    // is empty or numeric, so the values can be any of the three.
    PhpType::benevolent(PhpType::union(vec![int_ty, float_ty, PhpType::string()]))
}

/// The union of the arrays every combination of the arguments' literals
/// builds. `None` when an argument is not made of literals PHP would accept
/// as a bound or step, or a run could not be typed.
fn literal_ranges(start: &PhpType, end: &PhpType, step: Option<&PhpType>) -> Option<PhpType> {
    let literals = |ty: &PhpType| -> Option<Vec<LiteralValue>> {
        ty.union_members()
            .into_iter()
            .map(|member| member.as_literal().cloned())
            .collect()
    };
    let starts: Vec<Bound> = literals(start)?
        .iter()
        .map(Bound::of)
        .collect::<Option<_>>()?;
    let ends: Vec<Bound> = literals(end)?
        .iter()
        .map(Bound::of)
        .collect::<Option<_>>()?;
    let steps: Vec<Option<Step>> = match step {
        Some(step) => literals(step)?
            .iter()
            .map(Step::of)
            .collect::<Option<_>>()?,
        None => vec![Some(Step::Int {
            size: 1,
            negative: false,
        })],
    };

    if starts.len() * ends.len() * steps.len() > MAX_RANGE_RUNS {
        return None;
    }

    let mut shapes: Vec<(Vec<PhpType>, usize)> = Vec::new();
    let mut lists: Vec<PhpType> = Vec::new();
    for &start in &starts {
        for &end in &ends {
            // A step PHP rejects throws, so that combination builds nothing.
            for step in steps.iter().flatten() {
                match run(start, end, *step)? {
                    Run::Throws => {}
                    Run::Values(values) => add_shape(&mut shapes, values),
                    Run::Many(value) => {
                        let list = non_empty_list(value);
                        if !lists.contains(&list) {
                            lists.push(list);
                        }
                    }
                }
            }
        }
    }
    if shapes.is_empty() && lists.is_empty() {
        return None;
    }
    let mut members: Vec<PhpType> = shapes
        .into_iter()
        .map(|(values, required)| shape_of(values, required))
        .collect();
    members.extend(lists);
    Some(PhpType::union(members))
}

/// Fold `values` into the shape whose values it starts or is started by:
/// `range(1, $two ? 2 : 1)` is `array{0: 1, 1?: 2}`. The shapes are kept as
/// their longest values and how many of them are always there.
fn add_shape(shapes: &mut Vec<(Vec<PhpType>, usize)>, values: Vec<PhpType>) {
    for (longest, required) in shapes.iter_mut() {
        let shared = longest.len().min(values.len());
        if longest[..shared] == values[..shared] {
            *required = (*required).min(values.len());
            if values.len() > longest.len() {
                *longest = values;
            }
            return;
        }
    }
    let required = values.len();
    shapes.push((values, required));
}

fn shape_of(values: Vec<PhpType>, required: usize) -> PhpType {
    let has_optional = values.len() > required;
    PhpType::array_shape(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value_type)| ShapeEntry {
                // An entry that may be missing names the index it sits at.
                key: has_optional.then(|| index.to_string()),
                value_type,
                optional: index >= required,
            })
            .collect(),
    )
}

/// A start or end argument as `range()` reads it.
#[derive(Clone, Copy)]
enum Bound {
    Int(i64),
    Float(f64),
    /// A string that is not a number, or is a single digit: its first byte
    /// counts through characters when the other bound is one too, and
    /// otherwise it stands for `number` (`0`, or the digit).
    Char {
        byte: u8,
        number: i64,
    },
}

impl Bound {
    fn of(literal: &LiteralValue) -> Option<Self> {
        match literal {
            LiteralValue::Int(_) => literal.parse_i64().map(Bound::Int),
            LiteralValue::Float(_) => literal
                .parse_f64()
                .filter(|v| v.is_finite())
                .map(Bound::Float),
            LiteralValue::String(_) => {
                let content = literal.exact_string_content()?;
                let Some(&byte) = content.as_bytes().first() else {
                    // An empty string is read as `0`, with a warning.
                    return Some(Bound::Int(0));
                };
                match literal.numeric_string_value() {
                    Some(number @ LiteralValue::Int(_)) => {
                        let number = number.parse_i64()?;
                        Some(if content.len() == 1 {
                            Bound::Char { byte, number }
                        } else {
                            Bound::Int(number)
                        })
                    }
                    Some(number) => number
                        .parse_f64()
                        .filter(|v| v.is_finite())
                        .map(Bound::Float),
                    None => Some(Bound::Char { byte, number: 0 }),
                }
            }
        }
    }

    fn number(self) -> Number {
        match self {
            Bound::Int(v) | Bound::Char { number: v, .. } => Number::Int(v),
            Bound::Float(v) => Number::Float(v),
        }
    }
}

#[derive(Clone, Copy)]
enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    fn as_f64(self) -> f64 {
        match self {
            Number::Int(v) => v as f64,
            Number::Float(v) => v,
        }
    }
}

/// The step's size, and whether it was written negative. A float step that
/// is a whole number counts like an integer one.
#[derive(Clone, Copy)]
enum Step {
    Int { size: u64, negative: bool },
    Float { size: f64, negative: bool },
}

impl Step {
    /// `Some(None)` for a step PHP rejects outright (zero, not a number).
    fn of(literal: &LiteralValue) -> Option<Option<Self>> {
        match literal {
            LiteralValue::Int(_) => {
                let v = literal.parse_i64()?;
                Some((v != 0).then(|| Step::Int {
                    size: v.unsigned_abs(),
                    negative: v < 0,
                }))
            }
            LiteralValue::Float(_) => {
                let v = literal.parse_f64()?;
                if !v.is_finite() || v == 0.0 {
                    return Some(None);
                }
                let size = v.abs();
                Some(Some(if size.fract() == 0.0 && size < 9.0e15 {
                    Step::Int {
                        size: size as u64,
                        negative: v < 0.0,
                    }
                } else {
                    Step::Float {
                        size,
                        negative: v < 0.0,
                    }
                }))
            }
            LiteralValue::String(_) => match literal.numeric_string_value() {
                Some(number) => Step::of(&number),
                None => Some(None),
            },
        }
    }
}

enum Run {
    Throws,
    Values(Vec<PhpType>),
    /// More values than [`MAX_RANGE_ENTRIES`], all of this type.
    Many(PhpType),
}

/// Run `range(start, end, step)`. `None` when the values cannot be spelled
/// as types (a byte past ASCII).
fn run(start: Bound, end: Bound, step: Step) -> Option<Run> {
    if let (
        Bound::Char { byte: low, .. },
        Bound::Char { byte: high, .. },
        Step::Int { size, negative },
    ) = (start, end, step)
    {
        if !low.is_ascii() || !high.is_ascii() {
            return None;
        }
        return Some(
            match int_steps(i64::from(low), i64::from(high), size, negative) {
                None => Run::Throws,
                Some(values) => {
                    let values: Vec<PhpType> = values
                        .map(|byte| PhpType::literal_string_value((byte as u8 as char).to_string()))
                        .collect();
                    if values.len() > MAX_RANGE_ENTRIES {
                        Run::Many(PhpType::parse("non-empty-string"))
                    } else {
                        Run::Values(values)
                    }
                }
            },
        );
    }
    let (start, end) = (start.number(), end.number());
    match (start, end, step) {
        (Number::Int(start), Number::Int(end), Step::Int { size, negative }) => {
            Some(match int_steps(start, end, size, negative) {
                None => Run::Throws,
                Some(values) if values.len() > MAX_RANGE_ENTRIES => Run::Many(PhpType::int_range(
                    start.min(end).to_string(),
                    start.max(end).to_string(),
                )),
                Some(values) => Run::Values(
                    values
                        .map(|v| PhpType::literal_int(v.to_string()))
                        .collect(),
                ),
            })
        }
        _ => {
            let (size, negative) = match step {
                Step::Int { size, negative } => (size as f64, negative),
                Step::Float { size, negative } => (size, negative),
            };
            Some(
                match float_steps(start.as_f64(), end.as_f64(), size, negative) {
                    None => Run::Throws,
                    Some(values) if values.len() > MAX_RANGE_ENTRIES => Run::Many(PhpType::float()),
                    Some(values) => Run::Values(
                        values
                            .into_iter()
                            .map(|v| PhpType::literal_float(format!("{v:?}")))
                            .collect(),
                    ),
                },
            )
        }
    }
}

/// The integers from `start` towards `end`, `None` when PHP throws.
fn int_steps(
    start: i64,
    end: i64,
    size: u64,
    negative: bool,
) -> Option<impl ExactSizeIterator<Item = i64>> {
    let span = start.abs_diff(end);
    if end > start && negative || span != 0 && span < size {
        return None;
    }
    let count = span / size + 1;
    // Only the length is needed past the limit, so a huge range is not built.
    let count = usize::try_from(count.min(MAX_RANGE_ENTRIES as u64 + 1)).ok()?;
    let down = start > end;
    Some((0..count).map(move |i| {
        let offset = (i as i128) * i128::from(size);
        let value = if down {
            i128::from(start) - offset
        } else {
            i128::from(start) + offset
        };
        value as i64
    }))
}

/// The floats from `start` towards `end`, `None` when PHP throws.
fn float_steps(start: f64, end: f64, size: f64, negative: bool) -> Option<Vec<f64>> {
    if start == end {
        return Some(vec![start]);
    }
    let span = (start - end).abs();
    if end > start && negative || span < size {
        return None;
    }
    let count = (span / size + 1.0).round();
    let count = count.min((MAX_RANGE_ENTRIES + 1) as f64) as usize;
    let down = start > end;
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let value = if down {
            start - i as f64 * size
        } else {
            start + i as f64 * size
        };
        if (down && value < end) || (!down && value > end) {
            break;
        }
        values.push(value);
    }
    Some(values)
}
