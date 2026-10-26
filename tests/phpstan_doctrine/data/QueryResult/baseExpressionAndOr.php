<?php declare(strict_types = 1);

namespace QueryResult\CreateQuery;

use Doctrine\ORM\Query\Expr\Andx;
use function PHPStan\Testing\assertType;

$and = new Andx();
$count1 = $and->count();
// ORM 3 declares count() as `0|positive-int`, which is `int<0, max>`.
assertType('0|positive-int', $count1);

$modifiedAnd = $and->add('a = b');
assertType("Doctrine\ORM\Query\Expr\Andx", $modifiedAnd);

$string = $and->__toString();
assertType("string", $string);

$invalidAdd = $and->add(123);
assertType("Doctrine\ORM\Query\Expr\Andx", $invalidAdd);
