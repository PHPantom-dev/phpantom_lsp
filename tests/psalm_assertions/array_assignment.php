<?php
// Source: Psalm ArrayAssignmentTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: assignUnionOfLiterals
namespace PsalmTest_array_assignment_1 {
    $result = [];

    foreach (["a", "b"] as $k) {
        $result[$k] = true;
    }

    $resultOpt = [];

    foreach (["a", "b"] as $k) {
        if (random_int(0, 1)) {
            continue;
        }
        $resultOpt[$k] = true;
    }

    assertType('array{a: true, b: true}', $result);
    assertType('array{a?: true, b?: true}', $resultOpt);
}

// Test: assignUnionOfLiteralsClassKeys
namespace PsalmTest_array_assignment_2 {
    class a {}
    class b {}

    $result = [];

    foreach ([a::class, b::class] as $k) {
        $result[$k] = true;
    }

    foreach ($result as $k => $v) {
        $vv = new $k;
    }

    assertType('array{a::class: true, b::class: true}', $result);
}

// Test: genericArrayCreationWithSingleIntValue
namespace PsalmTest_array_assignment_3 {
    $out = [];

    $out[] = 4;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{4}', $out);
}

// Test: genericArrayCreationWithObjectAddedInIf
namespace PsalmTest_array_assignment_4 {
    class B {}

    $out = [];

    if (rand(0,10) === 10) {
        $out[] = new B();
    }

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{}|array{B}', $out);
}

// Test: genericArrayCreationWithElementAddedInSwitch
namespace PsalmTest_array_assignment_5 {
    $out = [];

    switch (rand(0,10)) {
        case 5:
            $out[] = 4;
            break;

        case 6:
            // do nothing
    }

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{4}|array{}', $out);
}

// Test: genericArrayCreationWithElementsAddedInSwitch
namespace PsalmTest_array_assignment_6 {
    $out = [];

    switch (rand(0,10)) {
        case 5:
            $out[] = 4;
            break;

        case 6:
            $out[] = "hello";
            break;
    }

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{4}|array{"hello"}|array{}', $out);
}

// Test: genericArrayCreationWithElementsAddedInSwitchWithNothing
namespace PsalmTest_array_assignment_7 {
    $out = [];

    switch (rand(0,10)) {
        case 5:
            $out[] = 4;
            break;

        case 6:
            $out[] = "hello";
            break;

        case 7:
            // do nothing
    }

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{4}|array{"hello"}|array{}', $out);
}

// Test: implicitIndexedIntArrayCreation
namespace PsalmTest_array_assignment_8 {
    $foo = [];
    $foo[0] = "a";
    $foo[1] = "b";
    $foo[2] = "c";

    $bar = [0, 1, 2];

    $bat = [];

    foreach ($foo as $i => $text) {
        $bat[$text] = $bar[$i];
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"a", "b", "c"}', $foo);
    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{0, 1, 2}', $bar);
    assertType('array{a: int, b: int, c: int}', $bat);
}

// Test: implicitStringArrayCreation
namespace PsalmTest_array_assignment_9 {
    $foo = [];
    $foo["bar"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: "hello"}', $foo);
    assertType('"hello"', $foo['bar']);
}

// Test: implicit2dStringArrayCreation
namespace PsalmTest_array_assignment_10 {
    $foo = [];
    $foo["bar"]["baz"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{baz: "hello"}}', $foo);
    assertType('"hello"', $foo['bar']['baz']);
}

// Test: implicit3dStringArrayCreation
namespace PsalmTest_array_assignment_11 {
    $foo = [];
    $foo["bar"]["baz"]["bat"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{baz: array{bat: "hello"}}}', $foo);
    assertType('"hello"', $foo['bar']['baz']['bat']);
}

// Test: implicit4dStringArrayCreation
namespace PsalmTest_array_assignment_12 {
    $foo = [];
    $foo["bar"]["baz"]["bat"]["bap"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{baz: array{bat: array{bap: "hello"}}}}', $foo);
    assertType('"hello"', $foo['bar']['baz']['bat']['bap']);
}

