<?php declare(strict_types=1);

namespace Bug14012;

use PhpParser\Node;
use PHPStan\Analyser\Scope;
use function PHPStan\Testing\assertType;

final class ExpectationMethodResolver
{
	/**
	 * @var array{
	 *   hasMethod: \Closure(Scope, Node\Arg, Node\Arg): Node\Expr,
	 *   hasProperty: \Closure(Scope, Node\Arg, Node\Arg): Node\Expr,
	 *   isArray: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isBool: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isCallable: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isCountable: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isFalse: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isFloat: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isInstanceOf: \Closure(Scope, Node\Arg, Node\Arg): Node\Expr,
	 *   isInt: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isIterable: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isList: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isMap: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isNaturalInt: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isNegativeInt: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isNonEmptyString: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isNull: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isNumeric: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isObject: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isPositiveInt: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isResource: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isSameAs: \Closure(Scope, Node\Arg, Node\Arg): Node\Expr,
	 *   isScalar: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isString: \Closure(Scope, Node\Arg): Node\Expr,
	 *   isTrue: \Closure(Scope, Node\Arg): Node\Expr,
	 * }
	 */
	public static array $resolvers = [];
}

assertType("array{hasMethod: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg, PhpParser\Node\Arg): PhpParser\Node\Expr, hasProperty: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg, PhpParser\Node\Arg): PhpParser\Node\Expr, isArray: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isBool: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isCallable: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isCountable: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isFalse: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isFloat: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isInstanceOf: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg, PhpParser\Node\Arg): PhpParser\Node\Expr, isInt: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isIterable: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isList: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isMap: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isNaturalInt: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isNegativeInt: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isNonEmptyString: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isNull: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isNumeric: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isObject: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isPositiveInt: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isResource: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isSameAs: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg, PhpParser\Node\Arg): PhpParser\Node\Expr, isScalar: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isString: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr, isTrue: Closure(PHPStan\Analyser\Scope, PhpParser\Node\Arg): PhpParser\Node\Expr}", ExpectationMethodResolver::$resolvers);

/**
 * @param callable(Scope, Node\Arg):Node\Expr $callable
 */
function doFoo($callable):void {}
doFoo(ExpectationMethodResolver::$resolvers['isArray']);

/**
 * @param \Closure(Scope, Node\Arg):Node\Expr $callable
 */
function doBar($callable):void {}
doBar(ExpectationMethodResolver::$resolvers['isArray']);
