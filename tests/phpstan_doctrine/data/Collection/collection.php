<?php declare(strict_types = 1);

use Doctrine\Common\Collections\ArrayCollection;

use function PHPStan\Testing\assertType;

class MyEntity
{

}

$new = new MyEntity();

/**
 * @var ArrayCollection<int, MyEntity> $collection
 */
$collection = new ArrayCollection();

$entityOrFalse1 = $collection->first();
assertType('MyEntity|false', $entityOrFalse1);

$entityOrFalse2 = $collection->last();
assertType('MyEntity|false', $entityOrFalse2);

if ($collection->isEmpty()) {
	$false1 = $collection->first();
	assertType('false', $false1); // SKIP: Doctrine collections are not narrowed by isEmpty()

	$false2 = $collection->last();
	assertType('false', $false2); // SKIP: Doctrine collections are not narrowed by isEmpty()
}

if (!$collection->isEmpty()) {
	$entity1 = $collection->first();
	assertType('MyEntity', $entity1); // SKIP: Doctrine collections are not narrowed by isEmpty()

	$entity2 = $collection->last();
	assertType('MyEntity', $entity2); // SKIP: Doctrine collections are not narrowed by isEmpty()
}

if ($collection->isEmpty()) {
	$collection->add($new);
	$result1 = $collection->first();
	assertType('MyEntity|false', $result1);

	$result2 = $collection->last();
	assertType('MyEntity|false', $result2);
}

if (!$collection->isEmpty()) {
	$collection->removeElement($new);
	$result3 = $collection->first();
	assertType('MyEntity|false', $result3);

	$result4 = $collection->last();
	assertType('MyEntity|false', $result4);
}
