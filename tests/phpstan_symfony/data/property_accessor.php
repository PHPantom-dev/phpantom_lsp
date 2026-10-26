<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

$propertyAccessor = new \Symfony\Component\PropertyAccess\PropertyAccessor();

$array = [1 => 'ea'];
$propertyAccessor->setValue($array, 'foo', 'bar');
assertType('array<mixed>', $array);

$object = new \stdClass();
$propertyAccessor->setValue($object, 'foo', 'bar');
assertType('stdClass', $object);
