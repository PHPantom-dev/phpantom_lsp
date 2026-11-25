<?php

namespace MethodPhpDocsNamespace;

use function PHPStan\Testing\assertType;

use SomeNamespace\Amet as Dolor;
use SomeNamespace\Consecteur;

class Bar
{
	/**
	 * @return Bar
	 */
	public function doBar()
	{
	}
}

class Lorem {}

class Baz extends Bar
{
	/**
	 * @return self
	 */
	public function doFluent()
	{
	}

	/**
	 * @return self|null
	 */
	public function doFluentNullable()
	{
	}

	/**
	 * @return self[]
	 */
	public function doFluentArray(): array
	{
	}

	/**
	 * @return Collection&iterable<self>
	 */
	public function doFluentUnionIterable()
	{
	}
}

class Collection {}

class Foo {}

/**
 * @return Foo
 */
function doFoo()
{
}

/**
 * @psalm-param Foo|Bar $unionTypeParameter
 * @psalm-param int $anotherMixedParameter
 * @psalm-param int $anotherMixedParameter
 * @psalm-paran int $yetAnotherMixedProperty
 * @psalm-param int $integerParameter
 * @psalm-param integer $anotherIntegerParameter
 * @psalm-param aRray $arrayParameterOne
 * @psalm-param mixed[] $arrayParameterOther
 * @psalm-param Lorem $objectRelative
 * @psalm-param \SomeOtherNamespace\Ipsum $objectFullyQualified
 * @psalm-param Dolor $objectUsed
 * @psalm-param null|int $nullableInteger
 * @psalm-param Dolor|null $nullableObject
 * @psalm-param Dolor $anotherNullableObject
 * @psalm-param Null $nullType
 * @psalm-param Bar $barObject
 * @psalm-param Foo $conflictedObject
 * @psalm-param Baz $moreSpecifiedObject
 * @psalm-param resource $resource
 * @psalm-param array[array] $yetAnotherAnotherMixedParameter
 * @psalm-param \\Test\Bar $yetAnotherAnotherAnotherMixedParameter
 * @psalm-param New $yetAnotherAnotherAnotherAnotherMixedParameter
 * @psalm-param void $voidParameter
 * @psalm-param Consecteur $useWithoutAlias
 * @psalm-param true $true
 * @psalm-param false $false
 * @psalm-param true $boolTrue
 * @psalm-param false $boolFalse
 * @psalm-param bool $trueBoolean
 * @psalm-param bool $parameterWithDefaultValueFalse
 * @psalm-return Foo
 */
function doFooPsalmPrefix(
	$mixedParameter,
	$unionTypeParameter,
	$anotherMixedParameter,
	$yetAnotherMixedParameter,
	$integerParameter,
	$anotherIntegerParameter,
	$arrayParameterOne,
	$arrayParameterOther,
	$objectRelative,
	$objectFullyQualified,
	$objectUsed,
	$nullableInteger,
	$nullableObject,
	$nullType,
	$barObject,
	Bar $conflictedObject,
	Bar $moreSpecifiedObject,
	$resource,
	$yetAnotherAnotherMixedParameter,
	$yetAnotherAnotherAnotherMixedParameter,
	$yetAnotherAnotherAnotherAnotherMixedParameter,
	$voidParameter,
	$useWithoutAlias,
	$true,
	$false,
	bool $boolTrue,
	bool $boolFalse,
	bool $trueBoolean,
	$parameterWithDefaultValueFalse = false,
	$anotherNullableObject = null
)
{
	$fooFunctionResult = doFoo();

	foreach ($moreSpecifiedObject->doFluentUnionIterable() as $fluentUnionIterableBaz) {
		assertType('mixed', $mixedParameter);
		assertType('MethodPhpDocsNamespace\Bar|MethodPhpDocsNamespace\Foo', $unionTypeParameter);
		assertType('int', $anotherMixedParameter);
		assertType('mixed', $yetAnotherMixedParameter);
		assertType('int', $integerParameter);
		assertType('int', $anotherIntegerParameter);
		assertType('array', $arrayParameterOne);
		assertType('array<mixed>', $arrayParameterOther);
		assertType('MethodPhpDocsNamespace\Lorem', $objectRelative);
		assertType('SomeOtherNamespace\Ipsum', $objectFullyQualified);
		assertType('SomeNamespace\Amet', $objectUsed);
		assertType('*ERROR*', $nonexistentParameter);
		assertType('int|null', $nullableInteger);
		assertType('SomeNamespace\Amet|null', $nullableObject);
		assertType('SomeNamespace\Amet|null', $anotherNullableObject);
		assertType('null', $nullType);
		assertType('MethodPhpDocsNamespace\Bar', $barObject->doBar());
		assertType('MethodPhpDocsNamespace\Bar', $conflictedObject);
		assertType('MethodPhpDocsNamespace\Baz', $moreSpecifiedObject);
		assertType('MethodPhpDocsNamespace\Baz', $moreSpecifiedObject->doFluent());
		assertType('MethodPhpDocsNamespace\Baz|null', $moreSpecifiedObject->doFluentNullable());
		assertType('MethodPhpDocsNamespace\Baz', $moreSpecifiedObject->doFluentArray()[0]);
		assertType('iterable<MethodPhpDocsNamespace\Baz>&MethodPhpDocsNamespace\Collection', $moreSpecifiedObject->doFluentUnionIterable());
		assertType('MethodPhpDocsNamespace\Baz', $fluentUnionIterableBaz);
		assertType('resource', $resource);
		assertType('mixed', $yetAnotherAnotherAnotherMixedParameter);
		assertType('void', $voidParameter);
		assertType('SomeNamespace\Consecteur', $useWithoutAlias);
		assertType('true', $true);
		assertType('false', $false);
		assertType('true', $boolTrue);
		assertType('false', $boolFalse);
		assertType('bool', $trueBoolean);
		assertType('bool', $parameterWithDefaultValueFalse);
		assertType('MethodPhpDocsNamespace\Foo', $fooFunctionResult);
	}
}
