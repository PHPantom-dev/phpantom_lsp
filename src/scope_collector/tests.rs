//! Tests for the ScopeCollector infrastructure.

use super::*;
use crate::parser::with_parsed_program;

/// Helper: parse PHP code and collect scope from the first function body found.
fn collect_from_function(php: &str) -> ScopeMap {
    with_parsed_program(php, "test", |program, _content| {
        for stmt in program.statements.iter() {
            if let Statement::Function(func) = stmt {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                return collect_function_scope(
                    &func.parameter_list,
                    func.body.statements.as_slice(),
                    body_start,
                    body_end,
                );
            }
        }
        panic!("No function found in test PHP code");
    })
}

/// Helper: parse PHP code and collect scope from the first method body
/// found inside the first class.
fn collect_from_method(php: &str) -> ScopeMap {
    with_parsed_program(php, "test", |program, _content| {
        for stmt in program.statements.iter() {
            if let Statement::Class(class) = stmt {
                for member in class.members.iter() {
                    if let ClassLikeMember::Method(method) = member
                        && let MethodBody::Concrete(block) = &method.body
                    {
                        let body_start = block.left_brace.start.offset;
                        let body_end = block.right_brace.end.offset;
                        return collect_function_scope(
                            &method.parameter_list,
                            block.statements.as_slice(),
                            body_start,
                            body_end,
                        );
                    }
                }
            }
        }
        panic!("No class method found in test PHP code");
    })
}

/// Helper: find access by name and return all offsets + kinds.
fn accesses_for(scope_map: &ScopeMap, name: &str) -> Vec<(u32, AccessKind)> {
    scope_map
        .accesses
        .iter()
        .filter(|a| a.name == name)
        .map(|a| (a.offset, a.kind))
        .collect()
}

/// Helper: count accesses of a specific kind for a variable.
fn count_kind(scope_map: &ScopeMap, name: &str, kind: AccessKind) -> usize {
    scope_map
        .accesses
        .iter()
        .filter(|a| a.name == name && a.kind == kind)
        .count()
}

// ─── Basic variable tracking ────────────────────────────────────────────────

#[test]
fn simple_assignment_and_read() {
    let php = r#"<?php
function test() {
    $x = 1;
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
    assert_eq!(scope_map.accesses.len(), 2);
}

#[test]
fn parameter_is_write() {
    let php = r#"<?php
function test($a, $b) {
    return $a + $b;
}
"#;
    let scope_map = collect_from_function(php);

    // Parameters are writes.
    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Write), 1);
    // Return reads both.
    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Read), 1);
}

#[test]
fn multiple_assignments() {
    let php = r#"<?php
function test() {
    $x = 1;
    $x = 2;
    $x = 3;
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 3);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
}

#[test]
fn compound_assignment_is_read_write() {
    let php = r#"<?php
function test() {
    $x = 0;
    $x += 5;
    $x .= "hello";
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::ReadWrite), 2);
}

#[test]
fn postfix_increment_is_read_write() {
    let php = r#"<?php
function test() {
    $x = 0;
    $x++;
    $x--;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::ReadWrite), 2);
}

#[test]
fn coalesce_assignment_is_read_write() {
    let php = r#"<?php
function test() {
    $x = null;
    $x ??= "default";
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::ReadWrite), 1);
}

// ─── Control flow ───────────────────────────────────────────────────────────

