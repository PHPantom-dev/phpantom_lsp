use std::ops::ControlFlow;

use super::*;
use crate::virtual_members::laravel::helpers::extract_string_literal;

fn collect_strings(php: &str) -> Vec<String> {
    let mut found = Vec::new();
    walk_all_php_expressions(php, &mut |expr| {
        if let Some((s, _, _)) = extract_string_literal(expr, php) {
            found.push(s.to_string());
        }
        ControlFlow::Continue(())
    });
    found
}

fn has(php: &str, needle: &str) -> bool {
    collect_strings(php).iter().any(|s| s == needle)
}

// ── Statement branches ───────────────────────────────────────────────────────

#[test]
fn walker_return_stmt() {
    assert!(has("<?php return 'ret';", "ret"));
}

#[test]
fn walker_echo_stmt() {
    let php = "<?php echo 'ea', 'eb';";
    assert!(has(php, "ea"));
    assert!(has(php, "eb"));
}

#[test]
fn walker_namespace_stmt() {
    assert!(has("<?php namespace Foo; return 'ns';", "ns"));
}

#[test]
fn walker_block_stmt() {
    assert!(has("<?php { return 'blk'; }", "blk"));
}

#[test]
fn walker_if_stmt() {
    let php =
        "<?php if (true) { return 'th'; } elseif (false) { return 'ei'; } else { return 'el'; }";
    assert!(has(php, "th"));
    assert!(has(php, "ei"));
    assert!(has(php, "el"));
}

#[test]
fn walker_while_stmt() {
    assert!(has("<?php while (true) { return 'wb'; }", "wb"));
}

#[test]
fn walker_do_while_stmt() {
    assert!(has("<?php do { return 'dw'; } while (false);", "dw"));
}

#[test]
fn walker_for_stmt() {
    let php = "<?php for (foo('fi'); foo('fc'); foo('fu')) { foo('fb'); }";
    assert!(has(php, "fi"));
    assert!(has(php, "fc"));
    assert!(has(php, "fu"));
    assert!(has(php, "fb"));
}

#[test]
fn walker_foreach_stmt() {
    let php = "<?php foreach (['item'] as $v) { return 'fv'; }";
    assert!(has(php, "item"));
    assert!(has(php, "fv"));
}

#[test]
fn walker_try_catch_finally() {
    let php = "<?php try { return 'tv'; } catch (\\Exception $e) { return 'cv'; } finally { return 'fv'; }";
    assert!(has(php, "tv"));
    assert!(has(php, "cv"));
    assert!(has(php, "fv"));
}

#[test]
fn walker_switch_stmt() {
    let php = "<?php switch ('sw') { case 'cs': return 'rv'; default: return 'dv'; }";
    assert!(has(php, "sw"));
    assert!(has(php, "cs"));
    assert!(has(php, "rv"));
    assert!(has(php, "dv"));
}

#[test]
fn walker_function_body() {
    assert!(has("<?php function foo() { return 'fn'; }", "fn"));
}

#[test]
fn walker_class_method_body() {
    assert!(has("<?php class C { function m() { return 'cm'; } }", "cm"));
}

#[test]
fn walker_class_property() {
    assert!(has("<?php class C { public $x = 'pv'; }", "pv"));
}

#[test]
fn walker_class_constant() {
    assert!(has("<?php class C { const K = 'ck'; }", "ck"));
}

#[test]
fn walker_interface_constant() {
    assert!(has("<?php interface I { const K = 'ik'; }", "ik"));
}

#[test]
fn walker_trait_method_body() {
    assert!(has("<?php trait T { function m() { return 'tm'; } }", "tm"));
}

#[test]
fn walker_enum_backed_case() {
    assert!(has("<?php enum S: string { case A = 'ev'; }", "ev"));
}

#[test]
fn walker_enum_constant() {
    assert!(has("<?php enum S { const K = 'ec'; }", "ec"));
}

#[test]
fn walker_static_var() {
    assert!(has("<?php function f() { static $x = 'sv'; }", "sv"));
}

