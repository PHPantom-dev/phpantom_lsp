<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony;

use Symfony\Component\Console\Input\InputArgument;

final class ExampleBCommand extends ExampleBaseCommand
{

	protected function configure(): void
	{
		parent::configure();
		$this->setName('example-b');

		$this->addArgument('both');
		$this->addArgument('bbb', null, '', 'bbb');
		$this->addArgument('diff', InputArgument::IS_ARRAY, '', ['diff']);
	}

}
