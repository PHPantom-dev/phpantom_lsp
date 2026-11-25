<?php

namespace MethodPhpDocsNamespace;

use function PHPStan\Testing\assertType;

use SomeNamespace\Amet as Dolor;
use SomeNamespace\Consecteur;

class FooParent
{
	/**
	 * @return static
	 */
	public function doLorem()
	{
	}

	/**
	 * @return static
	 */
	public function doIpsum()
	{
	}

	/**
	 * @return $this
	 */
	public function doThis()
	{
	}

	/**
	 * @return $this|null
	 */
	public function doThisNullable()
	{
	}

	/**
	 * @return $this|Bar|null
	 */
	public function doThisUnion()
	{
	}
}

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

class Collection {}

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

class Foo extends FooParent
{
	/**
	 * @return Bar
	 */
	public static function doSomethingStatic()
	{
	}

	/**
	 * @return Foo
	 */
	public function doFoo()
	{
	}
}

class FooPhpstanPrefix extends FooParent
{

	/**
	 * @phpstan-return Bar
	 */
	public static function doSomethingStatic()
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
	 * @phpstan-param self $selfType
	 * @phpstan-param static $staticType
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
	 * @phpstan-param object $objectWithoutNativeTypehint
	 * @phpstan-param object $objectWithNativeTypehint
	 * @phpstan-return Foo
	 */
	public function doFoo(
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
		$selfType,
		$staticType,
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
		$objectWithoutNativeTypehint,
		object $objectWithNativeTypehint,
		$parameterWithDefaultValueFalse = false,
		$anotherNullableObject = null
	)
	{
		$parent = new FooParent();
		$differentInstance = new self();

		/** @phpstan-var self $inlineSelf */
		$inlineSelf = doFoo();

		/** @phpstan-var Bar $inlineBar */
		$inlineBar = doFoo();

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
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $selfType);
			// assertType('static(MethodPhpDocsNamespace\FooPhpstanPrefix)', $staticType);
			assertType('MethodPhpDocsNamespace\Foo', $this->doFoo());
			assertType('MethodPhpDocsNamespace\Bar', static::doSomethingStatic());
			// assertType('static(MethodPhpDocsNamespace\FooPhpstanPrefix)', parent::doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPhpstanPrefix)', $this->doLorem());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $differentInstance->doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPhpstanPrefix)', parent::doIpsum());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $differentInstance->doIpsum());
			// assertType('static(MethodPhpDocsNamespace\FooPhpstanPrefix)', $this->doIpsum());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $this->doBar()[0]);
			assertType('MethodPhpDocsNamespace\Bar', self::doSomethingStatic());
			assertType('MethodPhpDocsNamespace\Bar', \MethodPhpDocsNamespace\Foo::doSomethingStatic());
			// assertType('$this(MethodPhpDocsNamespace\FooPhpstanPrefix)', parent::doThis());
			// assertType('$this(MethodPhpDocsNamespace\FooPhpstanPrefix)|null', parent::doThisNullable());
			// assertType('$this(MethodPhpDocsNamespace\FooPhpstanPrefix)|MethodPhpDocsNamespace\Bar|null', parent::doThisUnion());
			assertType('array<null>', $this->returnNulls());
			assertType('object', $objectWithoutNativeTypehint);
			assertType('object', $objectWithNativeTypehint);
			assertType('object', $this->returnObject());
			assertType('MethodPhpDocsNamespace\FooParent', new parent());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $inlineSelf);
			assertType('MethodPhpDocsNamespace\Bar', $inlineBar);
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $this->phpDocVoidMethod());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $this->phpDocVoidMethodFromInterface());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $this->phpDocVoidParentMethod());
			assertType('MethodPhpDocsNamespace\FooPhpstanPrefix', $this->phpDocWithoutCurlyBracesVoidParentMethod());
			assertType('array<string>', $this->returnsStringArray());
			assertType('mixed', $this->privateMethodWithPhpDoc());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doLorem());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doIpsum());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnParent());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnPhpDocParent());
		}
	}

	/**
	 * @phpstan-return self[]
	 */
	public function doBar(): array
	{

	}

	public function returnParent(): parent
	{

	}

	/**
	 * @phpstan-return parent
	 */
	public function returnPhpDocParent()
	{

	}

	/**
	 * @phpstan-return NULL[]
	 */
	public function returnNulls(): array
	{

	}

	public function returnObject(): object
	{

	}

	public function phpDocVoidMethod(): self
	{

	}

	public function phpDocVoidMethodFromInterface(): self
	{

	}

	public function phpDocVoidParentMethod(): self
	{

	}

	public function phpDocWithoutCurlyBracesVoidParentMethod(): self
	{

	}

	/**
	 * @phpstan-return string[]
	 */
	public function returnsStringArray(): array
	{

	}

	private function privateMethodWithPhpDoc()
	{

	}

}
