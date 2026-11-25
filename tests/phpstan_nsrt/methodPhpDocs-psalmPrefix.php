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

class FooPsalmPrefix extends FooParent
{

	/**
	 * @psalm-return Bar
	 */
	public static function doSomethingStatic()
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
	 * @psalm-param self $selfType
	 * @psalm-param static $staticType
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
	 * @psalm-param object $objectWithoutNativeTypehint
	 * @psalm-param object $objectWithNativeTypehint
	 * @psalm-return Foo
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

		/** @psalm-var self $inlineSelf */
		$inlineSelf = doFoo();

		/** @psalm-var Bar $inlineBar */
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
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $selfType);
			// assertType('static(MethodPhpDocsNamespace\FooPsalmPrefix)', $staticType);
			assertType('MethodPhpDocsNamespace\Foo', $this->doFoo());
			assertType('MethodPhpDocsNamespace\Bar', static::doSomethingStatic());
			// assertType('static(MethodPhpDocsNamespace\FooPsalmPrefix)', parent::doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPsalmPrefix)', $this->doLorem());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $differentInstance->doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPsalmPrefix)', parent::doIpsum());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $differentInstance->doIpsum());
			// assertType('static(MethodPhpDocsNamespace\FooPsalmPrefix)', $this->doIpsum());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $this->doBar()[0]);
			assertType('MethodPhpDocsNamespace\Bar', self::doSomethingStatic());
			assertType('MethodPhpDocsNamespace\Bar', \MethodPhpDocsNamespace\Foo::doSomethingStatic());
			// assertType('$this(MethodPhpDocsNamespace\FooPsalmPrefix)', parent::doThis());
			// assertType('$this(MethodPhpDocsNamespace\FooPsalmPrefix)|null', parent::doThisNullable());
			// assertType('$this(MethodPhpDocsNamespace\FooPsalmPrefix)|MethodPhpDocsNamespace\Bar|null', parent::doThisUnion());
			assertType('array<null>', $this->returnNulls());
			assertType('object', $objectWithoutNativeTypehint);
			assertType('object', $objectWithNativeTypehint);
			assertType('object', $this->returnObject());
			assertType('MethodPhpDocsNamespace\FooParent', new parent());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $inlineSelf);
			assertType('MethodPhpDocsNamespace\Bar', $inlineBar);
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $this->phpDocVoidMethod());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $this->phpDocVoidMethodFromInterface());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $this->phpDocVoidParentMethod());
			assertType('MethodPhpDocsNamespace\FooPsalmPrefix', $this->phpDocWithoutCurlyBracesVoidParentMethod());
			assertType('array<string>', $this->returnsStringArray());
			assertType('mixed', $this->privateMethodWithPhpDoc());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doLorem());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doIpsum());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnParent());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnPhpDocParent());
		}
	}

	/**
	 * @psalm-return self[]
	 */
	public function doBar(): array
	{

	}

	public function returnParent(): parent
	{

	}

	/**
	 * @psalm-return parent
	 */
	public function returnPhpDocParent()
	{

	}

	/**
	 * @psalm-return NULL[]
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
	 * @psalm-return string[]
	 */
	public function returnsStringArray(): array
	{

	}

	private function privateMethodWithPhpDoc()
	{

	}

}
