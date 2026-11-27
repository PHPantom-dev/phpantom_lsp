use super::*;

/// A file whose function nests `depth` closures, each passed to a generic
/// function inside the body of the one around it.
fn nested_callbacks(depth: usize) -> String {
    let mut body = String::from("$w = 1;");
    for _ in 0..depth {
        body = format!(
            "$y = my_map(function ($q) use ($x) {{
                $y = $q;
                if ($y) {{ {body} }}
                return my_map(function ($z) use ($x) {{ return $z; }}, [$y]);
            }}, [$x]);"
        );
    }
    format!(
        "<?php
/**
 * @template T
 * @template R
 * @param callable(T): R $f
 * @param array<T> $a
 * @return array<R>
 */
function my_map(callable $f, array $a): array {{ return array_map($f, $a); }}
function t(int $x) {{
    {body}
    return $y;
}}
"
    )
}

/// How many closure bodies a slow diagnostic pass over `content` walks to
/// bind templates from closure arguments.
fn closure_walks(content: &str) -> usize {
    const URI: &str = "file:///project/src/nested.php";
    let backend = Backend::new_test();
    backend.update_ast(URI, content);
    let before = TEST_CLOSURE_WALKS.with(std::cell::Cell::get);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(URI, content, &mut out);
    TEST_CLOSURE_WALKS.with(std::cell::Cell::get) - before
}

/// Reading a closure argument reads the closures nested in its body.
/// Each closure is parsed as a source of its own, so a closure is read
/// once more for every closure around it, but no more: one more level of
/// nesting adds one walk more than the level before did.
#[test]
fn nested_closure_arguments_are_walked_once_per_enclosing_level() {
    let walks: Vec<usize> = [4, 5, 6, 7, 8]
        .into_iter()
        .map(|depth| closure_walks(&nested_callbacks(depth)))
        .collect();
    let steps: Vec<usize> = walks.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let growth: Vec<usize> = steps.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(
        growth.windows(2).all(|pair| pair[0] == pair[1]),
        "walks per depth grow quadratically at most: {walks:?}"
    );
}
