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
 * @phpstan-param Foo|Bar $unionTypeParameter
 * @phpstan-param int $anotherMixedParameter
 * @phpstan-param int $anotherMixedParameter
 * @phpstan-paran int $yetAnotherMixedProperty
 * @phpstan-param int $integerParameter
 * @phpstan-param integer $anotherIntegerParameter
 * @phpstan-param aRray $arrayParameterOne
 * @phpstan-param mixed[] $arrayParameterOther
 * @phpstan-param Lorem $objectRelative
 * @phpstan-param \SomeOtherNamespace\Ipsum $objectFullyQualified
 * @phpstan-param Dolor $objectUsed
 * @phpstan-param null|int $nullableInteger
 * @phpstan-param Dolor|null $nullableObject
 * @phpstan-param Dolor $anotherNullableObject
 * @phpstan-param Null $nullType
 * @phpstan-param Bar $barObject
 * @phpstan-param Foo $conflictedObject
 * @phpstan-param Baz $moreSpecifiedObject
 * @phpstan-param resource $resource
 * @phpstan-param array[array] $yetAnotherAnotherMixedParameter
 * @phpstan-param \\Test\Bar $yetAnotherAnotherAnotherMixedParameter
 * @phpstan-param New $yetAnotherAnotherAnotherAnotherMixedParameter
 * @phpstan-param void $voidParameter
 * @phpstan-param Consecteur $useWithoutAlias
 * @phpstan-param true $true
 * @phpstan-param false $false
 * @phpstan-param true $boolTrue
 * @phpstan-param false $boolFalse
 * @phpstan-param bool $trueBoolean
 * @phpstan-param bool $parameterWithDefaultValueFalse
 * @phpstan-return Foo
 */
function doFooPhpstanPrefix(
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
