<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\DependencyInjection\ContainerInterface;
use Symfony\Component\DependencyInjection\ParameterBag\ParameterBagInterface;
use function PHPStan\Testing\assertType;

final class ExampleAbstractControllerWithoutContainer extends AbstractController
{

	public function services(): void
	{
		assertType('object', $this->get('foo')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get('synthetic')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get('bar')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get(doFoo())); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('object', $this->get()); // SKIP: AbstractController::get() and has() were removed in Symfony 6

		assertType('bool', $this->has('foo')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('bool', $this->has('synthetic')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
		assertType('bool', $this->has('bar')); // SKIP: AbstractController::get() and has() were removed in Symfony 6
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
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.int'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.int'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.int'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.int_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.int_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.int_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.int_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.int_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.int_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.float'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.float'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.float'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.float_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.float_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.float_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.float_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.float_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.float_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.boolean'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.boolean'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.boolean'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.boolean_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.boolean_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.boolean_as_string'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.boolean_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.boolean_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.boolean_as_processor'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.list_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.list_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.list_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.array_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.array_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.array_of_list'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.map'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.map'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.map'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.binary'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.binary'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.binary'));
		assertType('array|bool|float|int|string|UnitEnum|null', $container->getParameter('app.constant'));
		assertType('array|bool|float|int|string|UnitEnum|null', $parameterBag->get('app.constant'));
		assertType('array|bool|float|int|string|UnitEnum|null', $this->getParameter('app.constant'));

		assertType('bool', $container->hasParameter('unknown'));
		assertType('bool', $parameterBag->has('unknown'));
		assertType('bool', $container->hasParameter('app.string'));
		assertType('bool', $parameterBag->has('app.string'));
		assertType('bool', $container->hasParameter('app.int'));
		assertType('bool', $parameterBag->has('app.int'));
		assertType('bool', $container->hasParameter('app.int_as_string'));
		assertType('bool', $parameterBag->has('app.int_as_string'));
		assertType('bool', $container->hasParameter('app.int_as_processor'));
		assertType('bool', $parameterBag->has('app.int_as_processor'));
		assertType('bool', $container->hasParameter('app.float'));
		assertType('bool', $parameterBag->has('app.float'));
		assertType('bool', $container->hasParameter('app.float_as_string'));
		assertType('bool', $parameterBag->has('app.float_as_string'));
		assertType('bool', $container->hasParameter('app.float_as_processor'));
		assertType('bool', $parameterBag->has('app.float_as_processor'));
		assertType('bool', $container->hasParameter('app.boolean'));
		assertType('bool', $parameterBag->has('app.boolean'));
		assertType('bool', $container->hasParameter('app.boolean_as_string'));
		assertType('bool', $parameterBag->has('app.boolean_as_string'));
		assertType('bool', $container->hasParameter('app.boolean_as_processor'));
		assertType('bool', $parameterBag->has('app.boolean_as_processor'));
		assertType('bool', $container->hasParameter('app.list'));
		assertType('bool', $parameterBag->has('app.list'));
		assertType('bool', $container->hasParameter('app.list_of_list'));
		assertType('bool', $parameterBag->has('app.list_of_list'));
		assertType('bool', $container->hasParameter('app.map'));
		assertType('bool', $parameterBag->has('app.map'));
		assertType('bool', $container->hasParameter('app.binary'));
		assertType('bool', $parameterBag->has('app.binary'));
		assertType('bool', $container->hasParameter('app.constant'));
		assertType('bool', $parameterBag->has('app.constant'));
	}

}
