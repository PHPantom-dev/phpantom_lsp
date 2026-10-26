<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony;

use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Input\InputArgument;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Output\OutputInterface;
use function PHPStan\Testing\assertType;

abstract class ExampleBaseCommand extends Command
{

	protected function configure(): void
	{
		parent::configure();

		$this->addArgument('required', InputArgument::REQUIRED);
		$this->addArgument('base');
	}

	protected function initialize(InputInterface $input, OutputInterface $output): void
	{
		assertType('bool', $input->hasArgument('command'));
		assertType('string|null', $input->getArgument('command')); // SKIP: console arguments and options are not read from configure()

		assertType('string|null', $input->getArgument('base')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('aaa')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('bbb')); // SKIP: console arguments and options are not read from configure()
		assertType('string|null', $input->getArgument('required')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>|string', $input->getArgument('diff')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>', $input->getArgument('arr')); // SKIP: console arguments and options are not read from configure()
		assertType('string|null', $input->getArgument('both')); // SKIP: console arguments and options are not read from configure()
		assertType('Symfony\Component\Console\Helper\QuestionHelper', $this->getHelper('question')); // SKIP: Symfony return types an argument decides
	}

	protected function interact(InputInterface $input, OutputInterface $output): void
	{
		assertType('bool', $input->hasArgument('command'));
		assertType('string|null', $input->getArgument('command')); // SKIP: console arguments and options are not read from configure()

		assertType('string|null', $input->getArgument('base')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('aaa')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('bbb')); // SKIP: console arguments and options are not read from configure()
		assertType('string|null', $input->getArgument('required')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>|string', $input->getArgument('diff')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>', $input->getArgument('arr')); // SKIP: console arguments and options are not read from configure()
		assertType('string|null', $input->getArgument('both')); // SKIP: console arguments and options are not read from configure()
		assertType('Symfony\Component\Console\Helper\QuestionHelper', $this->getHelper('question')); // SKIP: Symfony return types an argument decides
	}

	protected function execute(InputInterface $input, OutputInterface $output): int
	{
		assertType('true', $input->hasArgument('command')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('command')); // SKIP: console arguments and options are not read from configure()

		assertType('string|null', $input->getArgument('base')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('aaa')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('bbb')); // SKIP: console arguments and options are not read from configure()
		assertType('string', $input->getArgument('required')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>|string', $input->getArgument('diff')); // SKIP: console arguments and options are not read from configure()
		assertType('array<int, string>', $input->getArgument('arr')); // SKIP: console arguments and options are not read from configure()
		assertType('string|null', $input->getArgument('both')); // SKIP: console arguments and options are not read from configure()
		assertType('Symfony\Component\Console\Helper\QuestionHelper', $this->getHelper('question')); // SKIP: Symfony return types an argument decides
	}

}
