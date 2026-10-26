<?php declare(strict_types = 1);

namespace QueryResult\CreateQuery;

use Doctrine\ORM\AbstractQuery;
use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\QueryBuilder;
use QueryResult\Entities\Many;
use function PHPStan\Testing\assertType;

class ExpressionBuilderGetQuery
{
	private function nonLiteralString(string $value): string {
		return $value; // Using the 'string' return type to provide a non `literal-string`, e.g. $_POST['field'];
	}

	private function nonLiteralInteger(int $value): int {
		return $value;
	}

	public function isNullLiteralString(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->isNull('field');
		assertType("'field IS NULL'", $result); // A ConstantStringType isLiteralString // SKIP: the expression builder's DQL fragments are not read as literal strings
	}

	public function isNullNonLiteralString(EntityManagerInterface $em): void
	{
		$field = $this->nonLiteralString('field');
		$result = $em->createQueryBuilder()->expr()->isNull($field);
		assertType('string', $result);
	}

	public function isNotNullLiteralString(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->isNotNull('field');
		assertType("'field IS NOT NULL'", $result); // A ConstantStringType isLiteralString // SKIP: the expression builder's DQL fragments are not read as literal strings
	}

	public function isNotNullNonLiteralString(EntityManagerInterface $em): void
	{
		$field = $this->nonLiteralString('field');
		$result = $em->createQueryBuilder()->expr()->isNotNull($field);
		assertType('string', $result);
	}

	public function betweenLiteralString(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->between('field', "'value_1'", "'value_2'");
		assertType("'field BETWEEN \'value_1\' AND \'value_2\''", $result); // A ConstantStringType isLiteralString // SKIP: the expression builder's DQL fragments are not read as literal strings
	}

	public function betweenNonLiteralString1(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralString('A');
		$result = $em->createQueryBuilder()->expr()->between($value, "'value_1'", "'value_2'");
		assertType('string', $result);
	}

	public function betweenNonLiteralString2(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralString('A');
		$result = $em->createQueryBuilder()->expr()->between('field', "'" . $value . "'", "'value_2'");
		assertType('string', $result);
	}

	public function betweenNonLiteralString3(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralString('A');
		$result = $em->createQueryBuilder()->expr()->between('field', "'value_1'", "'" . $value . "'");
		assertType('string', $result);
	}

	public function betweenNonLiteralString4(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralInteger(2); // Integers are not literal-strings
		$result = $em->createQueryBuilder()->expr()->between('field', '1', $value);
		assertType('string', $result);
	}

	public function countDistinctLiteralString(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->countDistinct('A', 'B', 'C');
		assertType("'COUNT(DISTINCT A, B, C)'", $result); // A ConstantStringType isLiteralString // SKIP: the expression builder's DQL fragments are not read as literal strings
	}

	public function countDistinctNonLiteralString1(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralString('A');
		$result = $em->createQueryBuilder()->expr()->countDistinct($value);
		assertType('string', $result);
	}

	public function countDistinctNonLiteralString2(EntityManagerInterface $em): void
	{
		$value = $this->nonLiteralString('A');
		$result = $em->createQueryBuilder()->expr()->countDistinct($value, 'B', 'C');
		assertType('string', $result);
	}

	// Disabled until Doctrine ORM 3.0.0, as the countDistinct() function definition does not use `countDistinct(...$x)`
	//   https://github.com/doctrine/orm/pull/9911
	//   https://github.com/phpstan/phpstan-doctrine/pull/352
	//
	// public function countDistinctNonLiteralString3(EntityManagerInterface $em): void
	// {
	// 	$value = $this->nonLiteralString('B');
	// 	$result = $em->createQueryBuilder()->expr()->countDistinct('A', $value, 'C');
	// 	assertType('string', $result);
	// }
	//
	// public function countDistinctNonLiteralString4(EntityManagerInterface $em): void
	// {
	// 	$value = $this->nonLiteralString('C');
	// 	$result = $em->createQueryBuilder()->expr()->countDistinct('A', 'B', $value);
	// 	assertType('string', $result);
	// }

	// Might be a problem, as these do not return a 'literal-string'.
	// As in, functions to support MOD() and ABS() return stringable value objects (Expr\Func).
	public function isNullNonLiteralStringExprFunc(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->isNull($qb->expr()->mod('field', '0'));
		assertType('string', $result);
	}

	public function betweenNonLiteralStringExprFunc(EntityManagerInterface $em): void
	{
		$result = $em->createQueryBuilder()->expr()->between($qb->expr()->abs('field'), '10', '30');
		assertType('string', $result);
	}

}
