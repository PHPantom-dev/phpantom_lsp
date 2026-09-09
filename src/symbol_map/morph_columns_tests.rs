use super::*;

fn candidates(content: &str) -> Vec<MorphColumnSite> {
    fn collect(node: Node<'_, '_>, content: &str, out: &mut Vec<MorphColumnSite>) {
        match node {
            Node::Call(call) => record_morph_column_call(call, content, out),
            Node::Binary(binary) => record_morph_column_comparison(binary, content, out),
            _ => {}
        }
        node.visit_children(|child| collect(child, content, out));
    }
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"morph_columns.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut out = Vec::new();
    collect(Node::Program(program), content, &mut out);
    out
}

#[test]
fn symbol_map_retains_sorted_candidates_once_per_chain_link() {
    let content = "<?php $query->where('first_type', 'post')->where('second_type', 'video'); if ('image' === $comment->third_type) {}";
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"morph_columns.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let map = super::super::extract_symbol_map(program, content);

    assert_eq!(
        map.morph_column_sites
            .iter()
            .map(|site| site.key.as_str())
            .collect::<Vec<_>>(),
        ["post", "video", "image"]
    );
    assert!(
        map.morph_column_sites
            .windows(2)
            .all(|pair| pair[0].start < pair[1].start)
    );
}

#[test]
fn query_candidates_keep_exact_receiver_column_and_literal_ranges() {
    for (call, receiver, kind) in [
        ("$query->where", "$query", MorphColumnReceiver::Query),
        ("$query?->orWhere", "$query", MorphColumnReceiver::Query),
        (
            "Comment::whereNot",
            "Comment",
            MorphColumnReceiver::StaticQuery,
        ),
        (
            "static::orWhereNot",
            "static",
            MorphColumnReceiver::StaticQuery,
        ),
        ("$query->WhErE", "$query", MorphColumnReceiver::Query),
        (
            "$query->active()->where",
            "$query->active()",
            MorphColumnReceiver::Query,
        ),
        ("($query)->where", "($query)", MorphColumnReceiver::Query),
        (
            "Comment::query()->where",
            "Comment::query()",
            MorphColumnReceiver::Query,
        ),
        ("self::where", "self", MorphColumnReceiver::StaticQuery),
        ("parent::where", "parent", MorphColumnReceiver::StaticQuery),
        (
            "$modelClass::where",
            "$modelClass",
            MorphColumnReceiver::StaticQuery,
        ),
    ] {
        let content = format!("<?php {call}('comments.subject_kind', 'póst');");
        let sites = candidates(&content);
        assert_eq!(sites.len(), 1, "{content}");
        let site = &sites[0];
        assert_eq!(site.key, "póst");
        assert_eq!(site.column.as_str(), "comments.subject_kind");
        assert_eq!(site.receiver_kind, kind);
        assert_eq!(&content[site.start as usize..site.end as usize], "póst");
        assert_eq!(
            &content[site.receiver_start as usize..site.receiver_end as usize],
            receiver
        );
    }
}

#[test]
fn scalar_query_arguments_bind_named_and_positional_equalities() {
    for arguments in [
        "'subject_type', '=', 'post'",
        "'subject_type', '!=', 'post', 'or'",
        "'subject_type', '<>', 'post'",
        "column: 'subject_type', operator: 'post'",
        "operator: 'post', column: 'subject_type'",
        "value: 'post', column: 'subject_type', operator: '='",
        "'subject_type', value: 'post', operator: '!='",
        "operator: '=', boolean: 'or', value: 'post', column: 'subject_type'",
        "('subject_type'), ('='), ('post')",
    ] {
        let content = format!("<?php $query->where({arguments});");
        let sites = candidates(&content);
        assert_eq!(sites.len(), 1, "{content}");
        assert_eq!(sites[0].key, "post");
    }
}

