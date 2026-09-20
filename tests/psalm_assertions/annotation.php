<?php
// Source: Psalm AnnotationTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: nopType
namespace PsalmTest_annotation_1 {
    $_a = "hello";

    /** @var int $_a */

    assertType('int', $_a);
}

// Test: doubleVar
namespace PsalmTest_annotation_2 {
    function foo() : array {
        return ["hello" => new stdClass, "goodbye" => new stdClass];
    }

    $_a = null;
    $_b = null;

    /**
     * @var string $_key
     * @var stdClass $_value
     */
    foreach (foo() as $_key => $_value) {
        $_a = $_key;
        $_b = $_value;
    }

    assertType('null|string', $_a);
    assertType('null|stdClass', $_b);
}

// Test: spreadOperatorByRefAnnotation
namespace PsalmTest_annotation_3 {
    /**
     * @param string &...$s
     * @psalm-suppress UnusedParam
     */
    function foo(&...$s) : void {}
    /**
     * @param string ...&$s
     * @psalm-suppress UnusedParam
     */
    function bar(&...$s) : void {}
    /**
     * @param string[] &$s
     * @psalm-suppress UnusedParam
     */
    function bat(&...$s) : void {}

    $a = "hello";
    $b = "goodbye";
    $c = "hello again";
    foo($a);
    bar($b);
    bat($c);

    assertType('string', $a);
    assertType('string', $b);
    assertType('string', $c);
}

// Test: arrayWithKeySlashesAndNewline
namespace PsalmTest_annotation_4 {
    $_arr = ["foo\bar\nbaz" => "literal"];

    // PHPantom keeps the value the literal names, where Psalm widens it to
    // `string`. Only the key's escaping is what this case is really about:
    // `\b` is no escape at all and stays two characters, while `\n` really
    // is a newline, so the key is spelled back out double-quoted.
    assertType('array{"foo\\bar\nbaz": "literal"}', $_arr);
}

