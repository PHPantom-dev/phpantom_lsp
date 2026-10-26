<?php declare(strict_types = 1);

namespace PHPStan\Rules\Symfony;

use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Input\InputArgument;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Input\InputOption;
use Symfony\Component\Console\Output\OutputInterface;

final class ExampleCommand extends Command
{

	protected function configure(): void
	{
		$this->setName('example-rule');

		$this->addArgument('arg');

		$this->addArgument('foo1', null, '', null);
		$this->addArgument('bar1', null, '', '');
		$this->addArgument('baz1', null, '', 1);
		$this->addArgument('quz1', null, '', ['']);

		$this->addArgument('quz2', InputArgument::IS_ARRAY, '', ['a' => 'b']);

		$this->addOption('aaa');

		$this->addOption('b', null, InputOption::VALUE_IS_ARRAY | InputOption::VALUE_OPTIONAL, '', [1]);
		$this->addOption('c', null, InputOption::VALUE_OPTIONAL, '', 1);
		$this->addOption('d', null, InputOption::VALUE_OPTIONAL, '', false);
		$this->addOption('f', null, InputOption::VALUE_REQUIRED, '', true);

		/** @var string[] $defaults */
		$defaults = [];
		$this->addOption('e', null, InputOption::VALUE_IS_ARRAY | InputOption::VALUE_OPTIONAL, '', $defaults);
	}

	protected function execute(InputInterface $input, OutputInterface $output): int
	{
		$input->getArgument('arg');
		$input->getArgument('undefined');

		if ($input->hasArgument('guarded')) {
			$input->getArgument('guarded');
		}

		$input->getOption('aaa');
		$input->getOption('bbb');

		if ($input->hasOption('ccc')) {
			$input->getOption('ccc');
		}

		return 0;
	}

}
