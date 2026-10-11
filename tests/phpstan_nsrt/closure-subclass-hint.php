<?php

namespace ClosureSubclassHint;

use function PHPStan\Testing\assertType;

interface Shape {}
/** @template T */
class Base implements Shape {}
/**
 * @template T
 * @extends Base<T>
 */
class Child extends Base {}
/** @template T */
class Unrelated {}

/** @param callable(Child<int>): void $cb */
function takeChild(callable $cb): void {}
/** @param callable(Base<int>): void $cb */
function takeBase(callable $cb): void {}
/** @param callable(Unrelated<int>): void $cb */
function takeUnrelated(callable $cb): void {}

function hints(): void {
    takeChild(function (Base $b) { assertType('ClosureSubclassHint\Child<int>', $b); });
    takeChild(function (Shape $s) { assertType('ClosureSubclassHint\Child<int>', $s); });
    takeBase(function (Child $c) { assertType('ClosureSubclassHint\Child', $c); });
    takeUnrelated(function (Base $b) { assertType('ClosureSubclassHint\Base', $b); });
}