#[test]
fn if_else_variables_leak() {
    let php = r#"<?php
function test($cond) {
    if ($cond) {
        $x = 1;
    } else {
        $x = 2;
    }
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    // $x is written in both branches and read after — it's visible.
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 2);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
    // $cond: 1 write (param) + 1 read (if condition).
    assert_eq!(count_kind(&scope_map, "$cond", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$cond", AccessKind::Read), 1);
}

#[test]
fn foreach_value_is_write() {
    let php = r#"<?php
function test($items) {
    foreach ($items as $key => $value) {
        echo $key;
        echo $value;
    }
}
"#;
    let scope_map = collect_from_function(php);

    // foreach key and value bindings are writes.
    assert_eq!(count_kind(&scope_map, "$key", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$value", AccessKind::Write), 1);
    // They are read inside the loop.
    assert_eq!(count_kind(&scope_map, "$key", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$value", AccessKind::Read), 1);
    // $items: 1 write (param) + 1 read (foreach expression).
    assert_eq!(count_kind(&scope_map, "$items", AccessKind::Read), 1);
}

#[test]
fn for_loop_variables() {
    let php = r#"<?php
function test() {
    for ($i = 0; $i < 10; $i++) {
        echo $i;
    }
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$i", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$i", AccessKind::ReadWrite), 1); // $i++
    assert_eq!(count_kind(&scope_map, "$i", AccessKind::Read), 2); // condition + body
}

#[test]
fn while_loop_variables() {
    let php = r#"<?php
function test() {
    $i = 0;
    while ($i < 10) {
        echo $i;
        $i++;
    }
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$i", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$i", AccessKind::ReadWrite), 1);
    assert_eq!(count_kind(&scope_map, "$i", AccessKind::Read), 2); // condition + body
}

#[test]
fn do_while_variables() {
    let php = r#"<?php
function test() {
    $x = 0;
    do {
        $x++;
    } while ($x < 10);
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::ReadWrite), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1); // condition
}

#[test]
fn switch_case_variables() {
    let php = r#"<?php
function test($val) {
    switch ($val) {
        case 1:
            $result = "one";
            break;
        case 2:
            $result = "two";
            break;
        default:
            $result = "other";
    }
    echo $result;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Write), 3);
    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$val", AccessKind::Read), 1);
}

#[test]
fn try_catch_finally() {
    let php = r#"<?php
function test() {
    try {
        $x = doSomething();
    } catch (\Exception $e) {
        echo $e;
    } finally {
        $y = cleanup();
    }
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    // $e is a write (catch binding) + read (echo).
    assert_eq!(count_kind(&scope_map, "$e", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$e", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$y", AccessKind::Write), 1);

    // Catch block creates a new frame.
    let catch_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Catch)
        .collect();
    assert_eq!(catch_frames.len(), 1);
}

// ─── Closures and arrow functions ───────────────────────────────────────────

#[test]
fn closure_creates_new_frame() {
    let php = r#"<?php
function test() {
    $x = 1;
    $fn = function() use ($x) {
        echo $x;
    };
}
"#;
    let scope_map = collect_from_function(php);

    // Should have at least 2 frames: function body + closure.
    let closure_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Closure)
        .collect();
    assert_eq!(closure_frames.len(), 1);

    // Closure captures $x.
    assert_eq!(closure_frames[0].captures.len(), 1);
    assert_eq!(closure_frames[0].captures[0].0, "$x");
    assert!(!closure_frames[0].captures[0].1); // not by reference
}

#[test]
fn closure_by_reference_capture() {
    let php = r#"<?php
function test() {
    $x = 1;
    $fn = function() use (&$x) {
        $x = 2;
    };
}
"#;
    let scope_map = collect_from_function(php);

    let closure_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Closure)
        .collect();
    assert_eq!(closure_frames.len(), 1);
    assert_eq!(closure_frames[0].captures[0].0, "$x");
    assert!(closure_frames[0].captures[0].1); // by reference
}

#[test]
fn closure_parameters() {
    let php = r#"<?php
function test() {
    $fn = function($a, $b) {
        return $a + $b;
    };
}
"#;
    let scope_map = collect_from_function(php);

    // Parameters $a and $b should be writes inside the closure frame.
    let closure_a_writes = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$a" && a.kind == AccessKind::Write)
        .count();
    assert!(closure_a_writes >= 1);

    let closure_b_writes = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$b" && a.kind == AccessKind::Write)
        .count();
    assert!(closure_b_writes >= 1);
}

#[test]
fn arrow_function_creates_frame() {
    let php = r#"<?php
function test() {
    $x = 1;
    $fn = fn($y) => $x + $y;
}
"#;
    let scope_map = collect_from_function(php);

    let arrow_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::ArrowFunction)
        .collect();
    assert_eq!(arrow_frames.len(), 1);
}

#[test]
fn nested_closures() {
    let php = r#"<?php
function test() {
    $x = 1;
    $outer = function() use ($x) {
        $y = $x + 1;
        $inner = function() use ($y) {
            return $y;
        };
    };
}
"#;
    let scope_map = collect_from_function(php);

    let closure_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Closure)
        .collect();
    assert_eq!(closure_frames.len(), 2);
}

// ─── $this / self / static / parent tracking ────────────────────────────────

#[test]
fn this_is_tracked() {
    let php = r#"<?php
class Foo {
    public function test() {
        $x = $this->bar();
    }
}
"#;
    let scope_map = collect_from_method(php);

    assert!(scope_map.has_this_or_self);
    let this_reads = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$this" && a.kind == AccessKind::Read)
        .count();
    assert!(this_reads >= 1);
}

#[test]
fn self_static_parent_tracked() {
    let php = r#"<?php
class Foo {
    public function test() {
        $x = self::VALUE;
    }
}
"#;
    let scope_map = collect_from_method(php);
    assert!(scope_map.has_this_or_self);
}

#[test]
fn no_this_when_absent() {
    let php = r#"<?php
function test() {
    $x = 1;
    return $x;
}
"#;
    let scope_map = collect_from_function(php);
    assert!(!scope_map.has_this_or_self);
}

// ─── Reference parameters ───────────────────────────────────────────────────

#[test]
fn reference_parameter_detected() {
    let php = r#"<?php
function test(&$x) {
    $x = 1;
}
"#;
    let scope_map = collect_from_function(php);
    assert!(scope_map.has_reference_params);
}

#[test]
fn no_reference_params() {
    let php = r#"<?php
function test($x) {
    $x = 1;
}
"#;
    let scope_map = collect_from_function(php);
    assert!(!scope_map.has_reference_params);
}

// ─── Static and global declarations ─────────────────────────────────────────

#[test]
fn static_variable_is_write() {
    let php = r#"<?php
function test() {
    static $counter = 0;
    $counter++;
    return $counter;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$counter", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$counter", AccessKind::ReadWrite), 1);
}

