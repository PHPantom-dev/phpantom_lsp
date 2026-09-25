<?php
// Source: Psalm MethodCallTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: dateTimeImmutableStatic
namespace PsalmTest_method_call_1 {
    final class MyDate extends DateTimeImmutable {}

    $today = new MyDate();
    $yesterday = $today->sub(new DateInterval("P1D"));

    $b = (new DateTimeImmutable())->modify("+3 hours");

    assertType('PsalmTest_method_call_1\MyDate', $yesterday);
    // Psalm's expectation is for PHP < 8.4, where `modify()` could return
    // `false`; the runner analyses as PHP 8.5, where it throws instead.
    assertType('DateTimeImmutable', $b);
}

// Test: magicCall
namespace PsalmTest_method_call_2 {
    /** @psalm-no-seal-methods */
    class A {
        public function __call(string $method_name, array $args) : string {
            return "hello";
        }
    }

    $a = new A;
    $s = $a->bar();

    assertType('string', $s);
}

// Test: pdoStatementSetFetchMode
namespace PsalmTest_method_call_4 {
    class A {
        /** @var ?string */
        public $a;
    }
    class B extends A {}

    $db = new PDO("sqlite::memory:");
    $db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
    $db->setAttribute(PDO::ATTR_DEFAULT_FETCH_MODE, PDO::FETCH_ASSOC);
    $stmt = $db->prepare("select \"a\" as a");
    $stmt->setFetchMode(PDO::FETCH_CLASS, A::class);
    $stmt2 = $db->prepare("select \"a\" as a");
    $stmt2->setFetchMode(PDO::FETCH_ASSOC);
    $stmt3 = $db->prepare("select \"a\" as a");
    $stmt3->setFetchMode(PDO::ATTR_DEFAULT_FETCH_MODE);
    $stmt->execute();
    $stmt2->execute();
    /** @psalm-suppress MixedAssignment */
    $a = $stmt->fetch();
    $b = $stmt->fetchAll();
    $c = $stmt->fetch(PDO::FETCH_CLASS);
    $d = $stmt->fetchAll(PDO::FETCH_CLASS);
    $e = $stmt->fetchAll(PDO::FETCH_CLASS, B::class);
    $f = $stmt->fetch(PDO::FETCH_ASSOC);
    $g = $stmt->fetchAll(PDO::FETCH_ASSOC);
    /** @psalm-suppress MixedAssignment */
    $h = $stmt2->fetch();
    $i = $stmt2->fetchAll();
    $j = $stmt2->fetch(PDO::FETCH_BOTH);
    $k = $stmt2->fetchAll(PDO::FETCH_BOTH);
    /** @psalm-suppress MixedAssignment */
    $l = $stmt3->fetch();

    assertType('mixed', $h);
    assertType('mixed', $l);
}

// Test: parentMagicMethodCall
namespace PsalmTest_method_call_6 {
    /** @psalm-no-seal-methods */
    class Model {
        /**
         * @return static
         */
        public function __call(string $method, array $args) {
            /** @psalm-suppress UnsafeInstantiation */
            return new static;
        }
    }

    class BlahModel extends Model {
        /**
         * @param mixed $input
         */
        public function create($input): BlahModel
        {
            return parent::create([]);
        }
    }

    $m = new BlahModel();
    $n = $m->create([]);

    assertType('BlahModel', $n);
}

