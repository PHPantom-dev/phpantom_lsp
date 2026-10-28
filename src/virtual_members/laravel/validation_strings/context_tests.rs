use super::*;

fn texts<'a>(content: &'a str, sites: &[ValidationSite]) -> Vec<&'a str> {
    sites
        .iter()
        .map(|site| &content[site.start..site.end])
        .collect()
}

fn cursor_site(source: &str) -> Option<(String, ValidationSite)> {
    let cursor = source.find("<cursor>").unwrap();
    let content = source.replace("<cursor>", "");
    let code = crate::completion::source::code_context::code_context_at(&content, cursor)?;
    let site = at_cursor(&content, cursor, &code)?;
    Some((content, site))
}

#[test]
fn request_rule_values_share_their_field_declarations() {
    let content = "<?php $request->validate(['email' => 'required|email', 'password' => ['required', 'same:confirmation'], 'confirmation' => 'string']);";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        ["required|email", "required", "same:confirmation", "string"]
    );
    assert!(
        sites
            .iter()
            .all(|site| site.kind == SiteKind::Rule && site.fields_complete)
    );
    assert_eq!(sites[0].field, Some(0));
    assert_eq!(sites[2].field, Some(1));
    assert_eq!(sites[0].fields[2].name, "confirmation");
    assert_eq!(
        &content[sites[0].fields[0].start..sites[0].fields[0].end],
        "email"
    );
    assert_eq!(
        &content[sites[0].fields[0].value_start..sites[0].fields[0].value_end],
        "'required|email'"
    );
    assert!(Arc::ptr_eq(&sites[0].fields, &sites[3].fields));
    assert!(Arc::ptr_eq(&sites[0].owner, &sites[3].owner));
    assert!(!sites[0].array_rule);
    assert!(sites[1].array_rule);
    let ValidationOwner::Method {
        receiver_start,
        receiver_end,
        method,
        rules_argument,
    } = sites[0].owner.as_ref()
    else {
        panic!("expected instance receiver")
    };
    assert_eq!(&content[*receiver_start..*receiver_end], "$request");
    assert_eq!(method, "validate");
    assert_eq!(*rules_argument, Some(0));
}

#[test]
fn make_records_the_written_receiver_for_later_type_confirmation() {
    let content = "<?php Checks::make(data: ['not_a_rule' => 'ignored'], rules: ['name' => 'required']); Unrelated::make([], ['name' => 'string']); $class::make([], ['name' => 'no_candidate']); Checks::$method([], ['name' => 'no_candidate']); Checks::other([], ['name' => 'no_candidate']);";
    let sites = collect(content);
    assert_eq!(texts(content, &sites), ["required", "string"]);
    let ValidationOwner::Static {
        class,
        offset,
        method,
    } = sites[0].owner.as_ref()
    else {
        panic!("expected static receiver")
    };
    assert_eq!(class, "Checks");
    assert_eq!(method, "make");
    assert_eq!(
        &content[*offset as usize..*offset as usize + class.len()],
        "Checks"
    );
}

#[test]
fn method_arguments_keep_positional_roles_and_named_rules_are_unique() {
    let content = r"<?php
        $request?->VALIDATE(rules: ['one' => 'string'], messages: ['one.string' => 'ignored']);
        $this->validate($request, ['two' => 'email']);
        $request->validateWithBag('bag', ['three' => 'required']);
        $this->validateWithBag('bag', $request, ['four' => 'integer']);
        $request->{$method}(['name' => 'ignored']);
        $request->validate(...['name' => 'ignored']);
        $request->validate(messages: ['name' => 'ignored']);
        $request->other(['name' => 'ignored']);
    ";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        ["string", "email", "required", "integer"]
    );
    let positions: Vec<_> = sites
        .iter()
        .map(|site| match site.owner.as_ref() {
            ValidationOwner::Method { rules_argument, .. } => *rules_argument,
            _ => panic!("expected method"),
        })
        .collect();
    assert_eq!(positions, [None, Some(1), Some(1), Some(2)]);
}

