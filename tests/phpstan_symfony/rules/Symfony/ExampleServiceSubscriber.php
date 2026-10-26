<?php declare(strict_types = 1);

namespace PHPStan\Rules\Symfony;

use Psr\Container\ContainerInterface;
use Symfony\Component\DependencyInjection\ParameterBag\ContainerBag;
use Symfony\Contracts\Service\ServiceSubscriberInterface;

final class ExampleServiceSubscriber implements ServiceSubscriberInterface
{

	private ContainerInterface $locator;

	public function __construct(ContainerInterface $locator)
	{
		$this->locator = $locator;
	}

	public function privateService(): void
	{
		$this->get('private');
		$this->locator->get('private');
	}

	public function containerParameter(): void
	{
		/** @var ContainerBag $containerBag */
		$containerBag = doFoo();
		$containerBag->get('parameter_name');
	}

	/**
	 * @return string[]
	 */
	public static function getSubscribedServices(): array
	{
		return [];
	}

}