#[test]
fn global_variable_is_write() {
    let php = r#"<?php
function test() {
    global $config;
    echo $config;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$config", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$config", AccessKind::Read), 1);
}

#[test]
fn dynamic_property_selector_reads_variable() {
    let php = r#"<?php
function test(object $message, string $type) {
    $attribute = strtolower($type);
    return $message->{$attribute};
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$attribute", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$attribute", AccessKind::Read), 1);
}

// ─── Unset ──────────────────────────────────────────────────────────────────

#[test]
fn unset_is_write() {
    let php = r#"<?php
function test() {
    $x = 1;
    unset($x);
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 2); // assignment + unset
}

// ─── Destructuring ──────────────────────────────────────────────────────────

#[test]
fn array_destructuring() {
    let php = r#"<?php
function test() {
    [$a, $b] = getValues();
    echo $a;
    echo $b;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Read), 1);
}

#[test]
fn list_destructuring() {
    let php = r#"<?php
function test() {
    list($a, $b) = getValues();
    echo $a;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Read), 1);
}

// ─── Array access patterns ──────────────────────────────────────────────────

#[test]
fn array_key_assignment_is_read_write() {
    let php = r#"<?php
function test() {
    $arr = [];
    $arr['key'] = 'value';
    echo $arr;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$arr", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$arr", AccessKind::ReadWrite), 1);
    assert_eq!(count_kind(&scope_map, "$arr", AccessKind::Read), 1);
}

#[test]
fn array_push_is_read_write() {
    let php = r#"<?php
function test() {
    $arr = [];
    $arr[] = 'value';
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$arr", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$arr", AccessKind::ReadWrite), 1);
}

// ─── Frame queries ──────────────────────────────────────────────────────────

#[test]
fn enclosing_frame_finds_function() {
    let php = r#"<?php
function test() {
    $x = 1;
}
"#;
    let scope_map = collect_from_function(php);

    // Any offset inside the function body should find the function frame.
    let frame = scope_map.enclosing_frame(scope_map.frames[0].start + 1);
    assert!(frame.is_some());
    assert_eq!(frame.unwrap().kind, FrameKind::Function);
}

#[test]
fn variables_in_scope_lists_all() {
    let php = r#"<?php
function test($a) {
    $b = 1;
    $c = $a + $b;
    return $c;
}
"#;
    let scope_map = collect_from_function(php);

    let vars = scope_map.variables_in_scope(scope_map.frames[0].start + 1);
    assert!(vars.contains(&"$a".to_string()));
    assert!(vars.contains(&"$b".to_string()));
    assert!(vars.contains(&"$c".to_string()));
}