#[test]
fn sometimes_accepts_scalar_and_array_rules_and_attribute_names() {
    let content = r"<?php
        $validator->sometimes('email', 'required|email', fn () => true);
        $validator->sometimes(['age', 'years'], ['integer', 'min:18'], fn () => true);
        $validator->sometimes(rules: 'required', attribute: $field, callback: fn () => true);
        $validator->sometimes(rules: 'ignored');
    ";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        ["required|email", "integer", "min:18", "required"]
    );
    assert_eq!(sites[0].fields[0].name, "email");
    assert_eq!(sites[1].fields[1].name, "years");
    assert!(sites[1].array_rule);
    assert!(sites[3].fields.is_empty());
    assert_eq!(sites[3].field, None);
    assert!(sites.iter().all(|site| !site.fields_complete));
}

#[test]
fn only_arrays_returned_by_rules_messages_and_attributes_are_candidates() {
    let content = r"<?php
        class Request {
            public function rules() {
                $unreturned = ['ignored' => 'unreturned'];
                $closure = function () { return ['ignored' => 'closure']; };
                $arrow = fn () => ['ignored' => 'arrow'];
                function helper() { return ['ignored' => 'function']; }
                $object = new class { public function unrelated() { return ['ignored' => 'anonymous']; } };
                if ($condition) { return ['email' => 'email']; }
                return (array('name' => ('string'), 'count' => array('integer')));
            }
            public function messages() { return ['email.email' => 'Not an email', 'name.string' => 'Not text']; }
            public function attributes() { return ['email' => 'E-mail address']; }
            public function unrelated() { return ['ignored' => 'unrelated']; }
            public function emptyMethod() { return; }
        }
        abstract class AbstractRequest { abstract public function rules(); }
    ";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        [
            "email",
            "string",
            "integer",
            "email.email",
            "name.string",
            "email"
        ]
    );
    assert_eq!(sites[3].kind, SiteKind::MessageKey);
    assert_eq!(sites[5].kind, SiteKind::AttributeKey);
    let ValidationOwner::RulesMethod { offset, method } = sites[0].owner.as_ref() else {
        panic!("expected method declaration")
    };
    assert_eq!(method, "rules");
    assert_eq!(
        &content[*offset as usize..*offset as usize + method.len()],
        "rules"
    );
}

#[test]
fn unknown_rule_array_parts_keep_the_field_set_open() {
    let content = r"<?php class Request {
        public function rules() {
            return array_merge(parent::rules(), ['email' => 'email', $field => 'required', ...$more, 'positional']);
        }
        public function messages() { return ['email.email' => 'message', 'ignored', $key => 'computed']; }
        public function attributes() { return; }
    }";
    let sites = collect(content);
    assert_eq!(texts(content, &sites), ["email", "required", "email.email"]);
    assert!(!sites[0].fields_complete);
    assert_eq!(sites[1].field, None);
    assert_eq!(sites[0].fields.len(), 1);
}

#[test]
fn array_merge_combines_literal_siblings_but_other_calls_do_not() {
    let content = r"<?php class Request {
        public function rules() { return \array_merge(['email' => 'email'], array('name' => 'string')); }
        public function messages() { return something(['ignored' => 'message']); }
    }";
    let sites = collect(content);
    assert_eq!(texts(content, &sites), ["email", "string"]);
    assert!(sites.iter().all(|site| site.fields_complete));
    assert_eq!(sites[0].fields.len(), 2);
}

#[test]
fn rule_objects_keep_in_values_separate_from_rule_names() {
    let content = r"<?php $request->validate([
        'status' => ['required', Rules::in(['draft', 'published', $dynamic]), Rules::in('archived', values: 'review')],
        'other' => [Rules::enum(Status::class), new CustomRule(), $rule, ['nested']],
        'computed' => ['named' => 'email', ...$more],
        'dynamic' => [$class::in(['ignored']), Rules::$method(['ignored'])],
    ]);";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        [
            "required",
            "draft",
            "published",
            "archived",
            "review",
            "email"
        ]
    );
    assert!(
        sites[1..5]
            .iter()
            .all(|site| site.kind == SiteKind::InValue)
    );
    assert_eq!(sites[5].kind, SiteKind::Rule);
    let (class, offset) = sites[1].in_rule_class.as_ref().unwrap();
    assert_eq!(class, "Rules");
    assert_eq!(
        &content[*offset as usize..*offset as usize + class.len()],
        "Rules"
    );
    assert_eq!(sites[1].field, Some(0));
}

