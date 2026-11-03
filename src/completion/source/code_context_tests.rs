use super::*;

/// Scan up to the last occurrence of `needle`, as if the cursor sat there.
fn at<'a>(content: &'a str, needle: &str) -> Option<CodeContext<'a>> {
    let offset = content.rfind(needle).expect("needle not found");
    code_context_at(content, offset)
}

/// The byte the offset `pick` names in `content`.
fn byte_at(content: &str, pick: usize) -> u8 {
    content.as_bytes()[pick]
}

#[test]
fn open_brackets_name_the_enclosing_array_and_call() {
    let src = "<?php\nroute('users.show', ['user' => 1]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    let (bracket, paren) = ctx.nested_pair(b'[', b'(').expect("a `[` nested in a `(`");
    assert_eq!(byte_at(src, bracket), b'[');
    assert_eq!(byte_at(src, paren), b'(');
    assert_eq!(ctx.last_code_byte(), Some(b'['));
}

#[test]
fn a_line_comment_between_the_call_and_the_key_does_not_unbalance_the_scan() {
    let src = "<?php\nroute('users.show', [   // TODO: check (parameters)\n    'user' => 1,\n]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_some());
    assert_eq!(ctx.last_code_byte(), Some(b'['));
}

#[test]
fn block_and_hash_comments_are_skipped_too() {
    for src in [
        "<?php\nroute('r', [ /* don't ( */ 'user' => 1]);\n",
        "<?php\nroute('r', [ # don't (\n    'user' => 1]);\n",
    ] {
        let ctx = at(src, "'user'").unwrap_or_else(|| panic!("the offset is in code: {src}"));
        assert!(
            ctx.nested_pair(b'[', b'(').is_some(),
            "expected a `[` nested in a `(`: {src}"
        );
        assert_eq!(ctx.last_code_byte(), Some(b'['), "in {src}");
    }
}

#[test]
fn an_attribute_is_not_a_hash_comment() {
    let src = "<?php\n#[Route('/x')]\nfunction f() { $a = ['k' => 1]; }\n";
    let ctx = at(src, "'k'").expect("the offset is in code");
    let (_, brace) = ctx
        .nested_pair(b'[', b'{')
        .expect("the array is nested in the function body");
    assert_eq!(byte_at(src, brace), b'{');
}

#[test]
fn a_heredoc_body_cannot_unbalance_the_scan() {
    let src = "<?php\n$sql = <<<SQL\n  select ( from '\nSQL;\nroute('r', ['user' => 1]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_some());
}

#[test]
fn inline_html_before_the_open_tag_is_not_read_as_code() {
    let src = "<p>Don't (do) that</p>\n<?php route('r', ['user' => 1]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_some());
}

#[test]
fn a_close_tag_inside_a_line_comment_ends_the_php_block() {
    let src = "<?php echo 1; // done ?>\n<p>'x' (</p>\n";
    assert!(at(src, "'x'").is_none(), "the offset sits in inline HTML");
}

#[test]
fn an_offset_inside_a_comment_has_no_context() {
    assert!(at("<?php\n// route('r', ['user']);\n", "'user'").is_none());
}

/// Brackets and quotes inside a literal are text, so the context reported for
/// an offset inside one is the context of the literal itself.
#[test]
fn an_offset_inside_a_string_reports_the_literal() {
    let src = "<?php\n$s = 'route(\\'r\\', [';\n";
    let ctx = at(src, "[").expect("the offset is inside the literal");
    assert_eq!(ctx.open_string, Some((src.find('\'').unwrap(), '\'')));
    assert!(ctx.open_brackets.is_empty());
    assert!(ctx.code_before.ends_with('='), "got {:?}", ctx.code_before);
}

/// The key being typed is reported along with the brackets around it, so both
/// come out of the one scan.
#[test]
fn an_array_key_being_typed_names_its_own_quote() {
    let src = "<?php\nroute('users.show', ['us";
    let ctx = at(src, "us").expect("the offset is inside the key");
    assert_eq!(ctx.open_string, Some((src.rfind('\'').unwrap(), '\'')));
    assert!(ctx.nested_pair(b'[', b'(').is_some());
    assert_eq!(ctx.last_code_byte(), Some(b'['));
}

/// Quotes only pair up left to right, so the literal an offset is in is the one
/// left open by the scan and not the nearest quote behind it.
#[test]
fn open_string_is_the_literal_the_offset_is_in() {
    /// The literal the end of `content` sits in.
    fn open_string(content: &str) -> Option<(usize, char)> {
        code_context_at(content, content.len())?.open_string
    }

    // An apostrophe inside a double-quoted string is not its opener.
    let src = "$request->input(\"it's ";
    assert_eq!(open_string(src), Some((src.find('"').unwrap(), '"')));

    // An escaped quote does not close the literal.
    let src = "$request->input('it\\'s ";
    assert_eq!(open_string(src), Some((src.find('\'').unwrap(), '\'')));

    // A literal that already closed leaves the offset outside a string, on this
    // line and on an earlier one.
    assert_eq!(open_string("$a = 'x'; $request["), None);
    assert_eq!(open_string("$a = 'x';\n$request["), None);

    // An apostrophe in a comment earlier on the line is not an opener.
    let src = "Artisan::call('app:sync', [ /* don't ( */ '";
    assert_eq!(open_string(src), Some((src.len() - 1, '\'')));

    // A literal opened on an earlier line is still found.
    let src = "$request->input(\n    'na";
    assert_eq!(open_string(src), Some((src.rfind('\'').unwrap(), '\'')));

    // An offset in a comment is in no literal, apostrophe or not.
    assert_eq!(open_string("// don't "), None);
}