#[test]
fn negated_query_wrappers_do_not_treat_operators_as_two_argument_aliases() {
    for operator in [
        "=",
        "!=",
        "<>",
        "<=>",
        ">",
        "<=",
        "like",
        "LIKE",
        "not like",
        "is",
        "is not",
        "&",
        "|",
        "^",
        "<<",
        ">>",
        "&~",
        "~",
        "~*",
        "!~",
        "!~*",
        "similar to",
        "not similar to",
        "not ilike",
        "~~*",
        "!~~*",
        "like binary",
        "rlike",
        "not rlike",
        "regexp",
        "not regexp",
        "ilike",
        "<",
        ">=",
    ] {
        for method in ["whereNot", "orWhereNot", "WhErEnOt"] {
            for arguments in [
                format!("'subject_type', '{operator}'"),
                format!("operator: '{operator}', column: 'subject_type'"),
            ] {
                let content = format!("<?php $query->{method}({arguments});");
                assert!(candidates(&content).is_empty(), "{content}");
            }
        }
    }
}

#[test]
fn ordinary_queries_treat_two_argument_operator_strings_as_alias_values() {
    for method in ["where", "orWhere"] {
        for alias in ["=", "!=", "LIKE", "is not", "&", "not similar to"] {
            let content = format!("<?php $query->{method}('subject_type', '{alias}');");
            let sites = candidates(&content);
            assert_eq!(sites.len(), 1, "{content}");
            assert_eq!(sites[0].key, alias);
        }
    }
}

#[test]
fn negated_queries_keep_operator_words_when_an_explicit_operator_precedes_them() {
    for method in ["whereNot", "orWhereNot"] {
        for (operator, alias) in [("=", "LIKE"), ("!=", "is not"), ("<>", "=")] {
            let content =
                format!("<?php $query->{method}('subject_type', '{operator}', '{alias}');");
            let sites = candidates(&content);
            assert_eq!(sites.len(), 1, "{content}");
            assert_eq!(sites[0].key, alias);
        }
    }
}

#[test]
fn query_method_variants_preserve_static_and_nullsafe_receivers() {
    for (receiver, call_operator, kind) in [
        ("$query", "->", MorphColumnReceiver::Query),
        ("$query", "?->", MorphColumnReceiver::Query),
        ("Comment", "::", MorphColumnReceiver::StaticQuery),
    ] {
        for method in ["where", "whereNot", "orWhere", "orWhereNot"] {
            for arguments in [
                "column: 'subject_type', operator: 'post'",
                "value: 'post', operator: '=', column: 'subject_type'",
            ] {
                let content = format!("<?php {receiver}{call_operator}{method}({arguments});");
                let sites = candidates(&content);
                assert_eq!(sites.len(), 1, "{content}");
                assert_eq!(sites[0].key, "post");
                assert_eq!(sites[0].receiver_kind, kind);
            }
        }
        for method in ["whereIn", "whereNotIn", "orWhereIn", "orWhereNotIn"] {
            let content = format!(
                "<?php {receiver}{call_operator}{method}(values: ['post'], column: 'subject_type');"
            );
            let sites = candidates(&content);
            assert_eq!(sites.len(), 1, "{content}");
            assert_eq!(sites[0].key, "post");
            assert_eq!(sites[0].receiver_kind, kind);
        }
    }
}

#[test]
fn set_membership_queries_collect_literal_values_independently_of_array_keys() {
    for (method, arguments) in [
        ("whereIn", "'subject_type', ['post', 'video'], 'and', false"),
        ("orWhereIn", "'subject_type', array('post', 'video')"),
        (
            "whereNotIn",
            "values: ['post', 'video'], column: 'subject_type', boolean: 'or'",
        ),
        ("orWhereNotIn", "'subject_type', (['post', 'video'])"),
        (
            "whereIn",
            "'subject_type', ['post', 'skip' => 'video', ...$types, $dynamic, 7, ['nested']]",
        ),
        (
            "whereIn",
            "'subject_type', ['post', 'App\\Models\\Post', 'video']",
        ),
        (
            "whereIn",
            "'subject_type', [42 => 'post', $key => 'video', 'dynamic' => $alias, 'nested' => ['hidden']]",
        ),
        (
            "orWhereNotIn",
            "'subject_type', array('first' => 'post', 'second' => 'video')",
        ),
    ] {
        let content = format!("<?php $query->{method}({arguments});");
        let sites = candidates(&content);
        assert_eq!(
            sites
                .iter()
                .map(|site| site.key.as_str())
                .collect::<Vec<_>>(),
            ["post", "video"],
            "{content}"
        );
    }
}

