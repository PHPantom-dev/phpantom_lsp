<?php

namespace NestedGenericTypesUnwrappingCovariant;

use function PHPStan\Testing\assertType;

interface BasePackage {}

interface InnerPackage extends BasePackage {}

/**
 * @template-covariant TInnerPackage of InnerPackage
 */
interface GenericPackage extends BasePackage {
	/** @return TInnerPackage */
	public function unwrap() : InnerPackage;
}

interface SomeInnerPackage extends InnerPackage {}

/**
 * @extends GenericPackage<SomeInnerPackage>
 */
interface SomePackage extends GenericPackage {}

/**
 * @template TInnerPackage of InnerPackage
 * @template TGenericPackage of GenericPackage<TInnerPackage>
 * @param TGenericPackage $package
 * @return TInnerPackage
 */
function unwrapGeneric(GenericPackage $package) {
	$result = $package->unwrap();
	assertType('TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\unwrapGeneric(), argument)', $result);

	return $result;
}

/**
 * @template TInnerPackage of InnerPackage
 * @template TGenericPackage of GenericPackage<TInnerPackage>
 * @param TGenericPackage $package
 * @return TGenericPackage
 */
function unwrapGeneric2(GenericPackage $package) {
	assertType('TGenericPackage of NestedGenericTypesUnwrappingCovariant\GenericPackage<TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\unwrapGeneric2(), argument)> (function NestedGenericTypesUnwrappingCovariant\unwrapGeneric2(), argument)', $package);

	return $package;
}

/**
 * @template TInnerPackage of InnerPackage
 * @template TGenericPackage of GenericPackage<TInnerPackage>
 * @param  class-string<TGenericPackage> $class  FQCN to be instantiated
 * @return TInnerPackage
 */
function loadWithDirectUnwrap(string $class) {
	$package = new $class();
	$result = $package->unwrap();
	assertType('TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\loadWithDirectUnwrap(), argument)', $result);

	return $result;
}

/**
 * @template TInnerPackage of InnerPackage
 * @template TGenericPackage of GenericPackage<TInnerPackage>
 * @param  class-string<TGenericPackage> $class  FQCN to be instantiated
 * @return TInnerPackage
 */
function loadWithIndirectUnwrap(string $class) {
	$package = new $class();
	$result = unwrapGeneric($package);
	assertType('TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\loadWithIndirectUnwrap(), argument)', $result);

	return $result;
}

/**
 * @template TInnerPackage of InnerPackage
 * @template TGenericPackage of GenericPackage<TInnerPackage>
 * @param  class-string<TGenericPackage> $class  FQCN to be instantiated
 * @return TGenericPackage
 */
function loadWithIndirectUnwrap2(string $class) {
	$package = new $class();
	assertType('TGenericPackage of NestedGenericTypesUnwrappingCovariant\GenericPackage<TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\loadWithIndirectUnwrap2(), argument)> (function NestedGenericTypesUnwrappingCovariant\loadWithIndirectUnwrap2(), argument)', $package);
	$result = unwrapGeneric2($package);
	assertType('TGenericPackage of NestedGenericTypesUnwrappingCovariant\GenericPackage<TInnerPackage of NestedGenericTypesUnwrappingCovariant\InnerPackage (function NestedGenericTypesUnwrappingCovariant\loadWithIndirectUnwrap2(), argument)> (function NestedGenericTypesUnwrappingCovariant\loadWithIndirectUnwrap2(), argument)', $result);

	return $result;
}

function (): void {
	$result = loadWithDirectUnwrap(SomePackage::class);
	assertType(SomeInnerPackage::class, $result);
};

function (): void {
	$result = loadWithIndirectUnwrap(SomePackage::class);
	assertType(SomeInnerPackage::class, $result);
};

function (): void {
	$result = loadWithIndirectUnwrap2(SomePackage::class);
	assertType(SomePackage::class, $result);
};

function (SomePackage $somePackage): void {
	$result = unwrapGeneric($somePackage);
	assertType(SomeInnerPackage::class, $result);

	$result = unwrapGeneric2($somePackage);
	assertType(SomePackage::class, $result);
};
