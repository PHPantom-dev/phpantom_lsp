<?php
// Source: Psalm NamespaceTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: varsAreNotScoped
namespace A {
    $a = "1";
}
namespace B\C {
    $bc = "2";
}
namespace {
    echo $a . PHP_EOL;
    echo $bc . PHP_EOL;
}

namespace {
    assertType('\'1\'', $a);
    assertType('\'2\'', $bc);
}

