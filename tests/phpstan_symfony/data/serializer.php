<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

$serializer = new \Symfony\Component\Serializer\Serializer();

assertType('Bar', $serializer->deserialize('bar', 'Bar', 'format')); // SKIP: Symfony return types an argument decides
assertType('array<Bar>', $serializer->deserialize('bar', 'Bar[]', 'format')); // SKIP: Symfony return types an argument decides
assertType('array<array<Bar>>', $serializer->deserialize('bar', 'Bar[][]', 'format')); // SKIP: Symfony return types an argument decides
assertType('mixed', $serializer->deserialize('bar'));