#[test]
fn property_comparisons_accept_equality_and_either_operand_order() {
    for operator in ["==", "!=", "===", "!==", "<>"] {
        for comparison in [
            format!("$comment->subject_type {operator} 'post'"),
            format!("'post' {operator} $comment?->subject_type"),
            format!("(($comment->subject_type)) {operator} (('post'))"),
        ] {
            let content = format!("<?php {comparison};");
            let sites = candidates(&content);
            assert_eq!(sites.len(), 1, "{content}");
            assert_eq!(sites[0].key, "post");
            assert_eq!(sites[0].column.as_str(), "subject_type");
            assert_eq!(sites[0].receiver_kind, MorphColumnReceiver::Model);
            assert_eq!(
                &content[sites[0].receiver_start as usize..sites[0].receiver_end as usize],
                "$comment"
            );
        }
    }
}

#[test]
fn property_candidates_preserve_the_full_receiver_expression() {
    for receiver in [
        "($comment)",
        "$comment->fresh()",
        "$comments[0]",
        "$this->comment",
    ] {
        let content = format!("<?php 'post' === {receiver}?->subject_type;");
        let sites = candidates(&content);
        assert_eq!(sites.len(), 1, "{content}");
        assert_eq!(
            &content[sites[0].receiver_start as usize..sites[0].receiver_end as usize],
            receiver
        );
    }
}

#[test]
fn asterisk_is_a_literal_alias_in_column_comparisons() {
    for expression in [
        "$query->where('subject_type', '*')",
        "$query->where('subject_type', '!=', '*')",
        "$query->whereIn('subject_type', ['*'])",
        "$query->whereNotIn('subject_type', ['alias' => '*'])",
        "$comment->subject_type === '*'",
        "'*' !== $comment?->subject_type",
    ] {
        let content = format!("<?php {expression};");
        let sites = candidates(&content);
        assert_eq!(sites.len(), 1, "{content}");
        assert_eq!(sites[0].key, "*");
    }
}

#[test]
fn empty_literals_are_retained_for_completion() {
    let content = "<?php $query->where('subject_type', ''); $comment->subject_type === ''; $query->whereIn('subject_type', [b'', B\"\"]);";
    let sites = candidates(content);
    assert_eq!(sites.len(), 4);
    for site in sites {
        assert_eq!(site.key, "");
        assert_eq!(site.start, site.end);
    }
}

#[test]
fn binary_string_prefixes_do_not_enter_aliases_or_replacement_ranges() {
    for expression in [
        "$query->where(b'subject_type', b'post')",
        "$query->where(B\"subject_type\", B\"post\")",
        "$comment->subject_type === (b'post')",
        "$query->where((B'subject_type'), (b'='), (B'post'))",
        "$query->whereIn(B'subject_type', [B'post'])",
    ] {
        let content = format!("<?php {expression};");
        let sites = candidates(&content);
        assert_eq!(sites.len(), 1, "{content}");
        assert_eq!(sites[0].key, "post");
        assert_eq!(sites[0].column.as_str(), "subject_type");
        assert_eq!(
            &content[sites[0].start as usize..sites[0].end as usize],
            "post"
        );
    }
}

#[test]
fn confirmed_alias_spans_preserve_literal_ranges_and_reference_semantics() {
    let content = "<?php $query->where('subject_type', B\"póst\");";
    let sites = candidates(content);
    assert_eq!(sites.len(), 1);
    let span = sites[0].to_span();
    assert_eq!(&content[span.start as usize..span.end as usize], "póst");
    let super::super::SymbolKind::LaravelStringKey {
        key,
        kind,
        is_write,
        is_optional,
    } = span.kind
    else {
        panic!("expected a Laravel alias reference");
    };
    assert_eq!(key, "póst");
    assert_eq!(kind, super::super::LaravelStringKind::MorphAlias);
    assert!(!is_write);
    assert!(!is_optional);
}

