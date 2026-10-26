//! The fields a Symfony form type adds, as properties of its data class.
//!
//! A form type that sets a `data_class` in `configureOptions()` maps each
//! field `buildForm()` adds to the property of that name: `->add('email')`
//! reads and writes `User::$email`.  The field name is a reference to the
//! property, so the symbol map records it as a member access on the data
//! class, which every member feature already reads.
//!
//! Both halves are read from the form type's own AST: the `data_class` a
//! `setDefaults()` or `setDefault()` gives, and the calls on the builder
//! `buildForm()` takes as a `FormBuilderInterface`.  Completion runs the
//! same reading over the buffer, so the two cannot disagree about which
//! strings name a field.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;

const FORM_BUILDER: &str = "Symfony\\Component\\Form\\FormBuilderInterface";

/// What a form type's fields are mapped to: its data class, as written
/// before `::class` and the offset it is written at, and the content range
/// of each string a field is named by (which may be empty while it is
/// being typed).
pub(crate) struct FormFields<'a> {
    pub data_class: (&'a str, u32),
    pub fields: Vec<(u32, u32)>,
}

/// The fields `class` maps to its data class, when it is a form type that
/// declares one.  `resolve` gives the class a name written at an offset
/// names.
pub(crate) fn form_fields<'a>(
    class: &Class<'a>,
    resolve: &dyn Fn(&str, u32) -> String,
) -> Option<FormFields<'a>> {
    let mut configure = None;
    let mut build = None;
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let MethodBody::Concrete(body) = &method.body else {
            continue;
        };
        let name = method.name.value;
        if name.eq_ignore_ascii_case(b"configureOptions") {
            configure = Some(body);
        } else if name.eq_ignore_ascii_case(b"buildForm") {
            build = Some((method, body));
        }
    }
    let (build, build_body) = build?;
    let data_class = configure.and_then(|body| data_class(&body.statements))?;

    let builder = build.parameter_list.parameters.first()?;
    let Some(Hint::Identifier(hint)) = &builder.hint else {
        return None;
    };
    let hint_name = bytes_to_str(hint.value());
    if !resolve(hint_name, hint.span().start.offset).eq_ignore_ascii_case(FORM_BUILDER) {
        return None;
    }

    let mut fields = Vec::new();
    for statement in build_body.statements.iter() {
        collect_statement_fields(statement, builder.variable.name, &mut fields);
    }
    Some(FormFields { data_class, fields })
}

/// The class a `configureOptions()` body gives as the `data_class` option.
fn data_class<'a>(statements: &Sequence<'a, Statement<'a>>) -> Option<(&'a str, u32)> {
    statements.iter().find_map(|statement| {
        let Statement::Expression(statement) = statement else {
            return None;
        };
        let mut found = None;
        for_each_chained_call(statement.expression, &mut |name, arguments| {
            if found.is_some() {
                return;
            }
            let mut positional = arguments.arguments.iter().map(Argument::value);
            if name.eq_ignore_ascii_case(b"setDefaults") {
                found = positional.next().and_then(array_data_class);
            } else if name.eq_ignore_ascii_case(b"setDefault")
                && let (Some(option), Some(value)) = (positional.next(), positional.next())
                && is_string(option, "data_class")
            {
                found = class_name(value);
            }
        });
        found
    })
}

/// The `data_class` entry of an options array.
fn array_data_class<'a>(array: &Expression<'a>) -> Option<(&'a str, u32)> {
    let elements = match array {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return None,
    };
    elements.iter().find_map(|element| match element {
        ArrayElement::KeyValue(entry) if is_string(entry.key, "data_class") => {
            class_name(entry.value)
        }
        _ => None,
    })
}

/// The class `Foo::class` names, as written, and its offset.
fn class_name<'a>(expr: &Expression<'a>) -> Option<(&'a str, u32)> {
    let identifier = crate::parser::class_fetch_identifier(expr)?;
    Some((
        bytes_to_str(identifier.value()),
        identifier.span().start.offset,
    ))
}

fn is_string(expr: &Expression<'_>, value: &str) -> bool {
    matches!(
        expr,
        Expression::Literal(Literal::String(literal))
            if literal.value.and_then(crate::atom::literal_bytes_to_str) == Some(value)
    )
}

/// Collect the fields the calls in `statement` add to the builder held in
/// the variable `builder`, looking into the branches and loops a form type
/// adds fields conditionally in.
fn collect_statement_fields(
    statement: &Statement<'_>,
    builder: &[u8],
    fields: &mut Vec<(u32, u32)>,
) {
    match statement {
        Statement::Expression(statement) => {
            collect_chain_fields(statement.expression, builder, fields);
        }
        Statement::If(statement) => crate::parser::for_each_if_branch(statement, |statements| {
            for statement in statements {
                collect_statement_fields(statement, builder, fields);
            }
        }),
        Statement::Foreach(statement) => {
            for statement in statement.body.statements() {
                collect_statement_fields(statement, builder, fields);
            }
        }
        Statement::Block(block) => {
            for statement in block.statements.iter() {
                collect_statement_fields(statement, builder, fields);
            }
        }
        _ => {}
    }
}

