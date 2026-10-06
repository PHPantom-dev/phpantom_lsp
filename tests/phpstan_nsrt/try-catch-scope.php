<?php

namespace TryCatchScope;

use function PHPStan\Testing\assertType;

function () {

	$resource = null;
	try {
		$resource = new Foo();
	} catch (FooException $e) {
		$resource = new Foo();
	} catch (BarException $e) {
		$resource = new Foo();
	}

	assertType('TryCatchScope\Foo', $resource);

};

function () {

	$resource = null;
	try {
		$resource = new Foo();
	} catch (FooException $e) {

	} catch (BarException $e) {
		$resource = new Foo();
	}

	assertType('TryCatchScope\Foo|null', $resource);

};

function () {

	$resource = null;
	try {
		$resource = new Foo();
	} catch (FooException $e) {

	} catch (BarException $e) {

	}

	assertType('TryCatchScope\Foo|null', $resource);

};