#[test]
fn unsupported_or_dynamic_query_forms_are_not_candidates() {
    for expression in [
        "where('subject_type', 'post')",
        "$query->having('subject_type', 'post')",
        "$query->whereColumn('subject_type', 'post')",
        "$query->whereRaw('subject_type = post')",
        "$query->$method('subject_type', 'post')",
        "$query->{'where'}('subject_type', 'post')",
        "$query?->$method('subject_type', 'post')",
        "Comment::$method('subject_type', 'post')",
        "$query->where()",
        "$query->where('subject_type')",
        "$query->where('', 'post')",
        "$query->where($column, 'post')",
        "$query->where('subject_' . 'type', 'post')",
        "$query->where('subject_type', $alias)",
        "$query->whereNot('subject_type', $alias)",
        "$query->orWhereNot('subject_type', null)",
        "$query->where('subject_type', \"$alias\")",
        "$query->where('subject_type', 'po' . 'st')",
        "$query->where('subject_type', 'App\\Models\\Post')",
        "$query->where('subject_type', 'like', 'post')",
        "$query->where('subject_type', '>', 'post')",
        "$query->where('subject_type', '==', 'post')",
        "$query->where('subject_type', '===', 'post')",
        "$query->where('subject_type', '!==', 'post')",
        "$query->where('subject_type', '<', 'post')",
        "$query->where('subject_type', '>=', 'post')",
        "$query->where('subject_type', '<=', 'post')",
        "$query->where('subject_type', 'not like', 'post')",
        "$query->where('subject_type', 'ilike', 'post')",
        "$query->where('subject_type', $operator, 'post')",
        "$query->where('subject_type', operator: '=', boolean: 'or')",
        "$query->where(column: 'subject_type', value: 'post')",
        "$query->where(Column: 'subject_type', operator: 'post')",
        "$query->where('subject_type', column: 'post')",
        "$query->where(column: 'subject_type', column: 'post')",
        "$query->where('subject_type', operator: '=', operator: 'post')",
        "$query->where(operator: 'post')",
        "$query->where('subject_type', alias: 'post')",
        "$query->where('subject_type', ...$arguments)",
        "$query->where(column: 'subject_type', 'post')",
        "$query->where(...$arguments)",
        "$query->where('subject_type', '=', 'post', 'and', false)",
        "$query->orWhere('subject_type', '=', 'post', 'or')",
        "$query->whereIn('subject_type', 'post')",
        "$query->whereIn('subject_type', $types)",
        "$query->whereIn('subject_type', [])",
        "$query->whereIn('subject_type', array())",
        "$query->whereIn('subject_type', values: ['post'], values: ['video'])",
        "$query->whereIn('subject_type', value: ['post'])",
        "$query->orWhereIn('subject_type', ['post'], boolean: 'or')",
        "$query->whereNotIn('subject_type', ['post'], not: false)",
    ] {
        let content = format!("<?php {expression};");
        assert!(candidates(&content).is_empty(), "{content}");
    }
}

#[test]
fn unsupported_or_dynamic_property_comparisons_are_not_candidates() {
    for comparison in [
        "$comment->subject_type > 'post'",
        "$comment->subject_type < 'post'",
        "$comment->subject_type >= 'post'",
        "$comment->subject_type <= 'post'",
        "$comment->subject_type <=> 'post'",
        "$comment->subject_type . 'post'",
        "$comment->subject_type === $alias",
        "$comment->subject_type === 'po' . 'st'",
        "$comment->subject_type === \"$alias\"",
        "$comment->subject_type === 'App\\Models\\Post'",
        "$comment->$column === 'post'",
        "$comment->{'subject_type'} === 'post'",
        "$comment['subject_type'] === 'post'",
        "Comment::$subject_type === 'post'",
        "Comment::SUBJECT_TYPE === 'post'",
        "'post' === 'video'",
        "$comment->subject_type === $other->subject_type",
    ] {
        let content = format!("<?php {comparison};");
        assert!(candidates(&content).is_empty(), "{content}");
    }
}

#[test]
fn literal_ranges_require_matching_source_content() {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"morph_columns.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, b"<?php 'post';");
    let statement = program
        .statements
        .iter()
        .find_map(|statement| match statement {
            Statement::Expression(statement) => Some(statement),
            _ => None,
        })
        .expect("expected string expression");
    assert!(literal_text(statement.expression, "").is_none());
    let mut sites = Vec::new();
    push_site(
        statement.expression,
        "subject_type",
        statement.expression,
        MorphColumnReceiver::Model,
        "",
        &mut sites,
    );
    assert!(sites.is_empty());
}
