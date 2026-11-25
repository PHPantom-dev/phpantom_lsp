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

class FooPhanPrefix extends FooParent
{

	/**
	 * @phan-return Bar
	 */
	public static function doSomethingStatic()
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
	 * @phan-param self $selfType
	 * @phan-param static $staticType
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
	 * @phan-param object $objectWithoutNativeTypehint
	 * @phan-param object $objectWithNativeTypehint
	 * @phan-return Foo
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

		/** @phan-var self $inlineSelf */
		$inlineSelf = doFoo();

		/** @phan-var Bar $inlineBar */
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
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $selfType);
			// assertType('static(MethodPhpDocsNamespace\FooPhanPrefix)', $staticType);
			assertType('MethodPhpDocsNamespace\Foo', $this->doFoo());
			assertType('MethodPhpDocsNamespace\Bar', static::doSomethingStatic());
			// assertType('static(MethodPhpDocsNamespace\FooPhanPrefix)', parent::doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPhanPrefix)', $this->doLorem());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $differentInstance->doLorem());
			// assertType('static(MethodPhpDocsNamespace\FooPhanPrefix)', parent::doIpsum());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $differentInstance->doIpsum());
			// assertType('static(MethodPhpDocsNamespace\FooPhanPrefix)', $this->doIpsum());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $this->doBar()[0]);
			assertType('MethodPhpDocsNamespace\Bar', self::doSomethingStatic());
			assertType('MethodPhpDocsNamespace\Bar', \MethodPhpDocsNamespace\Foo::doSomethingStatic());
			// assertType('$this(MethodPhpDocsNamespace\FooPhanPrefix)', parent::doThis());
			// assertType('$this(MethodPhpDocsNamespace\FooPhanPrefix)|null', parent::doThisNullable());
			// assertType('$this(MethodPhpDocsNamespace\FooPhanPrefix)|MethodPhpDocsNamespace\Bar|null', parent::doThisUnion());
			assertType('array<null>', $this->returnNulls());
			assertType('object', $objectWithoutNativeTypehint);
			assertType('object', $objectWithNativeTypehint);
			assertType('object', $this->returnObject());
			assertType('MethodPhpDocsNamespace\FooParent', new parent());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $inlineSelf);
			assertType('MethodPhpDocsNamespace\Bar', $inlineBar);
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $this->phpDocVoidMethod());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $this->phpDocVoidMethodFromInterface());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $this->phpDocVoidParentMethod());
			assertType('MethodPhpDocsNamespace\FooPhanPrefix', $this->phpDocWithoutCurlyBracesVoidParentMethod());
			assertType('array<string>', $this->returnsStringArray());
			assertType('mixed', $this->privateMethodWithPhpDoc());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doLorem());
			assertType('MethodPhpDocsNamespace\FooParent', $parent->doIpsum());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnParent());
			assertType('MethodPhpDocsNamespace\FooParent', $this->returnPhpDocParent());
		}
	}

	/**
	 * @phan-return self[]
	 */
	public function doBar(): array
	{

	}

	public function returnParent(): parent
	{

	}

	/**
	 * @phan-return parent
	 */
	public function returnPhpDocParent()
	{

	}

	/**
	 * @phan-return NULL[]
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
	 * @phan-return string[]
	 */
	public function returnsStringArray(): array
	{

	}

	private function privateMethodWithPhpDoc()
	{

	}

}
