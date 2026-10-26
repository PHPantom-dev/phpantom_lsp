<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\DependencyInjection\ContainerInterface;
use Symfony\Component\DependencyInjection\ParameterBag\ParameterBagInterface;
use function PHPStan\Testing\assertType;

final class ExampleAbstractController extends AbstractController
{

	public function services(): void
	{
		assertType('Foo', $this->get('foo')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('Foo', $this->get('parameterised_foo')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('Foo\Bar', $this->get('parameterised_bar')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('Synthetic', $this->get('synthetic')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get('bar')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get(doFoo())); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get()); // SKIP: AbstractController::get() and has() were removed in Symfony 6

		assertType('true', $this->has('foo')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('true', $this->has('synthetic')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('false', $this->has('bar')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('bool', $this->has(doFoo())); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('bool', $this->has()); // SKIP: AbstractController::get() and has() were removed in Symfony 6
	}

	public function parameters(ContainerInterface $container, ParameterBagInterface $parameterBag): void
	{
		// Symfony 7 declares a parameter value as possibly a `UnitEnum` too;
		// upstream expects the Symfony 5 declaration.
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('unknown'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('unknown'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('unknown'));
		assertType("string", $container->getParameter('app.string'));
		assertType("string", $parameterBag->get('app.string'));
		assertType("string", $this->getParameter('app.string'));
		assertType('int', $container->getParameter('app.int'));
		assertType('int', $parameterBag->get('app.int'));
		assertType('int', $this->getParameter('app.int'));
		assertType("string", $container->getParameter('app.int_as_string'));
		assertType("string", $parameterBag->get('app.int_as_string'));
		assertType("string", $this->getParameter('app.int_as_string'));
		assertType('int', $container->getParameter('app.int_as_processor'));
		assertType('int', $parameterBag->get('app.int_as_processor'));
		assertType('int', $this->getParameter('app.int_as_processor'));
		assertType('float', $container->getParameter('app.float'));
		assertType('float', $parameterBag->get('app.float'));
		assertType('float', $this->getParameter('app.float'));
		assertType("string", $container->getParameter('app.float_as_string'));
		assertType("string", $parameterBag->get('app.float_as_string'));
		assertType("string", $this->getParameter('app.float_as_string'));
		assertType('bool', $container->getParameter('app.boolean'));
		assertType('bool', $parameterBag->get('app.boolean'));
		assertType('bool', $this->getParameter('app.boolean'));
		assertType("string", $container->getParameter('app.boolean_as_string'));
		assertType("string", $parameterBag->get('app.boolean_as_string'));
		assertType("string", $this->getParameter('app.boolean_as_string'));
		assertType("array<int, string>", $container->getParameter('app.list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, string>", $parameterBag->get('app.list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, string>", $this->getParameter('app.list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $container->getParameter('app.list_of_int')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $parameterBag->get('app.list_of_int')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $this->getParameter('app.list_of_int')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $container->getParameter('app.list_of_int_as_processor')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $parameterBag->get('app.list_of_int_as_processor')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, int>", $this->getParameter('app.list_of_int_as_processor')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{name: string, value: string}>", $container->getParameter('app.list_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{name: string, value: string}>", $parameterBag->get('app.list_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{name: string, value: string}>", $this->getParameter('app.list_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{age: int, size: int}|array{name: string, value: string}>", $container->getParameter('app.list_of_different_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{age: int, size: int}|array{name: string, value: string}>", $parameterBag->get('app.list_of_different_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int, array{age: int, size: int}|array{name: string, value: string}>", $this->getParameter('app.list_of_different_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int|string, array{name: string, value: string}>", $container->getParameter('app.array_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int|string, array{name: string, value: string}>", $parameterBag->get('app.array_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array<int|string, array{name: string, value: string}>", $this->getParameter('app.array_of_list')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{url: string, endpoint: string, version: string, payment: array{default: array{username: string, password: string, signature: string}}, api: array{mode: string, default: array{username: string, password: string, signature: string}}}", $container->getParameter('app.list_of_things')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{url: string, endpoint: string, version: string, payment: array{default: array{username: string, password: string, signature: string}}, api: array{mode: string, default: array{username: string, password: string, signature: string}}}", $parameterBag->get('app.list_of_things')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{url: string, endpoint: string, version: string, payment: array{default: array{username: string, password: string, signature: string}}, api: array{mode: string, default: array{username: string, password: string, signature: string}}}", $this->getParameter('app.list_of_things')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{a: string, b: string, c: string}", $container->getParameter('app.map')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{a: string, b: string, c: string}", $parameterBag->get('app.map')); // SKIP: Symfony array parameters are typed as plain array
		assertType("array{a: string, b: string, c: string}", $this->getParameter('app.map')); // SKIP: Symfony array parameters are typed as plain array
		assertType("non-falsy-string", implode(',', $this->getParameter('app.hugemap'))); // SKIP: Symfony array parameters are typed as plain array
		assertType("string", $container->getParameter('app.binary'));
		assertType("string", $parameterBag->get('app.binary'));
		assertType("string", $this->getParameter('app.binary'));
		assertType("string", $container->getParameter('app.constant'));
		assertType("string", $parameterBag->get('app.constant'));
		assertType("string", $this->getParameter('app.constant'));
		assertType("array", $this->getParameter('test_collection'));
		assertType("array<int, string>", $this->getParameter('non_empty_collection')); // SKIP: Symfony array parameters are typed as plain array

		assertType('false', $container->hasParameter('unknown')); // SKIP: Symfony return types an argument decides
		assertType('false', $parameterBag->has('unknown')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.string')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.string')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.int')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.int')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.int_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.int_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.int_as_processor')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.int_as_processor')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.float')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.float')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.float_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.float_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.boolean')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.boolean')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.boolean_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.boolean_as_string')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.list')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.list')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.list_of_list')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.list_of_list')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.map')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.map')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.binary')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.binary')); // SKIP: Symfony return types an argument decides
		assertType('true', $container->hasParameter('app.constant')); // SKIP: Symfony return types an argument decides
		assertType('true', $parameterBag->has('app.constant')); // SKIP: Symfony return types an argument decides
	}

}