// Test: 2Step2dStringArrayCreation
namespace PsalmTest_array_assignment_13 {
    $foo = ["bar" => []];
    $foo["bar"]["baz"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{baz: "hello"}}', $foo);
    assertType('"hello"', $foo['bar']['baz']);
}

// Test: 2StepImplicit3dStringArrayCreation
namespace PsalmTest_array_assignment_14 {
    $foo = ["bar" => []];
    $foo["bar"]["baz"]["bat"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{baz: array{bat: "hello"}}}', $foo);
}

// Test: conflictingTypesWithNoAssignment
namespace PsalmTest_array_assignment_15 {
    $foo = [
        "bar" => ["a" => "b"],
        "baz" => [1]
    ];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{a: "b"}, baz: array{1}}', $foo);
}

// Test: implicitTKeyedArrayCreation
namespace PsalmTest_array_assignment_16 {
    $foo = [
        "bar" => 1,
    ];
    $foo["baz"] = "a";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: 1, baz: "a"}', $foo);
}

// Test: conflictingTypesWithAssignment
namespace PsalmTest_array_assignment_17 {
    $foo = [
        "bar" => ["a" => "b"],
        "baz" => [1]
    ];
    $foo["bar"]["bam"]["baz"] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{bar: array{a: "b", bam: array{baz: "hello"}}, baz: array{1}}', $foo);
}

// Test: conflictingTypesWithAssignment2
namespace PsalmTest_array_assignment_18 {
    $foo = [];
    $foo["a"] = "hello";
    $foo["b"][] = "goodbye";
    $bar = $foo["a"];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"', $foo['a']);
    assertType('"hello"', $bar);
}

// Test: conflictingTypesWithAssignment3
namespace PsalmTest_array_assignment_19 {
    $foo = [];
    $foo["a"] = "hello";
    $foo["b"]["c"]["d"] = "goodbye";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{a: "hello", b: array{c: array{d: "goodbye"}}}', $foo);
}

// Test: nestedTKeyedArrayAssignment
namespace PsalmTest_array_assignment_20 {
    $foo = [];
    $foo["a"]["b"] = "hello";
    $foo["a"]["c"] = 1;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{a: array{b: "hello", c: 1}}', $foo);
}

// Test: conditionalTKeyedArrayAssignment
namespace PsalmTest_array_assignment_21 {
    $foo = ["a" => "hello"];
    if (rand(0, 10) === 5) {
        $foo["b"] = 1;
    }
    else {
        $foo["b"] = 2;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{a: "hello", b: 1|2}', $foo);
}

// Test: arrayKey
namespace PsalmTest_array_assignment_22 {
    $a = ["foo", "bar"];
    $b = $a[0];

    $c = ["a" => "foo", "b"=> "bar"];
    $d = "a";
    $e = $c[$d];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"foo"', $b);
    assertType('"foo"', $e);
}

// Test: assignExplicitValueToGeneric
namespace PsalmTest_array_assignment_23 {
    /** @var array<string, array<string, string>> */
    $a = [];
    $a["foo"] = ["bar" => "baz"];

    // PHPantom follows PHPStan: a keyed write to a declared `array<string, …>` keeps the generic type.
    assertType('non-empty-array<string, array<string, string>>', $a);
}

// Test: additionWithEmpty
namespace PsalmTest_array_assignment_24 {
    $a = [];
    $a += ["bar"];

    $b = [] + ["bar"];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"bar"}', $a);
    assertType('array{"bar"}', $b);
}

// Test: additionDifferentType
namespace PsalmTest_array_assignment_25 {
    $a = ["bar"];
    $a += [1];

    $b = ["bar"] + [1];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"bar"}', $a);
    assertType('array{"bar"}', $b);
}

// Test: objectLikeWithIntegerKeys
namespace PsalmTest_array_assignment_26 {
    /** @var array{0: string, 1: int} **/
    $a = ["hello", 5];
    $b = $a[0]; // string
    $c = $a[1]; // int
    list($d, $e) = $a; // $d is string, $e is int

    assertType('string', $b);
    assertType('int', $c);
    assertType('string', $d);
    assertType('int', $e);
}

