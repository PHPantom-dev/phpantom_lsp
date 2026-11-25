<?php

namespace MethodPhpDocsNamespace;

use function PHPStan\Testing\assertType;

use SomeNamespace\Amet as Dolor;
use SomeNamespace\Consecteur;

class FooWithTrait extends FooParent
{

	use FooTrait;

	/**
	 * @return Bar
	 */
	public static function doSomethingStatic()
	{

	}

	/**
	 * @return self[]
	 */
	public function doBar(): array
	{

	}

	public function returnParent(): parent
	{

	}

	/**
	 * @return parent
	 */
	public function returnPhpDocParent()
	{

	}

	/**
	 * @return NULL[]
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
	 * @return string[]
	 */
	public function returnsStringArray(): array
	{

	}

	private function privateMethodWithPhpDoc()
	{

	}

}

function (FooWithTrait $foo): void {
	$parent = new FooParent();
	assertType('MethodPhpDocsNamespace\FooParent', $parent->doLorem());
	assertType('MethodPhpDocsNamespace\FooParent', $parent->doIpsum());
	assertType('MethodPhpDocsNamespace\FooParent', $foo->returnParent());
	assertType('MethodPhpDocsNamespace\FooParent', $foo->returnPhpDocParent());
};





trait FooTrait
{

	/**
	 * @param Foo|Bar $unionTypeParameter
	 * @param int $anotherMixedParameter
	 * @param int $anotherMixedParameter
	 * @paran int $yetAnotherMixedProperty
	 * @param int $integerParameter
	 * @param integer $anotherIntegerParameter
	 * @param aRray $arrayParameterOne
	 * @param mixed[] $arrayParameterOther
	 * @param Lorem $objectRelative
	 * @param \SomeOtherNamespace\Ipsum $objectFullyQualified
	 * @param Dolor $objectUsed
	 * @param null|int $nullableInteger
	 * @param Dolor|null $nullableObject
	 * @param Dolor $anotherNullableObject
	 * @param self $selfType
	 * @param static $staticType
	 * @param Null $nullType
	 * @param Bar $barObject
	 * @param Foo $conflictedObject
	 * @param Baz $moreSpecifiedObject
	 * @param resource $resource
	 * @param array[array] $yetAnotherAnotherMixedParameter
	 * @param \\Test\Bar $yetAnotherAnotherAnotherMixedParameter
	 * @param New $yetAnotherAnotherAnotherAnotherMixedParameter
	 * @param void $voidParameter
	 * @param Consecteur $useWithoutAlias
	 * @param true $true
	 * @param false $false
	 * @param true $boolTrue
	 * @param false $boolFalse
	 * @param bool $trueBoolean
	 * @param bool $parameterWithDefaultValueFalse
	 * @param object $objectWithoutNativeTypehint
	 * @param object $objectWithNativeTypehint
	 * @return Foo
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

		/** @var self $inlineSelf */
		$inlineSelf = doFoo();

