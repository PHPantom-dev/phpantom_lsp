<?php

namespace Bug3158;


use function PHPStan\Testing\assertType;

/**
 * @template T as object
 * @param class-string<T> $className
 * @param \Closure(T):void $outmaker
 * @return T
 */
function createProxy(
	string $className,
	\Closure $outmaker
) : object {
	$t = new $className();
	$outmaker($t);
	return $t;
}

/**
 * @template T as object
 * @param \Closure(T, T):void $outmaker
 * @return T
 */
function createProxy2(
	\Closure $outmaker
) : object {

}

class AAParent {}

class AParent extends AAParent {}

class A extends AParent {
	public function bar() : void {}
}

class B extends A {

}

function (): void {
	$proxy = createProxy(A::class, function(AParent $o):void {});
	assertType(A::class, $proxy);

	$proxy = createProxy(A::class, function($o):void {});
	assertType(A::class, $proxy);

	$proxy = createProxy(A::class, function(B $o):void {});
	assertType(A::class, $proxy);

	$proxy = createProxy2(function(A $a, B $o):void {});
	assertType(B::class, $proxy); // SKIP: a template inferred from several closure parameters takes the first, not the narrowest
};

function (): void {
	/** @var object $object */
	$object = doFoo();
	$objectClass = get_class($object);
	assertType('class-string<object>', $objectClass); // PHPStan spells the same type `class-string`
	$proxy = createProxy($objectClass, function(AParent $o):void {});
	assertType('object', $proxy);
};