// Test: objectLikeArrayAdditionNotNested
namespace PsalmTest_array_assignment_27 {
    $foo = [];
    $foo["a"] = 1;
    $foo += ["b" => [2, 3]];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{a: 1, b: array{2, 3}}', $foo);
}

// Test: nestedTKeyedArrayAddition
namespace PsalmTest_array_assignment_28 {
    $foo = [];
    $foo["root"]["a"] = 1;
    $foo["root"] += ["b" => [2, 3]];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{root: array{a: 1, b: array{2, 3}}}', $foo);
}

// Test: updateStringIntKey1
namespace PsalmTest_array_assignment_29 {
    $a = [];

    $a["a"] = 5;
    $a[0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{a: 5, 3}', $a);
}

// Test: updateStringIntKey2
namespace PsalmTest_array_assignment_30 {
    $string = "c";

    $b = [];

    $b[$string] = 5;
    $b[0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{c: 5, 3}', $b);
}

// Test: updateStringIntKey3
namespace PsalmTest_array_assignment_31 {
    $string = "c";

    $c = [];

    $c[0] = 3;
    $c[$string] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{3, c: 5}', $c);
}

// Test: updateStringIntKey4
namespace PsalmTest_array_assignment_32 {
    $int = 5;

    $d = [];

    $d[$int] = 3;
    $d["a"] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{5: 3, a: 5}', $d);
}

// Test: updateStringIntKey5
namespace PsalmTest_array_assignment_33 {
    $string = "c";
    $int = 5;

    $e = [];

    $e[$int] = 3;
    $e[$string] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{5: 3, c: 5}', $e);
}

// Test: updateStringIntKeyWithIntRootAndNumberOffset
namespace PsalmTest_array_assignment_34 {
    $string = "c";
    $int = 5;

    $a = [];

    $a[0]["a"] = 5;
    $a[0][0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{array{a: 5, 3}}', $a);
}

// Test: updateStringIntKeyWithIntRoot
namespace PsalmTest_array_assignment_35 {
    $string = "c";
    $int = 5;

    $b = [];

    $b[0][$string] = 5;
    $b[0][0] = 3;

    $c = [];

    $c[0][0] = 3;
    $c[0][$string] = 5;

    $d = [];

    $d[0][$int] = 3;
    $d[0]["a"] = 5;

    $e = [];

    $e[0][$int] = 3;
    $e[0][$string] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{array{c: 5, 3}}', $b);
    assertType('array{array{3, c: 5}}', $c);
    assertType('array{array{5: 3, a: 5}}', $d);
    assertType('array{array{5: 3, c: 5}}', $e);
}

// Test: updateStringIntKeyWithTKeyedArrayRootAndNumberOffset
namespace PsalmTest_array_assignment_36 {
    $string = "c";
    $int = 5;

    $a = [];

    $a["root"]["a"] = 5;
    $a["root"][0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{root: array{a: 5, 3}}', $a);
}

// Test: updateStringIntKeyWithTKeyedArrayRoot
namespace PsalmTest_array_assignment_37 {
    $string = "c";
    $int = 5;

    $b = [];

    $b["root"][$string] = 5;
    $b["root"][0] = 3;

    $c = [];

    $c["root"][0] = 3;
    $c["root"][$string] = 5;

    $d = [];

    $d["root"][$int] = 3;
    $d["root"]["a"] = 5;

    $e = [];

    $e["root"][$int] = 3;
    $e["root"][$string] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{root: array{c: 5, 3}}', $b);
    assertType('array{root: array{3, c: 5}}', $c);
    assertType('array{root: array{5: 3, a: 5}}', $d);
    assertType('array{root: array{5: 3, c: 5}}', $e);
}

// Test: changeTKeyedArrayType
namespace PsalmTest_array_assignment_38 {
    $a = ["b" => "c"];
    $a["d"] = ["e" => "f"];
    $a["b"] = 4;
    $a["d"]["e"] = 5;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('4', $a['b']);
    assertType('array{e: 5}', $a['d']);
    assertType('5', $a['d']['e']);
    assertType('array{b: 4, d: array{e: 5}}', $a);
}

// Test: changeTKeyedArrayTypeInIf
namespace PsalmTest_array_assignment_39 {
    $a = [];