#[test]
fn comments_and_interpolated_or_computed_strings_are_not_rules() {
    let content = r#"<?php
        // $request->validate(['comment' => 'ignored']);
        /* Validator::make([], ['comment' => 'ignored']); */
        $request->validate(['name' => "required|$rule", 'other' => 'required' . '|email']);
        $request->validate($rules);
    "#;
    assert!(collect(content).is_empty());
}

#[test]
fn tokens_keep_rule_names_parameter_ranges_and_empty_segments() {
    let content = "<?php $request->validate(['email' => ' required | email:rfc,dns |']);";
    let sites = collect(content);
    let tokens = rule_tokens(&sites[0], content);
    assert_eq!(tokens.len(), 3);
    assert_eq!(&content[tokens[0].start..tokens[0].name_end], "required");
    assert_eq!(tokens[0].parameters_start, None);
    assert_eq!(&content[tokens[1].start..tokens[1].name_end], "email");
    assert_eq!(
        &content[tokens[1].parameters_start.unwrap()..tokens[1].end],
        "rfc,dns "
    );
    assert_eq!(tokens[2].start, tokens[2].end);
    assert_eq!(
        token_at(&sites[0], content, tokens[1].start),
        Some(tokens[1])
    );
    assert_eq!(token_at(&sites[0], content, sites[0].start - 1), None);
}

#[test]
fn escaped_rule_tokens_use_runtime_delimiters_and_source_ranges() {
    let content = r#"<?php $r->validate(["na\x6de" => "requ\x69red\174in\x3a\u{1F600},é", 'raw' => 'requ\x69red']);"#;
    let sites = collect(content);
    assert_eq!(sites[0].fields[0].name, "name");
    let tokens = rule_tokens(&sites[0], content);
    assert_eq!(tokens.len(), 2);
    assert_eq!(&content[tokens[0].start..tokens[0].end], r"requ\x69red");
    assert_eq!(
        literal_value(&sites[0], content, tokens[0].start, tokens[0].name_end).unwrap(),
        "required"
    );
    assert_eq!(&content[tokens[1].start..tokens[1].name_end], "in");
    assert_eq!(
        literal_value(
            &sites[0],
            content,
            tokens[1].parameters_start.unwrap(),
            tokens[1].end
        )
        .unwrap(),
        "😀,é"
    );
    let params = decoded_literal(
        &sites[0],
        content,
        tokens[1].parameters_start.unwrap(),
        tokens[1].end,
    )
    .unwrap();
    assert_eq!(
        &content[params.source_offset(0)..params.source_offset(4)],
        r"\u{1F600}"
    );
    let raw = rule_tokens(&sites[1], content);
    assert_eq!(
        literal_value(&sites[1], content, raw[0].start, raw[0].name_end).unwrap(),
        r"requ\x69red"
    );
}