/// Collect the fields a call chain on the builder names.  `add()` and
/// `remove()` return the builder itself, so the chain goes on naming its
/// fields; `get()` returns a field's own builder, whose fields are another
/// form's.
fn collect_chain_fields(expr: &Expression<'_>, builder: &[u8], fields: &mut Vec<(u32, u32)>) {
    let mut calls = Vec::new();
    let Some(root) = chain_root(expr, &mut calls) else {
        return;
    };
    if !matches!(root, Expression::Variable(Variable::Direct(variable)) if variable.name == builder)
    {
        return;
    }
    for (name, arguments) in calls {
        let names_field = ["add", "get", "has", "remove"]
            .iter()
            .any(|field_call| name.eq_ignore_ascii_case(field_call.as_bytes()));
        if !names_field {
            return;
        }
        let mut positional = arguments.arguments.iter();
        if let Some(Argument::Positional(field)) = positional.next()
            && let Expression::Literal(Literal::String(literal)) = field.value
            && !(name.eq_ignore_ascii_case(b"add") && is_unmapped(positional.nth(1)))
        {
            fields.push((literal.span.start.offset + 1, literal.span.end.offset - 1));
        }
        if !(name.eq_ignore_ascii_case(b"add") || name.eq_ignore_ascii_case(b"remove")) {
            return;
        }
    }
}

/// Whether the options an `add()` passes map the field to something other
/// than the data-class property of its name: `'mapped' => false`, or a
/// `property_path`.
fn is_unmapped(options: Option<&Argument<'_>>) -> bool {
    let elements = match options.map(Argument::value) {
        Some(Expression::Array(array)) => &array.elements,
        Some(Expression::LegacyArray(array)) => &array.elements,
        _ => return false,
    };
    elements.iter().any(|element| match element {
        ArrayElement::KeyValue(entry) => {
            is_string(entry.key, "property_path")
                || (is_string(entry.key, "mapped")
                    && matches!(entry.value, Expression::Literal(Literal::False(_))))
        }
        _ => false,
    })
}

/// The receiver a chain of method calls starts from, with the calls in the
/// order they run.
fn chain_root<'e, 'a>(
    expr: &'e Expression<'a>,
    calls: &mut Vec<(&'a [u8], &'e ArgumentList<'a>)>,
) -> Option<&'e Expression<'a>> {
    match expr {
        Expression::Call(Call::Method(call)) => {
            let root = chain_root(call.object, calls)?;
            let ClassLikeMemberSelector::Identifier(name) = &call.method else {
                return None;
            };
            calls.push((name.value, &call.argument_list));
            Some(root)
        }
        Expression::Parenthesized(inner) => chain_root(inner.expression, calls),
        _ => Some(expr),
    }
}

/// Visit each call of a method-call chain, from its receiver outward.
fn for_each_chained_call<'a>(
    expr: &Expression<'a>,
    visit: &mut impl FnMut(&'a [u8], &ArgumentList<'a>),
) {
    let mut calls = Vec::new();
    if chain_root(expr, &mut calls).is_some() {
        for (name, arguments) in calls {
            visit(name, arguments);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields_of(php: &str) -> Option<(String, Vec<String>)> {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let class = program
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Class(class) => Some(class),
                _ => None,
            })?;
        let resolve = |name: &str, _: u32| match name {
            "FormBuilderInterface" => FORM_BUILDER.to_string(),
            other => other.to_string(),
        };
        let found = form_fields(class, &resolve)?;
        Some((
            found.data_class.0.to_string(),
            found
                .fields
                .iter()
                .map(|(start, end)| php[*start as usize..*end as usize].to_string())
                .collect(),
        ))
    }

    #[test]
    fn fields_on_the_builder_are_read_with_their_data_class() {
        let php = "<?php\nclass T {\n    public function buildForm(FormBuilderInterface $builder, array $options): void {\n        $builder->add('email')->add('name', null, ['mapped' => false])->add('')->get('address')->add('street');\n        if ($options['x']) { $builder->remove('age'); }\n        $other->add('nope');\n    }\n    public function configureOptions($resolver): void {\n        $resolver->setDefaults(['data_class' => User::class]);\n    }\n}\n";
        assert_eq!(
            fields_of(php),
            Some((
                "User".to_string(),
                vec![
                    "email".to_string(),
                    String::new(),
                    "address".to_string(),
                    "age".to_string()
                ]
            ))
        );
    }

    #[test]
    fn a_form_without_a_data_class_or_a_typed_builder_maps_nothing() {
        let untyped = "<?php\nclass T {\n    public function buildForm($builder, array $options): void { $builder->add('email'); }\n    public function configureOptions($resolver): void { $resolver->setDefault('data_class', User::class); }\n}\n";
        assert_eq!(fields_of(untyped), None);
        let no_class = "<?php\nclass T {\n    public function buildForm(FormBuilderInterface $builder, array $options): void { $builder->add('email'); }\n    public function configureOptions($resolver): void { $resolver->setDefaults(['data_class' => null]); }\n}\n";
        assert_eq!(fields_of(no_class), None);
    }
}