    if (rand(0, 5) > 3) {
      $a["b"] = new stdClass;
    } else {
      $a["b"] = ["e" => "f"];
    }

    if ($a["b"] instanceof stdClass) {
      $a["b"] = [];
    }

    $a["b"]["e"] = "d";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{b: array{e: "d"}}', $a);
    assertType('array{e: "d"}', $a['b']);
    assertType('"d"', $a['b']['e']);
}

// Test: implementsArrayAccess
namespace PsalmTest_array_assignment_40 {
    /**
     * @implements \ArrayAccess<array-key, mixed>
     */
    class A implements \ArrayAccess {
        /**
         * @param  string|int $offset
         * @param  mixed $value
         */
        public function offsetSet($offset, $value): void {}

        /** @param string|int $offset */
        public function offsetExists($offset): bool {
            return true;
        }

        /** @param string|int $offset */
        public function offsetUnset($offset): void {}

        /**
         * @param  string $offset
         * @return mixed
         */
        public function offsetGet($offset) {
            return 1;
        }
    }

    $a = new A();
    $a["bar"] = "cool";
    $a["bar"]->foo();

    assertType('A', $a);
}

// Test: stringAssignment
namespace PsalmTest_array_assignment_41 {
    $str = "hello";
    $str[0] = "i";

    assertType('string', $str);
}

// Test: keyedIntOffsetArrayValues
namespace PsalmTest_array_assignment_42 {
    $a = ["hello", 5];
    /** @psalm-suppress RedundantFunctionCall */
    $a_values = array_values($a);
    $a_keys = array_keys($a);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"hello", 5}', $a);
}

// Test: changeIntOffsetKeyValuesWithDirectAssignment
namespace PsalmTest_array_assignment_43 {
    $b = ["hello", 5];
    $b[0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{3, 5}', $b);
}

// Test: changeIntOffsetKeyValuesAfterCopy
namespace PsalmTest_array_assignment_44 {
    $b = ["hello", 5];
    $c = $b;
    $c[0] = 3;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"hello", 5}', $b);
    assertType('array{3, 5}', $c);
}

// Test: mergeIntOffsetValues
namespace PsalmTest_array_assignment_45 {
    $d = array_merge(["hello", 5], []);
    $e = array_merge(["hello", 5], ["hello again"]);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"hello", 5}', $d);
    assertType('array{"hello", 5, "hello again"}', $e);
}

// Test: addIntOffsetToEmptyArray
namespace PsalmTest_array_assignment_46 {
    $f = [];
    $f[0] = "hello";

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{"hello"}', $f);
}

// Test: dontIncrementIntOffsetForKeyedItems
namespace PsalmTest_array_assignment_47 {
    $a = [1, "a" => 2, 3];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{1, a: 2, 3}', $a);
}

// Test: assignArrayOrSetNull
namespace PsalmTest_array_assignment_48 {
    $a = [];

    if (rand(0, 1)) {
        $a[] = 4;
    }

    if (!$a) {
        $a = null;
    }

    assertType('list{4}|null', $a);
}

// Test: assignArrayOrSetNullInElseIf
namespace PsalmTest_array_assignment_49 {
    $a = [];

    if (rand(0, 1)) {
        $a[] = 4;
    }

    if ($a) {
    } elseif (rand(0, 1)) {
        $a = null;
    }

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{}|array{4}|null', $a);
}

// Test: assignArrayOrSetNullInElse
namespace PsalmTest_array_assignment_50 {
    $a = [];

    if (rand(0, 1)) {
        $a[] = 4;
    }

    if ($a) {
    } else {
        $a = null;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{4}|null', $a);
}

// Test: castToArray
namespace PsalmTest_array_assignment_51 {
    $a = (array) (rand(0, 1) ? [1 => "one"] : 0);
    $b = (array) null;

    // PHPantom keeps each branch's shape, where Psalm merges them into one with optional keys.
    assertType('array{1: "one"}|array{0}', $a);
    assertType('array<never, never>', $b);
}

// Test: getOnCoercedArray
namespace PsalmTest_array_assignment_52 {
    function getArray() : array {
        return rand(0, 1) ? ["attr" => []] : [];
    }

