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
 * @phan-param Foo|Bar $unionTypeParameter
 * @phan-param int $anotherMixedParameter
 * @phan-param int $anotherMixedParameter
 * @phan-paran int $yetAnotherMixedProperty
 * @phan-param int $integerParameter
 * @phan-param integer $anotherIntegerParameter
 * @phan-param aRray $arrayParameterOne
 * @phan-param mixed[] $arrayParameterOther
 * @phan-param Lorem $objectRelative
 * @phan-param \SomeOtherNamespace\Ipsum $objectFullyQualified
 * @phan-param Dolor $objectUsed
 * @phan-param null|int $nullableInteger
 * @phan-param Dolor|null $nullableObject
 * @phan-param Dolor $anotherNullableObject
 * @phan-param Null $nullType
 * @phan-param Bar $barObject
 * @phan-param Foo $conflictedObject
 * @phan-param Baz $moreSpecifiedObject
 * @phan-param resource $resource
 * @phan-param array[array] $yetAnotherAnotherMixedParameter
 * @phan-param \\Test\Bar $yetAnotherAnotherAnotherMixedParameter
 * @phan-param New $yetAnotherAnotherAnotherAnotherMixedParameter
 * @phan-param void $voidParameter
 * @phan-param Consecteur $useWithoutAlias
 * @phan-param true $true
 * @phan-param false $false
 * @phan-param true $boolTrue
 * @phan-param false $boolFalse
 * @phan-param bool $trueBoolean
 * @phan-param bool $parameterWithDefaultValueFalse
 * @phan-return Foo
 */
function doFooPhanPrefix(
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
