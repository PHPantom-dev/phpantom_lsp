<?php // lint >= 8.0

declare(strict_types=1);

namespace Issue5511;

use function PHPStan\Testing\assertType;

#[\Attribute]
class Abc
{
}

/**
 * @param string $str
 * @param class-string $className
 * @param class-string<Abc> $genericClassName
 */
function testGetAttributes(
	\ReflectionClass $reflectionClass,
	\ReflectionMethod $reflectionMethod,
	\ReflectionParameter $reflectionParameter,
	\ReflectionProperty $reflectionProperty,
	\ReflectionClassConstant $reflectionClassConstant,
	\ReflectionFunction $reflectionFunction,
	string $str,
	string $className,
	string $genericClassName
): void
{
	$classAll = $reflectionClass->getAttributes();
	$classAbc1 = $reflectionClass->getAttributes(Abc::class);
	$classAbc2 = $reflectionClass->getAttributes(Abc::class, \ReflectionAttribute::IS_INSTANCEOF);
	$classGCN = $reflectionClass->getAttributes($genericClassName);
	$classCN = $reflectionClass->getAttributes($className);
	$classStr = $reflectionClass->getAttributes($str);
	$classNonsense = $reflectionClass->getAttributes("some random string");

	assertType('list<ReflectionAttribute<object>>', $classAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $classAbc1);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $classAbc2);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $classGCN);
	assertType('list<ReflectionAttribute<object>>', $classCN);
	assertType('list<ReflectionAttribute<object>>', $classStr);
	assertType('list<ReflectionAttribute<*ERROR*>>', $classNonsense); // SKIP: a getAttributes() filter naming no class is not reported as an error type

	$methodAll = $reflectionMethod->getAttributes();
	$methodAbc = $reflectionMethod->getAttributes(Abc::class);
	assertType('list<ReflectionAttribute<object>>', $methodAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $methodAbc);

	$paramAll = $reflectionParameter->getAttributes();
	$paramAbc = $reflectionParameter->getAttributes(Abc::class);
	assertType('list<ReflectionAttribute<object>>', $paramAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $paramAbc);

	$propAll = $reflectionProperty->getAttributes();
	$propAbc = $reflectionProperty->getAttributes(Abc::class);
	assertType('list<ReflectionAttribute<object>>', $propAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $propAbc);

	$constAll = $reflectionClassConstant->getAttributes();
	$constAbc = $reflectionClassConstant->getAttributes(Abc::class);
	assertType('list<ReflectionAttribute<object>>', $constAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $constAbc);

	$funcAll = $reflectionFunction->getAttributes();
	$funcAbc = $reflectionFunction->getAttributes(Abc::class);
	assertType('list<ReflectionAttribute<object>>', $funcAll);
	assertType('list<ReflectionAttribute<Issue5511\Abc>>', $funcAbc);
}

/**
 * @param \ReflectionAttribute<Abc> $ra
 */
function testNewInstance(\ReflectionAttribute $ra): void
{
	assertType('ReflectionAttribute<Issue5511\\Abc>', $ra);
	$abc = $ra->newInstance();
	assertType(Abc::class, $abc);
}