#[test]
fn walker_unset_stmt() {
    // unset args are variable expressions; verify the walker reaches code after it
    assert!(has("<?php unset($x); return 'au';", "au"));
}

// ── Expression branches ──────────────────────────────────────────────────────

#[test]
fn walker_method_call() {
    assert!(has("<?php $o->m('ma');", "ma"));
}

#[test]
fn walker_null_safe_method_call() {
    assert!(has("<?php $o?->m('na');", "na"));
}

#[test]
fn walker_binary_expr() {
    let php = "<?php return 'lhs' . 'rhs';";
    assert!(has(php, "lhs"));
    assert!(has(php, "rhs"));
}

#[test]
fn walker_unary_prefix() {
    assert!(has("<?php $x = !true; return 'up';", "up"));
}

#[test]
fn walker_unary_postfix() {
    assert!(has("<?php $i = 0; $i++; return 'upo';", "upo"));
}

#[test]
fn walker_parenthesized() {
    assert!(has("<?php return ('pv');", "pv"));
}

#[test]
fn walker_assignment() {
    assert!(has("<?php $x = 'av';", "av"));
}

#[test]
fn walker_conditional_ternary() {
    let php = "<?php return true ? 'th' : 'el';";
    assert!(has(php, "th"));
    assert!(has(php, "el"));
}

#[test]
fn walker_conditional_elvis() {
    let php = "<?php return 'cv' ?: 'ev';";
    assert!(has(php, "cv"));
    assert!(has(php, "ev"));
}

#[test]
fn walker_array_literal() {
    let php = "<?php return ['v1', 'k' => 'v2'];";
    assert!(has(php, "v1"));
    assert!(has(php, "k"));
    assert!(has(php, "v2"));
}

#[test]
fn walker_legacy_array() {
    assert!(has("<?php return array('la1', 'la2');", "la1"));
}

#[test]
fn walker_variadic_array_element() {
    assert!(has("<?php return [...$a, 'vae'];", "vae"));
}

#[test]
fn walker_array_access() {
    assert!(has("<?php return $a['ki'];", "ki"));
}

#[test]
fn walker_closure_body() {
    assert!(has("<?php $f = function() { return 'clv'; };", "clv"));
}

#[test]
fn walker_arrow_function() {
    assert!(has("<?php $f = fn() => 'afv';", "afv"));
}

#[test]
fn walker_match_expr() {
    let php = "<?php return match ('ms') { 'ma' => 'mv', default => 'md' };";
    assert!(has(php, "ms"));
    assert!(has(php, "ma"));
    assert!(has(php, "mv"));
    assert!(has(php, "md"));
}

#[test]
fn walker_throw_expr() {
    assert!(has("<?php throw new \\Exception('te');", "te"));
}

#[test]
fn walker_yield_value() {
    assert!(has("<?php function g() { yield 'yv'; }", "yv"));
}

#[test]
fn walker_yield_pair() {
    let php = "<?php function g() { yield 'yk' => 'yp'; }";
    assert!(has(php, "yk"));
    assert!(has(php, "yp"));
}

#[test]
fn walker_yield_from() {
    assert!(has("<?php function g() { yield from ['yf']; }", "yf"));
}

#[test]
fn walker_clone() {
    assert!(has("<?php $b = clone $a; return 'after';", "after"));
}

#[test]
fn walker_instantiation_args() {
    assert!(has("<?php new Foo('ca');", "ca"));
}

// ── ControlFlow early exit ───────────────────────────────────────────────────

#[test]
fn walker_early_exit_stops_after_break() {
    let php = "<?php foo('a'); foo('b'); foo('c');";
    let mut visited: Vec<String> = Vec::new();
    walk_all_php_expressions(php, &mut |expr| {
        if let Some((s, _, _)) = extract_string_literal(expr, php) {
            visited.push(s.to_string());
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    assert_eq!(visited, vec!["a".to_string()]);
}