#[test]
fn code_before_stops_at_the_last_byte_of_code() {
    let src = "<?php\nroute('r', [ // note\n    'user'\n]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(
        ctx.code_before.ends_with('['),
        "trailing comment and whitespace should be cut off, got {:?}",
        ctx.code_before
    );
}

#[test]
fn a_key_of_a_nested_array_is_not_a_key_of_the_call() {
    let src = "<?php\nroute('r', ['a' => ['user' => 1]]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_none());
}

#[test]
fn a_fragment_with_no_open_tag_is_read_as_code() {
    let src = "route('users.show', ['user' => 1]);";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_some());
}

/// Each bracket counts its own commas, so the count on the call's paren is the
/// index of the argument the offset is in whatever nesting sits in between.
#[test]
fn commas_are_counted_per_bracket() {
    let src = "<?php\nfoo('a', bar(1, 2), ['b', 'user']);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("the call's paren");
    assert_eq!(byte_at(src, call.offset), b'(');
    assert_eq!(call.commas, 2, "the array is the third argument");
    let (array, _) = ctx.nested_pair(b'[', b'(').expect("the argument's array");
    assert_eq!(
        ctx.open_brackets.last().map(|b| b.commas),
        Some(1),
        "the second element of the array at {array}"
    );
}

/// A comma or a bracket inside a comment is not code, so neither moves the
/// enclosing call nor the argument index.
#[test]
fn a_comment_holds_neither_commas_nor_brackets() {
    let src = "<?php\nfoo('a' /* , ) [ */, 'user');\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("the call's paren");
    assert_eq!(call.offset, src.find('(').unwrap());
    assert_eq!(call.commas, 1);
}

/// The code before a bracket ends at code, so the callee is read without the
/// comment that sits between it and its argument list.
#[test]
fn a_bracket_records_where_the_code_before_it_ends() {
    let src = "<?php\n$q->orderBy /* asc */ ('user');\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("the call's paren");
    assert_eq!(&src[..call.code_before], "<?php\n$q->orderBy");
}

#[test]
fn an_unmatched_closer_leaves_the_brackets_that_do_pair_up() {
    let src = "<?php\n$x = [1, 2);\nroute('r', ['user' => 1]);\n";
    let ctx = at(src, "'user'").expect("the offset is in code");
    assert!(ctx.nested_pair(b'[', b'(').is_some());
}

#[test]
fn a_call_records_its_receivers_arrow_and_double_colon() {
    for (src, is_static) in [
        ("<?php\n$q->where('a');\n", false),
        ("<?php\n$q?->where('a');\n", false),
        ("<?php\nUser::where('a');\n", true),
    ] {
        let ctx = at(src, "'a'").expect("the offset is in code");
        let call = ctx.enclosing_paren().expect("the call's paren");
        let op = call.callee_operator.expect("the arrow or double colon");
        assert_eq!(op.is_static, is_static, "in {src}");
    }
}

/// The receiver's boundary comes from the operator's own `code_before`, cut
/// off at the last byte of code the same way a bracket's is, so a comment
/// between the receiver and the operator does not leak into it.
#[test]
fn the_operators_code_before_stops_before_a_comment() {
    let src = "<?php\n$q /* the query */ ->where('a');\n";
    let ctx = at(src, "'a'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("the call's paren");
    let op = call.callee_operator.expect("the arrow");
    assert_eq!(&src[..op.code_before], "<?php\n$q");
}

/// A plain function call has no receiver, so it gets no operator, even when
/// an unrelated one sits earlier in an argument list to another call.
#[test]
fn a_bare_call_has_no_callee_operator() {
    let src = "<?php\nfoo($a->bar, baz('a'));\n";
    let ctx = at(src, "'a'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("baz's call");
    assert!(call.callee_operator.is_none());
}

/// A second identifier after the operator (here the `bar` of `and bar`) is
/// not its callee, so the operator does not survive to credit `bar` with
/// `$a`'s receiver.
#[test]
fn a_word_after_the_operators_callee_clears_it() {
    let src = "<?php\nif ($a->foo and bar('a')) {}\n";
    let ctx = at(src, "'a'").expect("the offset is in code");
    let call = ctx.enclosing_paren().expect("bar's call");
    assert!(call.callee_operator.is_none());
}

#[test]
fn call_closing_paren_skips_comments_strings_and_documents() {
    use super::closing_call_paren;
    for source in [
        "f(') <?php', /* ) */ [g(')')]) trailing",
        "f(['body' => <<<TEXT\n)\nTEXT, 'name' => 1]) trailing",
        "f(['body' => <<<'TEXT'\n)\nTEXT, 'name' => 1]) trailing",
    ] {
        assert_eq!(closing_call_paren(source, 1), source.rfind(')'), "{source}");
    }
    for (source, open) in [("f(", 1), ("f(/*", 1), ("f", 0), ("f()", 99)] {
        assert!(closing_call_paren(source, open).is_none());
    }
}