#[test]
fn literal_source_mapping_covers_php_escape_widths_without_allocating_plain_text() {
    let content =
        r#"<?php $r->validate(['value' => "\\\"\$\n\r\t\v\e\f\x4A\x4\xZ\101\7\u{20AC}\zé"]);"#;
    let site = collect(content).remove(0);
    let decoded = decoded_literal(&site, content, site.start, site.end).unwrap();
    assert_eq!(decoded.value, "\\\"$\n\r\t\x0b\x1b\x0cJ\x04\\xZA\x07€\\zé");
    assert_eq!(
        decoded.offsets.as_ref().unwrap().len(),
        decoded.value.len() + 1
    );
    assert_eq!(decoded.source_offset(decoded.value.len()), site.end);
    let content = "<?php $r->validate(['value' => 'éplain']);";
    let site = collect(content).remove(0);
    let decoded = decoded_literal(&site, content, site.start, site.end).unwrap();
    assert!(matches!(decoded.value, Cow::Borrowed("éplain")));
    assert!(decoded.offsets.is_none());
    assert_eq!(decoded.source_offset(2), site.start + 2);
    assert_eq!(escape_width(b"\\", 0, false), (1, 1));
    assert_eq!(escape_width(b"\\'", 0, false), (2, 1));
    assert_eq!(escape_width(b"\\\\", 0, false), (2, 1));
    assert_eq!(escape_width(b"\\n", 0, false), (2, 2));
    assert_eq!(escape_width(b"\\u", 0, true), (2, 2));
    assert_eq!(escape_width(b"\\777", 0, true), (4, 1));
}

#[test]
fn invalid_decoded_literals_do_not_produce_rule_names() {
    let mut site = collect("<?php $r->validate(['name' => 'required']);").remove(0);
    let content = r#""\u{D800}""#;
    site.start = 1;
    site.end = content.len() - 1;
    assert!(rule_tokens(&site, content).is_empty());
    assert!(literal_value(&site, content, 0, content.len() + 1).is_none());
}

#[test]
fn associative_rule_lists_and_in_arrays_use_values_not_labels() {
    let content = "<?php $r->validate(['name' => ['label' => 'required', 5 => 'string'], 'status' => [Rule::in(['label' => 'pending', 4 => 'done'])]]);";
    let sites = collect(content);
    assert_eq!(
        texts(content, &sites),
        ["required", "string", "pending", "done"]
    );
    assert!(sites[..2].iter().all(|site| site.array_rule));
    assert!(sites[2..].iter().all(|site| site.kind == SiteKind::InValue));
    assert_eq!(sites[2].field, Some(1));
}

#[test]
fn later_literal_field_assignments_replace_overwritten_rule_values() {
    for content in [
        "<?php $r->validate(['name' => 'not_a_rule', 'other' => 'string', 'name' => 'required']);",
        "<?php $r->validate(array_merge(['name' => 'not_a_rule', 'other' => 'string'], ['name' => 'required']));",
        r#"<?php $r->validate(['name' => 'not_a_rule', 'other' => 'string', "na\x6de" => 'required']);"#,
    ] {
        let sites = collect(content);
        assert_eq!(texts(content, &sites), ["string", "required"], "{content}");
        assert_eq!(sites[0].fields.len(), 2);
        assert_eq!(sites[0].fields[0].name, "other");
        assert_eq!(sites[1].fields[sites[1].field.unwrap()].name, "name");
    }
}

#[test]
fn unfinished_enum_values_reuse_rules_from_the_repaired_program() {
    for source in [
        r"<?php namespace App; use Illuminate\Validation\Rule as Check; use Domain\State; $r->validate(['state' => [Check::enum(State::class), Check::in(['pe<cursor>",
        r"<?php namespace App; use Illuminate\Validation\Rule as Check; use Domain\State; $r->validate(['state' => [Check::enum(State::class), 'in:pe<cursor>",
    ] {
        let (_, site) = cursor_site(source).expect(source);
        let rules = site.recovered_rules.expect("enum metadata survives repair");
        assert_eq!(
            rules.entries[0].enum_class.as_deref(),
            Some("Domain\\State")
        );
        assert_eq!(rules.entries[0].key, "state");
        assert_eq!(
            rules.entries[0].key_start,
            source.find("'state'").unwrap() + 1
        );
    }
}