    $out = getArray();
    $out["attr"] = (array) ($out["attr"] ?? []);
    $out["attr"]["bar"] = 1;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1', $out['attr']['bar']);
}

// Test: listUsedAsArray
namespace PsalmTest_array_assignment_53 {
    function takesArray(array $arr) : void {}

    $a = [];
    $a[] = 1;
    $a[] = 2;

    takesArray($a);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{1, 2}', $a);
}

// Test: listTakesEmptyArray
namespace PsalmTest_array_assignment_54 {
    /** @param list<int> $arr */
    function takesList(array $arr) : void {}

    $a = [];

    takesList($a);

    assertType('array<never, never>', $a);
}

// Test: listCreatedInSingleStatementUsedAsArray
namespace PsalmTest_array_assignment_55 {
    function takesArray(array $arr) : void {}

    /** @param list<int> $arr */
    function takesList(array $arr) : void {}

    $a = [1, 2];

    takesArray($a);
    takesList($a);

    $a[] = 3;

    takesArray($a);
    takesList($a);

    $b = $a;

    $b[] = rand(0, 10);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{1, 2, 3}', $a);
}

// Test: arraySpread
namespace PsalmTest_array_assignment_56 {
    $arrayA = [1, 2, 3];
    $arrayB = [4, 5];
    $result = [0, ...$arrayA, ...$arrayB, 6 ,7];

    $arr1 = [3 => 1, 1 => 2, 3];
    $arr2 = [...$arr1];
    $arr3 = [1 => 0, ...$arr1];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{0, 1, 2, 3, 4, 5, 6, 7}', $result);
    assertType('array{1, 2, 3}', $arr2);
    assertType('array{1: 0, 2: 1, 3: 2, 4: 3}', $arr3);
}

// Test: arraySpreadWithString
// Requires PHP 8.1
namespace PsalmTest_array_assignment_57 {
    $x = [
        "a" => 0,
        ...["a" => 1],
        ...["b" => 2]
    ];

    assertType('array{a: 1, b: 2}', $x);
}

// Test: constantArraySpreadWithString
// Requires PHP 8.1
namespace PsalmTest_array_assignment_58 {
    class BaseClass {
        public const KEYS = [
            "a" => "a",
            "b" => "b",
        ];
    }

    class ChildClass extends BaseClass {
        public const A = [
            ...parent::KEYS,
            "c" => "c",
        ];
    }

    $a = ChildClass::A;

    assertType('array{a: \'a\', b: \'b\', c: \'c\'}', $a);
}

// Test: mergeWithNestedMixed
namespace PsalmTest_array_assignment_59 {
    function getArray() : array {
        return [];
    }

    $arr = getArray();

    if (rand(0, 1)) {
        /** @psalm-suppress MixedArrayAssignment */
        $arr["hello"]["goodbye"] = 5;
    }

    // PHPantom keeps the write's shape beside the declared `array`.
    assertType('array|array{hello: array{goodbye: 5}}', $arr);
}

// Test: manipulateArrayTwice
namespace PsalmTest_array_assignment_60 {
    /** @var array */
    $options = [];
    $options['a'] = 1;
    /** @psalm-suppress MixedArrayAssignment */
    $options['b']['c'] = 2;

    // PHPantom is more precise than Psalm here: the nested write gives `$options['b']` a known shape.
    assertType('array{c: 2}', $options['b']);
}

// Test: binaryOperation
namespace PsalmTest_array_assignment_61 {
    $a = array_map(
        function (string $x) {
            return new RuntimeException($x);
        },
        ["c" => ""]
    );

    $a += ["e" => new RuntimeException()];

    assertType('array{c: RuntimeException, e: RuntimeException}', $a);
}

// Test: unpackEmptyArrayIsEmpty
namespace PsalmTest_array_assignment_62 {
    $x = [];
    $y = [];

    $x = [...$x, ...$y];

