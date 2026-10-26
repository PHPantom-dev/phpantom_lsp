<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony;

use Symfony\Component\Console\Input\InputArgument;

final class ExampleACommand extends ExampleBaseCommand
{

	protected function configure(): void
	{
		parent::configure();
		$this->setName('example-a');

		$this->addArgument('aaa', null, '', 'aaa');
		$this->addArgument('both');
		$this->addArgument('diff', null, '', 'ddd');
		$this->addArgument('arr', InputArgument::IS_ARRAY, '', ['arr']);
	}

}