#[test]
fn pipes_in_regex_parameters_require_array_rules_as_in_laravel() {
    let content = "<?php $r->validate(['field' => 'regex:/a|b/|required']);";
    let sites = collect(content);
    let tokens = rule_tokens(&sites[0], content);
    let names: Vec<_> = tokens
        .iter()
        .map(|token| &content[token.start..token.name_end])
        .collect();
    assert_eq!(names, ["regex", "b/", "required"]);
    assert_eq!(
        &content[tokens[0].parameters_start.unwrap()..tokens[0].end],
        "/a"
    );
    let content = "<?php $r->validate(['field' => ['regex:/a|b/', 'required|email']]);";
    let sites = collect(content);
    let regex = rule_tokens(&sites[0], content);
    assert_eq!(regex.len(), 1);
    assert_eq!(
        &content[regex[0].parameters_start.unwrap()..regex[0].end],
        "/a|b/"
    );
    let combined = rule_tokens(&sites[1], content);
    assert_eq!(combined.len(), 1);
    assert_eq!(
        &content[combined[0].start..combined[0].name_end],
        "required|email"
    );
}

#[test]
fn empty_literals_are_completion_sites_in_every_supported_role() {
    for source in [
        "<?php $request->validate(['email' => '<cursor>']);",
        "<?php $request->validate(['email' => ['<cursor>']]);",
        "<?php $request->validate(['status' => [Rule::in(['<cursor>'])]]);",
        "<?php class Request { public function messages() { return ['<cursor>' => 'Message']; } }",
        "<?php class Request { public function attributes() { return ['<cursor>' => 'Name']; } }",
    ] {
        let (content, site) = cursor_site(source).expect(source);
        assert_eq!(site.start, site.end);
        assert_eq!(&content[site.start..site.end], "");
    }
    let content = "<?php $request->validate(['' => '', 'email' => 'required']);";
    let sites = collect(content);
    assert_eq!(sites.len(), 2);
    assert!(sites.iter().all(|site| !site.fields_complete));
    assert_eq!(sites[0].field, None);
    assert_eq!(sites[1].fields.len(), 1);
    assert_eq!(sites[1].fields[0].name, "email");
}

#[test]
fn empty_and_pipe_tail_rule_names_have_cursor_tokens() {
    for source in [
        "<?php $r->validate(['field' => '<cursor>']);",
        "<?php $r->validate(['field' => 'required|<cursor>']);",
        "<?php $r->validate(['field' => 'required||<cursor>']);",
        "<?php $r->validate(['field' => '<cursor>",
        "<?php $r->validate(['field' => 'required|<cursor>",
    ] {
        let (content, site) = cursor_site(source).expect(source);
        let cursor = source.find("<cursor>").unwrap();
        let token = token_at(&site, &content, cursor).expect(source);
        assert_eq!(token.start, cursor);
        assert_eq!(token.name_end, cursor);
        assert_eq!(token.end, cursor);
        assert_eq!(token.parameters_start, None);
    }
}

#[test]
fn cursor_lookup_uses_existing_complete_literals() {
    let (content, site) =
        cursor_site("<?php $request->validate(['email' => 'required|em<cursor>ail']);").unwrap();
    assert_eq!(&content[site.start..site.end], "required|email");
    assert!(site.fields_complete);
}

#[test]
fn cursor_recovery_closes_calls_arrays_and_method_bodies() {
    for source in [
        "<?php $request->validate(['email' => 'required|em<cursor>",
        "<?php class Request { public function rules() { return ['email' => ['em<cursor>",
        "<?php Validator::make([], ['email' => \"em<cursor>",
        "<?php class Request { public function messages() { return ['email.em<cursor>",
    ] {
        let (content, site) = cursor_site(source).expect(source);
        assert_eq!(site.end, content.len());
        assert!(content[site.start..site.end].ends_with("em"));
    }
}

#[test]
fn cursor_outside_a_rule_literal_does_not_create_a_candidate() {
    for source in [
        "<?php $request->validate(['em<cursor>ail' => 'required']);",
        "<?php $request->validate(['email' => ['em<cursor>ail' /* label */ => 'required']]);",
        "<?php $request->other(['field' => 'em<cursor>",
        "<?php $request->validate(<cursor>[]);",
        "<?php // $request->validate(['field' => 'em<cursor>",
    ] {
        assert!(cursor_site(source).is_none(), "{source}");
    }
}