#[test]
fn all_occurrences_returns_sorted() {
    let php = r#"<?php
function test() {
    $x = 1;
    echo $x;
    $x = 2;
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    let occurrences = scope_map.all_occurrences("$x", scope_map.frames[0].start + 1);
    assert_eq!(occurrences.len(), 4);
    // Should be in source order.
    for i in 1..occurrences.len() {
        assert!(occurrences[i].0 > occurrences[i - 1].0);
    }
    assert_eq!(occurrences[0].1, AccessKind::Write);
    assert_eq!(occurrences[1].1, AccessKind::Read);
    assert_eq!(occurrences[2].1, AccessKind::Write);
    assert_eq!(occurrences[3].1, AccessKind::Read);
}

// ─── Range classification ───────────────────────────────────────────────────

#[test]
fn classify_range_parameters() {
    // $x is written before the range and read inside → parameter.
    let php = r#"<?php
function test() {
    $x = new Foo();
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    // Find the offsets of the write and read.
    let x_accesses = accesses_for(&scope_map, "$x");
    assert_eq!(x_accesses.len(), 2);
    let write_offset = x_accesses[0].0;
    let read_offset = x_accesses[1].0;
    let frame_end = scope_map.frames[0].end;

    // Range that only includes the read (not the write).
    // Use frame_end so the range stays within the function body.
    let classification = scope_map.classify_range(read_offset, frame_end);
    assert!(
        classification.parameters.contains(&"$x".to_string()),
        "Expected $x in parameters, got: {:?}",
        classification.parameters
    );
    assert!(classification.return_values.is_empty());
    assert!(classification.locals.is_empty());

    // Range that includes only the write.
    let classification2 = scope_map.classify_range(write_offset, read_offset);
    // Written inside, read after → return value.
    assert!(
        classification2.return_values.contains(&"$x".to_string()),
        "Expected $x in return_values, got: {:?}",
        classification2
    );
}

#[test]
fn classify_range_locals() {
    // Variable entirely within the range → local.
    let php = r#"<?php
function test() {
    $before = 1;
    $local = 2;
    echo $local;
    $after = 3;
}
"#;
    let scope_map = collect_from_function(php);

    let local_accesses = accesses_for(&scope_map, "$local");
    assert_eq!(local_accesses.len(), 2);

    let _before_accesses = accesses_for(&scope_map, "$before");
    let after_accesses = accesses_for(&scope_map, "$after");

    // Range from $local write to just after $local read, but before $after.
    let range_start = local_accesses[0].0;
    let range_end = after_accesses[0].0;

    let classification = scope_map.classify_range(range_start, range_end);
    assert!(
        classification.locals.contains(&"$local".to_string()),
        "Expected $local in locals, got: {:?}",
        classification
    );
}

#[test]
fn classify_range_return_values() {
    // Variable written inside range and read after → return value.
    let php = r#"<?php
function test() {
    $x = compute();
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    let x_accesses = accesses_for(&scope_map, "$x");
    let write_offset = x_accesses[0].0;
    let read_offset = x_accesses[1].0;

    // Range includes only the write.
    let classification = scope_map.classify_range(write_offset, read_offset);
    assert!(
        classification.return_values.contains(&"$x".to_string()),
        "Expected $x in return_values, got: {:?}",
        classification
    );
}

#[test]
fn classify_range_this_detection() {
    let php = r#"<?php
class Foo {
    public function test() {
        $x = $this->bar();
    }
}
"#;
    let scope_map = collect_from_method(php);

    let frame = &scope_map.frames[0];
    let classification = scope_map.classify_range(frame.start, frame.end);
    assert!(classification.uses_this);
}

#[test]
fn classify_range_no_this() {
    let php = r#"<?php
function test() {
    $x = 1;
}
"#;
    let scope_map = collect_from_function(php);

    let frame = &scope_map.frames[0];
    let classification = scope_map.classify_range(frame.start, frame.end);
    assert!(!classification.uses_this);
}

// ─── Complex scenarios ──────────────────────────────────────────────────────

#[test]
fn method_call_chain() {
    let php = r#"<?php
function test() {
    $builder = new QueryBuilder();
    $result = $builder->where('x', 1)->orderBy('id')->get();
    echo $result;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$builder", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$builder", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Read), 1);
}

#[test]
fn ternary_expression() {
    let php = r#"<?php
function test($cond) {
    $x = $cond ? 'yes' : 'no';
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$cond", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
}

#[test]
fn null_coalescing() {
    let php = r#"<?php
function test($a, $b) {
    $x = $a ?? $b;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$a", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$b", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
}

#[test]
fn instanceof_reads_variable() {
    let php = r#"<?php
function test($obj) {
    if ($obj instanceof Foo) {
        echo $obj;
    }
}
"#;
    let scope_map = collect_from_function(php);

    // $obj: param write + instanceof read + echo read + if condition read.
    let obj_reads = count_kind(&scope_map, "$obj", AccessKind::Read);
    assert!(
        obj_reads >= 2,
        "Expected at least 2 reads for $obj, got {}",
        obj_reads
    );
}

#[test]
fn match_expression_variables() {
    let php = r#"<?php
function test($status) {
    $message = match($status) {
        'active' => 'Active',
        'inactive' => 'Inactive',
        default => 'Unknown',
    };
    echo $message;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$status", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$message", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$message", AccessKind::Read), 1);
}

#[test]
fn yield_expression() {
    let php = r#"<?php
function test() {
    $x = yield 'value';
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
}

#[test]
fn multiple_catch_clauses() {
    let php = r#"<?php
function test() {
    try {
        riskyOperation();
    } catch (\InvalidArgumentException $e) {
        log($e);
    } catch (\RuntimeException $e) {
        log($e);
    }
}
"#;
    let scope_map = collect_from_function(php);

    let catch_frames: Vec<&Frame> = scope_map
        .frames
        .iter()
        .filter(|f| f.kind == FrameKind::Catch)
        .collect();
    assert_eq!(catch_frames.len(), 2);
}

#[test]
fn interpolated_string_variables() {
    let php = r#"<?php
function test($name) {
    $greeting = "Hello, $name!";
}
"#;
    let scope_map = collect_from_function(php);

    // $name: param write + interpolation read.
    assert_eq!(count_kind(&scope_map, "$name", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$name", AccessKind::Read), 1);
}

#[test]
fn clone_expression() {
    let php = r#"<?php
function test($obj) {
    $copy = clone $obj;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$obj", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$copy", AccessKind::Write), 1);
}

#[test]
fn throw_expression() {
    let php = r#"<?php
function test($msg) {
    throw new \Exception($msg);
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$msg", AccessKind::Read), 1);
}

#[test]
fn return_expression() {
    let php = r#"<?php
function test() {
    $x = compute();
    return $x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
}

// ─── Frame nesting ──────────────────────────────────────────────────────────

#[test]
fn accesses_in_frame_excludes_nested() {
    let php = r#"<?php
function test() {
    $x = 1;
    $fn = function() use ($x) {
        $y = $x;
    };
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    // In the outer frame, $y should not appear.
    let outer_frame = scope_map
        .frames
        .iter()
        .find(|f| f.kind == FrameKind::Function)
        .unwrap();
    let y_in_outer = scope_map.accesses_in_frame("$y", outer_frame);
    assert!(
        y_in_outer.is_empty(),
        "Expected $y to not appear in outer frame"
    );
}

#[test]
fn echo_reads_variable() {
    let php = r#"<?php
function test() {
    $x = "hello";
    echo $x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
}

// ─── Spread / variadic ─────────────────────────────────────────────────────

#[test]
fn spread_in_function_call() {
    let php = r#"<?php
function test($args) {
    foo(...$args);
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$args", AccessKind::Read), 1);
}

// ─── isset / empty ─────────────────────────────────────────────────────────

#[test]
fn isset_reads_variable() {
    let php = r#"<?php
function test($x) {
    if (isset($x)) {
        echo $x;
    }
}
"#;
    let scope_map = collect_from_function(php);

    // $x: param write + isset read + if read + echo read.
    let x_reads = count_kind(&scope_map, "$x", AccessKind::Read);
    assert!(
        x_reads >= 2,
        "Expected at least 2 reads for $x, got {}",
        x_reads
    );
}

#[test]
fn empty_reads_variable() {
    let php = r#"<?php
function test($x) {
    if (empty($x)) {
        echo "empty";
    }
}
"#;
    let scope_map = collect_from_function(php);

    assert!(count_kind(&scope_map, "$x", AccessKind::Read) >= 1);
}

// ─── Edge cases ─────────────────────────────────────────────────────────────

#[test]
fn empty_function_body() {
    let php = r#"<?php
function test() {
}
"#;
    let scope_map = collect_from_function(php);

    assert!(scope_map.accesses.is_empty());
    assert_eq!(scope_map.frames.len(), 1);
    assert_eq!(scope_map.frames[0].kind, FrameKind::Function);
}

#[test]
fn function_with_only_parameters() {
    let php = r#"<?php
function test($a, $b, $c) {
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(scope_map.accesses.len(), 3);
    assert!(
        scope_map
            .accesses
            .iter()
            .all(|a| a.kind == AccessKind::Write)
    );
}

#[test]
fn nested_array_access() {
    let php = r#"<?php
function test() {
    $data = [];
    $x = $data['foo']['bar'];
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$data", AccessKind::Write), 1);
    assert_eq!(count_kind(&scope_map, "$data", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
}

#[test]
fn property_access_reads_object() {
    let php = r#"<?php
function test($obj) {
    $x = $obj->name;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$obj", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Write), 1);
}

#[test]
fn property_write_reads_object() {
    let php = r#"<?php
function test($obj) {
    $obj->name = "test";
}
"#;
    let scope_map = collect_from_function(php);

    // Writing $obj->name reads $obj (the object itself).
    assert_eq!(count_kind(&scope_map, "$obj", AccessKind::Read), 1);
}

#[test]
fn cast_reads_variable() {
    let php = r#"<?php
function test($x) {
    $y = (string)$x;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$x", AccessKind::Read), 1);
    assert_eq!(count_kind(&scope_map, "$y", AccessKind::Write), 1);
}

#[test]
fn elseif_branches() {
    let php = r#"<?php
function test($val) {
    if ($val === 1) {
        $result = "one";
    } elseif ($val === 2) {
        $result = "two";
    } else {
        $result = "other";
    }
    echo $result;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Write), 3);
    assert_eq!(count_kind(&scope_map, "$result", AccessKind::Read), 1);
    // $val reads: if condition + elseif condition.
    assert!(count_kind(&scope_map, "$val", AccessKind::Read) >= 2);
}

#[test]
fn include_reads_path() {
    let php = r#"<?php
function test($path) {
    include $path;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$path", AccessKind::Read), 1);
}

#[test]
fn print_reads_variable() {
    let php = r#"<?php
function test($msg) {
    print $msg;
}
"#;
    let scope_map = collect_from_function(php);

    assert_eq!(count_kind(&scope_map, "$msg", AccessKind::Read), 1);
}

// ─── classify_range complex scenarios ───────────────────────────────────────

#[test]
fn classify_range_mixed_roles() {
    // $x is a parameter (read inside, written before)
    // $y is a local (written and read only inside)
    // $z is a return value (written inside, read after)
    let php = r#"<?php
function test() {
    $x = 1;
    $y = $x + 1;
    echo $y;
    $z = $y * 2;
    echo $z;
}
"#;
    let scope_map = collect_from_function(php);

    let _x_accesses = accesses_for(&scope_map, "$x");
    let y_accesses = accesses_for(&scope_map, "$y");
    let z_accesses = accesses_for(&scope_map, "$z");

    // Extract the middle range: from $y write to just before $z write.
    let range_start = y_accesses[0].0;
    let range_end = z_accesses[0].0;

    let classification = scope_map.classify_range(range_start, range_end);

    // $x is written before range, read inside → parameter.
    assert!(
        classification.parameters.contains(&"$x".to_string()),
        "Expected $x as parameter: {:?}",
        classification
    );

    // $y is written inside, read inside, then read after (in $z = $y * 2) → depends on range.
    // Since $y is also read at `echo $y` which might be inside or outside depending on exact offsets.
    // The important thing is the classification runs without panicking.
}

#[test]
fn classify_excludes_this_from_names() {
    let php = r#"<?php
class Foo {
    public function test() {
        $x = $this->bar();
        return $x;
    }
}
"#;
    let scope_map = collect_from_method(php);

    let frame = &scope_map.frames[0];
    let classification = scope_map.classify_range(frame.start, frame.end);

    // $this should NOT appear in parameters/return_values/locals.
    assert!(!classification.parameters.contains(&"$this".to_string()));
    assert!(!classification.return_values.contains(&"$this".to_string()));
    assert!(!classification.locals.contains(&"$this".to_string()));
    // But uses_this should be true.
    assert!(classification.uses_this);
}

// ─── Accumulator pattern ────────────────────────────────────────────────────

#[test]
fn classify_range_init_and_accumulate_is_return_only() {
    // $count is first written inside the range ($count = 0), then read
    // and written again inside ($count = $count + …), then read after
    // (return $count).  Because its first write is inside the range and
    // there is no write before, it should be a return value only — NOT
    // a parameter.
    let php = r#"<?php
function test($items) {
    $count = 0;
    foreach ($items as $item) {
        $count = $count + 1;
    }
    return $count;
}
"#;
    let scope_map = collect_from_function(php);

    let count_accesses = accesses_for(&scope_map, "$count");
    // First access is the `$count = 0` write.
    let range_start = count_accesses[0].0;
    // Range ends just before `return $count`.
    let last_access = count_accesses.last().unwrap().0;
    let range_end = last_access; // exclude the return read

    let classification = scope_map.classify_range(range_start, range_end);

    assert!(
        classification.return_values.contains(&"$count".to_string()),
        "Expected $count in return_values: {:?}",
        classification
    );
    assert!(
        !classification.parameters.contains(&"$count".to_string()),
        "$count must NOT be a parameter (first write is inside range): {:?}",
        classification
    );
}

#[test]
fn classify_range_read_before_inner_write_is_param_and_return() {
    // $subcategories is read at the start of the range (`if (!$subcategories)`)
    // before its first write inside the range, so the range consumes the
    // incoming value → it must be a parameter.  It is also written inside
    // and read after → it is also a return value.
    let php = r#"<?php
function test($subcategories) {
    if (!$subcategories) {
        $subcategories = array_merge($subcategories, [1]);
    }
    return $subcategories;
}
"#;
    let scope_map = collect_from_function(php);

    let range_start = php.find("if (!$subcategories)").unwrap() as u32;
    let range_end = php.find("return $subcategories;").unwrap() as u32;

    let classification = scope_map.classify_range(range_start, range_end);

    assert!(
        classification
            .parameters
            .contains(&"$subcategories".to_string()),
        "$subcategories must be a parameter (read before its inner write): {:?}",
        classification
    );
    assert!(
        classification
            .return_values
            .contains(&"$subcategories".to_string()),
        "$subcategories must also be a return value (written inside, read after): {:?}",
        classification
    );
}

// ─── Source order ───────────────────────────────────────────────────────────

#[test]
fn accesses_are_in_source_order() {
    let php = r#"<?php
function test() {
    $a = 1;
    $b = $a;
    $c = $b;
    echo $c;
}
"#;
    let scope_map = collect_from_function(php);

    // All accesses should be in ascending offset order.
    for i in 1..scope_map.accesses.len() {
        assert!(
            scope_map.accesses[i].offset >= scope_map.accesses[i - 1].offset,
            "Access at index {} (offset {}) is before index {} (offset {})",
            i,
            scope_map.accesses[i].offset,
            i - 1,
            scope_map.accesses[i - 1].offset,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Static property access — should NOT produce variable reads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn static_property_not_recorded_as_variable_read() {
    let php = r#"<?php
class Config {
    private static ?string $instance = null;

    public static function get(): ?string {
        if (self::$instance === null) {
            self::$instance = 'default';
        }
        return self::$instance;
    }
}
"#;
    let scope_map = collect_from_method(php);

    // $instance should NOT appear in accesses at all — it is a static
    // property, not a local variable.
    let instance_accesses: Vec<_> = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$instance")
        .collect();
    assert!(
        instance_accesses.is_empty(),
        "self::$instance should not be recorded as a variable access. Got: {:?}",
        instance_accesses,
    );
}

#[test]
fn static_keyword_property_not_recorded() {
    let php = r#"<?php
class Base {
    protected static int $count = 0;

    public function increment(): void {
        static::$count++;
    }
}
"#;
    let scope_map = collect_from_method(php);

    let count_accesses: Vec<_> = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$count")
        .collect();
    assert!(
        count_accesses.is_empty(),
        "static::$count should not be recorded as a variable access. Got: {:?}",
        count_accesses,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// By-reference out-parameters — should produce Write accesses
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn preg_match_out_param_is_write() {
    let php = r#"<?php
function test(string $input): ?string {
    if (preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#;
    let scope_map = collect_from_function(php);

    let match_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$match" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        match_writes >= 1,
        "preg_match's $match argument should be recorded as a Write",
    );
}

#[test]
fn parse_str_out_param_is_write() {
    let php = r#"<?php
function test(string $query): string {
    parse_str($query, $data);
    return $data['key'] ?? '';
}
"#;
    let scope_map = collect_from_function(php);

    let data_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$data" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        data_writes >= 1,
        "parse_str's $data argument should be recorded as a Write",
    );
}

#[test]
fn fqn_preg_match_out_param_is_write() {
    let php = r#"<?php
function test(string $input): ?string {
    if (\preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#;
    let scope_map = collect_from_function(php);

    let match_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$match" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        match_writes >= 1,
        "FQN \\preg_match's $match argument should be recorded as a Write",
    );
}

#[test]
fn non_out_param_args_still_reads() {
    let php = r#"<?php
function test(string $input): ?string {
    if (preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#;
    let scope_map = collect_from_function(php);

    // $input (arg index 1) should still be a Read, not a Write.
    let input_reads = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$input" && matches!(a.kind, AccessKind::Read))
        .count();
    assert!(
        input_reads >= 1,
        "Non-out-param arguments should still be recorded as reads",
    );
}

// Expanded by-ref out-parameter table — one representative test per category
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn curl_multi_exec_out_param_is_write() {
    let php = r#"<?php
function test($mh): int {
    curl_multi_exec($mh, $running);
    return $running;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$running" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "curl_multi_exec's $running should be a Write");
}

#[test]
fn fsockopen_out_params_are_writes() {
    let php = r#"<?php
function test(): void {
    $fp = fsockopen('example.com', 80, $errno, $errstr);
}
"#;
    let scope_map = collect_from_function(php);
    let errno_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$errno" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let errstr_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$errstr" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(errno_writes >= 1, "fsockopen's $errno should be a Write");
    assert!(errstr_writes >= 1, "fsockopen's $errstr should be a Write");
}

#[test]
fn openssl_sign_out_param_is_write() {
    let php = r#"<?php
function test(string $data, $key): string {
    openssl_sign($data, $signature, $key);
    return $signature;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$signature" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "openssl_sign's $signature should be a Write");
}

#[test]
fn mb_parse_str_out_param_is_write() {
    let php = r#"<?php
function test(string $input): array {
    mb_parse_str($input, $result);
    return $result;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$result" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "mb_parse_str's $result should be a Write");
}

#[test]
fn pcntl_wait_out_param_is_write() {
    let php = r#"<?php
function test(): void {
    pcntl_wait($status);
    echo $status;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$status" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "pcntl_wait's $status should be a Write");
}

#[test]
fn getimagesize_out_param_is_write() {
    let php = r#"<?php
function test(string $file): array {
    $info = getimagesize($file, $imageinfo);
    return $imageinfo;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$imageinfo" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "getimagesize's $imageinfo should be a Write");
}

#[test]
fn dns_get_mx_out_params_are_writes() {
    let php = r#"<?php
function test(string $host): void {
    dns_get_mx($host, $mxhosts, $weights);
    var_dump($mxhosts, $weights);
}
"#;
    let scope_map = collect_from_function(php);
    let mxhosts_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$mxhosts" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let weights_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$weights" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        mxhosts_writes >= 1,
        "dns_get_mx's $mxhosts should be a Write"
    );
    assert!(
        weights_writes >= 1,
        "dns_get_mx's $weights should be a Write"
    );
}

#[test]
fn flock_out_param_is_write() {
    let php = r#"<?php
function test($fp): void {
    flock($fp, LOCK_EX, $wouldblock);
    echo $wouldblock;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$wouldblock" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "flock's $wouldblock should be a Write");
}

#[test]
fn msg_receive_out_params_are_writes() {
    let php = r#"<?php
function test($queue): void {
    msg_receive($queue, 1, $msgtype, 1024, $data, true, 0, $errorcode);
    echo $msgtype . $data . $errorcode;
}
"#;
    let scope_map = collect_from_function(php);
    let msgtype_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$msgtype" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let data_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$data" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let errorcode_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$errorcode" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        msgtype_writes >= 1,
        "msg_receive's $msgtype should be a Write"
    );
    assert!(data_writes >= 1, "msg_receive's $data should be a Write");
    assert!(
        errorcode_writes >= 1,
        "msg_receive's $errorcode should be a Write"
    );
}

#[test]
fn ldap_parse_result_out_params_are_writes() {
    let php = r#"<?php
function test($ldap, $result): void {
    ldap_parse_result($ldap, $result, $errcode, $matcheddn, $errmsg, $referrals, $controls);
    echo $errcode;
}
"#;
    let scope_map = collect_from_function(php);
    let errcode_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$errcode" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        errcode_writes >= 1,
        "ldap_parse_result's $errcode should be a Write"
    );
}

#[test]
fn headers_sent_out_params_are_writes() {
    let php = r#"<?php
function test(): void {
    headers_sent($file, $line);
    echo $file . ':' . $line;
}
"#;
    let scope_map = collect_from_function(php);
    let file_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$file" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let line_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$line" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(file_writes >= 1, "headers_sent's $file should be a Write");
    assert!(line_writes >= 1, "headers_sent's $line should be a Write");
}

#[test]
fn getopt_out_param_is_write() {
    let php = r#"<?php
function test(): void {
    $opts = getopt('v', ['verbose'], $optind);
    echo $optind;
}
"#;
    let scope_map = collect_from_function(php);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$optind" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(writes >= 1, "getopt's $optind should be a Write");
}

#[test]
fn exif_thumbnail_out_params_are_writes() {
    let php = r#"<?php
function test(string $file): void {
    $thumb = exif_thumbnail($file, $width, $height, $type);
    echo $width . 'x' . $height;
}
"#;
    let scope_map = collect_from_function(php);
    let width_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$width" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let height_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$height" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    let type_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$type" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        width_writes >= 1,
        "exif_thumbnail's $width should be a Write"
    );
    assert!(
        height_writes >= 1,
        "exif_thumbnail's $height should be a Write"
    );
    assert!(type_writes >= 1, "exif_thumbnail's $type should be a Write");
}

// By-reference resolver callback — user-defined functions, static methods, constructors
// ═══════════════════════════════════════════════════════════════════════════

/// Helper: parse PHP and collect scope from the first function body,
/// using a custom by-ref resolver callback.
fn collect_from_function_with_resolver<F>(php: &str, resolver: F) -> ScopeMap
where
    F: Fn(&super::ByRefCallKind<'_>) -> Option<Vec<usize>>,
{
    with_parsed_program(php, "test", |program, _content| {
        for stmt in program.statements.iter() {
            if let Statement::Function(func) = stmt {
                let body_start = func.body.left_brace.start.offset;
                let body_end = func.body.right_brace.end.offset;
                return super::collect_function_scope_with_resolver(
                    &func.parameter_list,
                    func.body.statements.as_slice(),
                    body_start,
                    body_end,
                    Some(&resolver),
                );
            }
        }
        panic!("No function found in test PHP code");
    })
}

#[test]
fn user_defined_function_byref_via_resolver() {
    let php = r#"<?php
function test(): void {
    myFunc($output);
    echo $output;
}
"#;
    let resolver = |kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        match kind {
            super::ByRefCallKind::Function(name) if *name == "myFunc" => Some(vec![0]),
            _ => None,
        }
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$output" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        writes >= 1,
        "User-defined function's by-ref $output should be a Write"
    );
}

#[test]
fn resolver_does_not_override_hardcoded_table() {
    // preg_match is in the hardcoded table — the resolver should not
    // be consulted for it, and it should still work.
    let php = r#"<?php
function test(string $input): void {
    preg_match('/(\d+)/', $input, $match);
    echo $match[0];
}
"#;
    let resolver = |_kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        // Return None for everything — hardcoded table should still apply.
        None
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$match" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        writes >= 1,
        "Hardcoded preg_match should still mark $match as Write even when resolver returns None"
    );
}

#[test]
fn static_method_byref_via_resolver() {
    let php = r#"<?php
function test(): void {
    Validator::validate($errors);
    echo $errors;
}
"#;
    let resolver = |kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        match kind {
            super::ByRefCallKind::StaticMethod(class, method)
                if *class == "Validator" && *method == "validate" =>
            {
                Some(vec![0])
            }
            _ => None,
        }
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$errors" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        writes >= 1,
        "Static method's by-ref $errors should be a Write"
    );
}

#[test]
fn constructor_byref_via_resolver() {
    let php = r#"<?php
function test(): void {
    $obj = new Parser($warnings);
    echo $warnings;
}
"#;
    let resolver = |kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        match kind {
            super::ByRefCallKind::Constructor(class) if *class == "Parser" => Some(vec![0]),
            _ => None,
        }
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$warnings" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        writes >= 1,
        "Constructor's by-ref $warnings should be a Write"
    );
}

#[test]
fn resolver_no_match_treats_args_as_reads() {
    let php = r#"<?php
function test(): void {
    unknownFunc($var);
}
"#;
    let resolver = |_kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> { None };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let reads = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$var" && matches!(a.kind, AccessKind::Read))
        .count();
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$var" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite))
        .count();
    assert!(reads >= 1, "Unresolved function args should be reads");
    assert_eq!(writes, 0, "Unresolved function args should not be writes");
}

#[test]
fn resolver_second_arg_byref_first_arg_read() {
    let php = r#"<?php
function test(string $input): void {
    transform($input, $result);
    echo $result;
}
"#;
    let resolver = |kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        match kind {
            super::ByRefCallKind::Function(name) if *name == "transform" => Some(vec![1]),
            _ => None,
        }
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);

    // $input (arg 0) should be a Read.
    let input_reads = scope_map
        .accesses
        .iter()
        .filter(|a| a.name == "$input" && matches!(a.kind, AccessKind::Read))
        .count();
    assert!(
        input_reads >= 1,
        "Non-byref first arg should still be a Read"
    );

    // $result (arg 1) should be a Write.
    let result_writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$result" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(result_writes >= 1, "By-ref second arg should be a Write");
}

#[test]
fn static_method_with_self_keyword_byref() {
    let php = r#"<?php
function test(): void {
    self::parse($output);
    echo $output;
}
"#;
    let resolver = |kind: &super::ByRefCallKind<'_>| -> Option<Vec<usize>> {
        match kind {
            super::ByRefCallKind::StaticMethod(class, method)
                if *class == "self" && *method == "parse" =>
            {
                Some(vec![0])
            }
            _ => None,
        }
    };
    let scope_map = collect_from_function_with_resolver(php, resolver);
    let writes = scope_map
        .accesses
        .iter()
        .filter(|a| {
            a.name == "$output" && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
        })
        .count();
    assert!(
        writes >= 1,
        "self::parse() by-ref $output should be a Write"
    );
}