		/** @var Bar $inlineBar */
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
		assertType('iterable<MethodPhpDocsNamespace\Baz>&MethodPhpDocsNamespace\Collection', $moreSpecifiedObject->doFluentUnionIterable()); // SKIP: `static[]|Collection` on a native `Collection` return is not read as a collection of the class
		assertType('MethodPhpDocsNamespace\Baz', $fluentUnionIterableBaz); // SKIP: `static[]|Collection` on a native `Collection` return is not read as a collection of the class
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
		assertType('MethodPhpDocsNamespace\FooWithTrait', $selfType); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('static(MethodPhpDocsNamespace\FooWithTrait)', $staticType); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\Foo', $this->doFoo());
		assertType('MethodPhpDocsNamespace\Bar', static::doSomethingStatic()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('static(MethodPhpDocsNamespace\FooWithTrait)', parent::doLorem()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('static(MethodPhpDocsNamespace\FooWithTrait)', $this->doLorem()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $differentInstance->doLorem()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('static(MethodPhpDocsNamespace\FooWithTrait)', parent::doIpsum()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $differentInstance->doIpsum()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('static(MethodPhpDocsNamespace\FooWithTrait)', $this->doIpsum()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $this->doBar()[0]);
		assertType('MethodPhpDocsNamespace\Bar', self::doSomethingStatic()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\Bar', \MethodPhpDocsNamespace\Foo::doSomethingStatic());
		assertType('$this(MethodPhpDocsNamespace\FooWithTrait)', parent::doThis()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('$this(MethodPhpDocsNamespace\FooWithTrait)|null', parent::doThisNullable()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('$this(MethodPhpDocsNamespace\FooWithTrait)|MethodPhpDocsNamespace\Bar|null', parent::doThisUnion()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('array<null>', $this->returnNulls());
		assertType('object', $objectWithoutNativeTypehint);
		assertType('object', $objectWithNativeTypehint);
		assertType('object', $this->returnObject());
		assertType('MethodPhpDocsNamespace\FooParent', new parent()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $inlineSelf); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\Bar', $inlineBar);
		assertType('MethodPhpDocsNamespace\FooWithTrait', $this->phpDocVoidMethod()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $this->phpDocVoidMethodFromInterface()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $this->phpDocVoidParentMethod()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('MethodPhpDocsNamespace\FooWithTrait', $this->phpDocWithoutCurlyBracesVoidParentMethod()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		assertType('array<string>', $this->returnsStringArray());
		assertType('mixed', $this->privateMethodWithPhpDoc()); // SKIP: inside a trait, `self`, `static`, `parent::` and `$this` do not stand for the class using it
		}
	}

}




class Bar
{

	/**
	 * @return self
	 */
	public function doBar()
	{

	}

	/**
	 * @return static
	 */
	public function doFluent()
	{

	}

	/**
	 * @return static|null
	 */
	public function doFluentNullable()
	{

	}

	/**
	 * @return static[]
	 */
	public function doFluentArray(): array
	{

	}

	/**
	 * @return static[]|Collection
	 */
	public function doFluentUnionIterable(): Collection
	{

	}

}




interface FooInterface
{

	/**
	 * @return void
	 */
	public function phpDocVoidMethodFromInterface();

}

class FooParentParent
{

	/**
	 * @return void
	 */
	public function phpDocVoidParentMethod()
	{

	}

	/**
	 * @return void
	 */
	public function phpDocWithoutCurlyBracesVoidParentMethod()
	{

	}

}

abstract class FooParent extends FooParentParent implements FooInterface
{

	/**
	 * @return Static
	 */
	public function doLorem()
	{

	}

	/**
	 * @return static
	 */
	public function doIpsum(): self
	{

	}

	/**
	 * @return $this
	 */
	public function doThis()
	{
		return $this;
	}

	/**
	 * @return $this|null
	 */
	public function doThisNullable()
	{
		return $this;
	}

	/**
	 * @return $this|Bar|null
	 */
	public function doThisUnion()
	{

	}

	/**
	 * @return void
	 */
	public function phpDocVoidMethod()
	{

	}

	/**
	 * {@inheritDoc}
	 */
	public function phpDocVoidParentMethod()
	{

	}

	/**
	 * @inheritDoc
	 */
	public function phpDocWithoutCurlyBracesVoidParentMethod()
	{

	}

	/**
	 * @return string[]
	 */
	private function privateMethodWithPhpDoc()
	{

	}

}




class Baz extends Bar
{

}



interface Collection extends \Traversable
{

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
	 * @param Foo|Bar $unionTypeParameter
	 * @param int $anotherMixedParameter
	 * @param int $anotherMixedParameter
	 * @paran int $yetAnotherMixedProperty
	 * @param int $integerParameter
	 * @param integer $anotherIntegerParameter
	 * @param aRray $arrayParameterOne
	 * @param mixed[] $arrayParameterOther
	 * @param Lorem $objectRelative
	 * @param \SomeOtherNamespace\Ipsum $objectFullyQualified
	 * @param Dolor $objectUsed
	 * @param null|int $nullableInteger
	 * @param Dolor|null $nullableObject
	 * @param Dolor $anotherNullableObject
	 * @param self $selfType
	 * @param static $staticType
	 * @param Null $nullType
	 * @param Bar $barObject
	 * @param Foo $conflictedObject
	 * @param Baz $moreSpecifiedObject
	 * @param resource $resource
	 * @param array[array] $yetAnotherAnotherMixedParameter
	 * @param \\Test\Bar $yetAnotherAnotherAnotherMixedParameter
	 * @param New $yetAnotherAnotherAnotherAnotherMixedParameter
	 * @param void $voidParameter
	 * @param Consecteur $useWithoutAlias
	 * @param true $true
	 * @param false $false
	 * @param true $boolTrue
	 * @param false $boolFalse
	 * @param bool $trueBoolean
	 * @param bool $parameterWithDefaultValueFalse
	 * @param object $objectWithoutNativeTypehint
	 * @param object $objectWithNativeTypehint
	 * @return Foo
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

		/** @var self $inlineSelf */
		$inlineSelf = doFoo();

		/** @var Bar $inlineBar */
		$inlineBar = doFoo();

		foreach ($moreSpecifiedObject->doFluentUnionIterable() as $fluentUnionIterableBaz) {
		}
	}

	/**
	 * @return self[]
	 */
	public function doBar(): array
	{

	}

	public function returnParent(): parent
	{

	}

	/**
	 * @return parent
	 */
	public function returnPhpDocParent()
	{

	}

	/**
	 * @return NULL[]
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
	 * @return string[]
	 */
	public function returnsStringArray(): array
	{

	}

	private function privateMethodWithPhpDoc()
	{

	}

}
