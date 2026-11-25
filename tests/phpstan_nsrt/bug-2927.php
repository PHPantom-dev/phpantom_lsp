<?php

namespace Bug2927;

use function PHPStan\Testing\assertType;

abstract class Event
{

}

class MyEvent extends Event
{

}

function (): void {
	$reflect = new \ReflectionFunction('test\\handler');
	$paramClass = $reflect->getParameters()[0]->getClass();
	if ($paramClass === null or !$paramClass->isSubclassOf(Event::class)) {
		return;
	}

	/** @var \ReflectionClass<MyEvent> $paramClass */

	assertType('class-string<Bug2927\\MyEvent>', $paramClass->getName());

	try {
		throw new \Exception();
	} catch (\Exception $e) {

	}

	assertType('class-string<Bug2927\\MyEvent>', $paramClass->getName());
};
