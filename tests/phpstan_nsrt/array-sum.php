<?php

namespace ArraySum;

use function PHPStan\Testing\assertType;

/**
 * @param int[] $integerList
 */
function foo($integerList)
{
	$sum = array_sum($integerList);
	assertType('int', $sum);
}

/**
 * @param float[] $floatList
 */
function foo2($floatList)
{
	$sum = array_sum($floatList);
	assertType('0|float', $sum);
}

/**
 * @param non-empty-array<float> $floatList
 */
function foo3($floatList)
{
	$sum = array_sum($floatList);
	assertType('float', $sum);
}

/**
 * @param mixed[] $list
 */
function foo4($list)
{
	$sum = array_sum($list);
	assertType('(float|int)', $sum);
}

/**
 * @param string[] $list
 */
function foo5($list)
{
	$sum = array_sum($list);
	assertType('float|int', $sum);
}

/**
 * @param list<0> $list
 */
function foo6($list)
{
	assertType('0', array_sum($list));
}
/**
 * @param list<1> $list
 */
function foo7($list)
{
	assertType('int<0, max>', array_sum($list));
}

/**
 * @param non-empty-list<1> $list
 */
function foo8($list)
{
	assertType('int<1, max>', array_sum($list));
}

/**
 * @param list<-1> $list
 */
function foo9($list)
{
	assertType('int<min, 0>', array_sum($list));
}

/**
 * @param list<1|2|3> $list
 */
function foo10($list)
{
	assertType('int<0, max>', array_sum($list));
}

/**
 * @param non-empty-list<1|2|3> $list
 */
function foo11($list)
{
	assertType('int<1, max>', array_sum($list));
}

/**
 * @param list<1|-1> $list
 */
function foo12($list)
{
	assertType('int', array_sum($list));
}
/**
 * @param non-empty-list<1|-1> $list
 */
function foo13($list)
{
	assertType('int', array_sum($list));
}

/**
 * @param array{0} $list
 */
function foo14($list)
{
	assertType('0', array_sum($list));
}
/**
 * @param array{1} $list
 */
function foo15($list)
{
	assertType('1', array_sum($list));
}

/**
 * @param array{1, 2, 3} $list
 */
function foo16($list)
{
	assertType('6', array_sum($list));
}

/**
 * @param array{1, int} $list
 */
function foo17($list)
{
	assertType('int', array_sum($list));
}

/**
 * @param array{1, float} $list
 */
function foo18($list)
{
	assertType('float', array_sum($list));
}

/**
 * @param array{} $list
 */
function foo19($list)
{
	assertType('0', array_sum($list));
}


/**
 * @param list<1|float> $list
 */
function foo20($list)
{
	assertType('float|int<0, max>', array_sum($list));
}

/**
 * @param array{1, int|float} $list
 */
function foo21($list)
{
	assertType('float|int', array_sum($list));
}

/**
 * @param array{1, string} $list
 */
function foo22($list)
{
	assertType('float|int', array_sum($list));
}


/**
 * @param array{1, 3.2} $list
 */
function foo23($list)
{
	assertType('4.2', array_sum($list));
}

/**
 * @param array{1, float|4} $list
 */
function foo24($list)
{
	assertType('5|float', array_sum($list));
}

/**
 * @param array{1, 2|3.4} $list
 */
function foo25($list)
{
	assertType('3|4.4', array_sum($list));
}

/**
 * @param array{1, 2.4|3.4} $list
 */
function foo26($list)
{
	assertType('3.4|4.4', array_sum($list));
}


/**
 * @param array{1}|array{2, 3} $list
 */
function foo27($list)
{
	assertType('1|5', array_sum($list));
}

/**
 * @param array{1}|list<1>|array{float} $list
 */
function foo28($list)
{
	assertType('float|int<0, max>', array_sum($list));
}

/**
 * @param non-empty-list<int<min, -1>|int<1, max>> $list
 */
function foo29($list)
{
	assertType('int', array_sum($list));
}

/**
 * @param array{'133', 3} $list
 */
function foo30($list)
{
	assertType('136', array_sum($list));
}

/**
 * @param array{0: 1, 1?: 2, 2?: 3} $list
 */
function foo31($list)
{
	assertType('1|3|4|6', array_sum($list));
}

/**
 * @param mixed $list
 */
function foo32($list)
{
	assertType('(float|int)', array_sum($list));
}