    assertType('array<never, never>', $x);
}

// Test: unpackListCanBeEmpty
namespace PsalmTest_array_assignment_63 {
    /** @var list<int> */
    $x = [];
    /** @var list<int> */
    $y = [];

    $x = [...$x, ...$y];

    assertType('list<int>', $x);
}

// Test: unpackEmptyKeepsCorrectKeys
namespace PsalmTest_array_assignment_65 {
    $a = [];
    $b = [1];
    $c = [];
    $d = [2];

    $e = [...$a, ...$b, ...$c, ...$d, 3];

    assertType('list{1, 2, 3}', $e);
}

// Test: unpackArrayCanBeEmpty
// Requires PHP 8.1
namespace PsalmTest_array_assignment_66 {
    /** @var array<array-key, int> */
    $x = [];
    /** @var array<array-key, int> */
    $y = [];

    $x = [...$x, ...$y];

    assertType('array<array-key, int>', $x);
}

// Test: unpackIntKeyedArrayResultsInList
namespace PsalmTest_array_assignment_67 {
    /** @var array<int, int> */
    $x = [];
    /** @var array<int, int> */
    $y = [];

    $x = [...$x, ...$y];

    assertType('list<int>', $x);
}

// Test: unpackStringKeyedArrayPhp8.1
// Requires PHP 8.1
namespace PsalmTest_array_assignment_68 {
    /** @var array<string, int> */
    $x = [];
    /** @var array<array-key, int> */
    $y = [];

    $x = [...$x, ...$y];

    assertType('array<array-key, int>', $x);
}

// Test: unpackLiteralStringKeyedArrayPhp8.1
// Requires PHP 8.1
namespace PsalmTest_array_assignment_69 {
    /** @var array<"foo"|"bar", int> */
    $x = [];
    /** @var array<"baz", int> */
    $y = [];

    $x = [...$x, ...$y];

    assertType('array<\'bar\'|\'baz\'|\'foo\', int>', $x);
}

// Test: unpackArrayShapesUnionsLaterUnpacks
// Requires PHP 8.1
namespace PsalmTest_array_assignment_70 {
    $shape = ["foo" => 1, "bar" => 2, 10 => 3];
    /** @var array<int, 4> */
    $a = [];
    /** @var list<5> */
    $b = [];
    /** @var array<array-key, 6> */
    $c = [];

    $x = [...$a, ...$b, ...$c, ...$shape]; // Shape is last so it overrides previous
    $y = [...$shape, ...$a, ...$b, ...$c]; // Shape is first, but only possibly matching keys union their values

    // Psalm keeps `0: 3` in `$x` and widens it to `0: 3|4|5|6` in `$y`, but a
    // spread renumbers every integer key onto the end: the `3` lands after
    // whatever came before it in `$x`, and sits at `0` in `$y` because
    // nothing comes before it there.
    assertType('array{foo: 1, bar: 2, ...<array-key, 3|4|5|6>}', $x);
    assertType('array{foo: 1|6, bar: 2|6, 3, ...<array-key, 4|5|6>}', $y);
}

// Test: AddTwoSealedArrays
namespace PsalmTest_array_assignment_71 {
    final class Token
    {
        public const ONE = [
            16 => 16,
        ];

        public const TWO = [
            17 => 17,
        ];

        public const THREE = [
            18 => 18,
        ];
    }
    $_a = Token::ONE + Token::TWO + Token::THREE;

    assertType('array{16: 16, 17: 17, 18: 18}', $_a);
}

// Test: nullableDestructuring
// Requires PHP 8.1
namespace PsalmTest_array_assignment_72 {
    /**
     * @return array{"foo", "bar"}|null
     */
    function foobar(): ?array
    {
        return null;
    }

    [$_foo, $_bar] = foobar();

    assertType('\'foo\'|null', $_foo);
    assertType('\'bar\'|null', $_bar);
}

// Test: listAppendShape
namespace PsalmTest_array_assignment_73 {
    $a = [];
    $a[]= 0;
    $a[]= 1;
    $a[]= 2;

    $b = [0];
    $b[]= 1;
    $b[]= 2;

    assertType('list{0, 1, 2}', $a);
    assertType('list{0, 1, 2}', $b);
}
