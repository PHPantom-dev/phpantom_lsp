<?php
// Source: Psalm PropertyTypeTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: sharedPropertyInIf
namespace PsalmTest_property_type_1 {
    class A {
        /** @var int */
        public $foo = 0;
    }
    class B {
        /** @var string */
        public $foo = "";
    }

    $a = rand(0, 10) ? new A(): (rand(0, 10) ? new B() : null);
    $b = null;

    if ($a instanceof A || $a instanceof B) {
        $b = $a->foo;
    }

    assertType('int|null|string', $b);
}

// Test: sharedPropertyInElseIf
namespace PsalmTest_property_type_2 {
    class A {
        /** @var int */
        public $foo = 0;
    }
    class B {
        /** @var string */
        public $foo = "";
    }

    $a = rand(0, 10) ? new A() : new B();
    if (rand(0, 1)) {
        $a = null;
    }
    $b = null;

    if (rand(0, 10) === 4) {
        // do nothing
    }
    elseif ($a instanceof A || $a instanceof B) {
        $b = $a->foo;
    }

    assertType('int|null|string', $b);
}

// Test: grandparentReflectedProperties
namespace PsalmTest_property_type_3 {
    $a = new DOMElement("foo");
    $owner = $a->ownerDocument;

    assertType('DOMDocument|null', $owner);
}

// Test: selfPropertyType
namespace PsalmTest_property_type_4 {
    class Node
    {
        /** @var self|null */
        public $next;

        public function __construct() {
            if (rand(0, 1)) {
                $this->next = new Node();
            }
        }
    }

    $node = new Node();
    $next = $node->next;

    assertType('Node|null', $next);
}

// Test: setPropertiesOfStdClass
namespace PsalmTest_property_type_5 {
    $a = new stdClass();
    $a->b = "c";

    assertType('stdClass', $a);
}

// Test: getPropertiesOfSimpleXmlElement
namespace PsalmTest_property_type_6 {
    $a = new SimpleXMLElement("<person><child role=\"son\"></child></person>");
    $b = $a->b;

    assertType('SimpleXMLElement', $a);
    assertType('SimpleXMLElement', $b);
}

// Test: staticVarSelf
namespace PsalmTest_property_type_7 {
    class Foo {
        /** @var self */
        public static $current;
    }

    $a = Foo::$current;

    assertType('Foo', $a);
}

// Test: intersectionPropertyAccess
namespace PsalmTest_property_type_8 {
    /** @property int $test1 */
    class a {
        public function __get(string $name)
        {
            return 0;
        }
    }

    /** @var a&object{test2: "lmao"} */
    $r = null;

    $test1 = $r->test1;
    $test2 = $r->test2;

    assertType('int', $test1);
    assertType('\'lmao\'', $test2);
}
