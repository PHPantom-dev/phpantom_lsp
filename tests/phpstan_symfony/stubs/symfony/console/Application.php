<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Console;

use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Command\CompleteCommand;
use Symfony\Component\Console\Command\DumpCompletionCommand;
use Symfony\Component\Console\Command\HelpCommand;
use Symfony\Component\Console\Command\LazyCommand;
use Symfony\Component\Console\Command\ListCommand;
use Symfony\Component\Console\CommandLoader\CommandLoaderInterface;
use Symfony\Component\Console\Completion\CompletionInput;
use Symfony\Component\Console\Completion\CompletionSuggestions;
use Symfony\Component\Console\Completion\Suggestion;
use Symfony\Component\Console\Event\ConsoleAlarmEvent;
use Symfony\Component\Console\Event\ConsoleCommandEvent;
use Symfony\Component\Console\Event\ConsoleErrorEvent;
use Symfony\Component\Console\Event\ConsoleSignalEvent;
use Symfony\Component\Console\Event\ConsoleTerminateEvent;
use Symfony\Component\Console\Exception\CommandNotFoundException;
use Symfony\Component\Console\Exception\ExceptionInterface;
use Symfony\Component\Console\Exception\LogicException;
use Symfony\Component\Console\Exception\NamespaceNotFoundException;
use Symfony\Component\Console\Exception\RuntimeException;
use Symfony\Component\Console\Formatter\OutputFormatter;
use Symfony\Component\Console\Helper\DebugFormatterHelper;
use Symfony\Component\Console\Helper\DescriptorHelper;
use Symfony\Component\Console\Helper\FormatterHelper;
use Symfony\Component\Console\Helper\Helper;
use Symfony\Component\Console\Helper\HelperSet;
use Symfony\Component\Console\Helper\ProcessHelper;
use Symfony\Component\Console\Helper\QuestionHelper;
use Symfony\Component\Console\Input\ArgvInput;
use Symfony\Component\Console\Input\ArrayInput;
use Symfony\Component\Console\Input\InputArgument;
use Symfony\Component\Console\Input\InputAwareInterface;
use Symfony\Component\Console\Input\InputDefinition;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Input\InputOption;
use Symfony\Component\Console\Output\ConsoleOutput;
use Symfony\Component\Console\Output\ConsoleOutputInterface;
use Symfony\Component\Console\Output\OutputInterface;
use Symfony\Component\Console\SignalRegistry\SignalRegistry;
use Symfony\Component\Console\Style\SymfonyStyle;
use Symfony\Component\ErrorHandler\ErrorHandler;
use Symfony\Component\EventDispatcher\EventDispatcherInterface as ComponentEventDispatcherInterface;
use Symfony\Contracts\EventDispatcher\EventDispatcherInterface;
use Symfony\Contracts\Service\ResetInterface;

/**
 * An Application is the container for a collection of commands.
 *
 * It is the main entry point of a Console application.
 *
 * This class is optimized for a standard CLI environment.
 *
 * Usage:
 *
 *     $app = new Application('myapp', '1.0 (stable)');
 *     $app->addCommand(new SimpleCommand());
 *     $app->run();
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class Application implements ResetInterface
{
    private array $commands = [];
    private bool $wantHelps = false;
    private ?Command $runningCommand = null;
    private ?CommandLoaderInterface $commandLoader = null;
    private bool $catchExceptions = true;
    private bool $catchErrors = false;
    private bool $autoExit = true;
    private InputDefinition $definition;
    private HelperSet $helperSet;
    private ?EventDispatcherInterface $dispatcher = null;
    private Terminal $terminal;
    private string $defaultCommand;
    private bool $singleCommand = false;
    private bool $initialized = false;
    private ?SignalRegistry $signalRegistry = null;
    private array $signalsToDispatchEvent = [];
    private ?int $alarmInterval = null;

    public function __construct(
        private string $name = 'UNKNOWN',
        private string $version = 'UNKNOWN',
    ) {}

    /**
     * @final
     */
    public function setDispatcher(EventDispatcherInterface $dispatcher): void
    {}

    public function setCommandLoader(CommandLoaderInterface $commandLoader): void
    {}

    public function getSignalRegistry(): SignalRegistry
    {}

    public function setSignalsToDispatchEvent(int ...$signalsToDispatchEvent): void
    {}

    /**
     * Sets the interval to schedule a SIGALRM signal in seconds.
     */
    public function setAlarmInterval(?int $seconds): void
    {}

    /**
     * Gets the interval in seconds on which a SIGALRM signal is dispatched.
     */
    public function getAlarmInterval(): ?int
    {}

    private function scheduleAlarm(): void
    {}

    /**
     * Runs the current application.
     *
     * @return int 0 if everything went fine, or an error code
     *
     * @throws \Exception When running fails. Bypass this when {@link setCatchExceptions()}.
     */
    public function run(?InputInterface $input = null, ?OutputInterface $output = null): int
    {}

    /**
     * Runs the current application.
     *
     * @return int 0 if everything went fine, or an error code
     */
    public function doRun(InputInterface $input, OutputInterface $output): int
    {}

    public function reset(): void
    {}

    public function setHelperSet(HelperSet $helperSet): void
    {}

    /**
     * Get the helper set associated with the command.
     */
    public function getHelperSet(): HelperSet
    {}

    public function setDefinition(InputDefinition $definition): void
    {}

    /**
     * Gets the InputDefinition related to this Application.
     */
    public function getDefinition(): InputDefinition
    {}

    /**
     * Adds suggestions to $suggestions for the current completion input (e.g. option or argument).
     */
    public function complete(CompletionInput $input, CompletionSuggestions $suggestions): void
    {}

    /**
     * Gets the help message.
     */
    public function getHelp(): string
    {}

    /**
     * Gets whether to catch exceptions or not during commands execution.
     */
    public function areExceptionsCaught(): bool
    {}

    /**
     * Sets whether to catch exceptions or not during commands execution.
     */
    public function setCatchExceptions(bool $boolean): void
    {}

    /**
     * Sets whether to catch errors or not during commands execution.
     */
    public function setCatchErrors(bool $catchErrors = true): void
    {}

    /**
     * Gets whether to automatically exit after a command execution or not.
     */
    public function isAutoExitEnabled(): bool
    {}

    /**
     * Sets whether to automatically exit after a command execution or not.
     */
    public function setAutoExit(bool $boolean): void
    {}

    /**
     * Gets the name of the application.
     */
    public function getName(): string
    {}

    /**
     * Sets the application name.
     */
    public function setName(string $name): void
    {}

    /**
     * Gets the application version.
     */
    public function getVersion(): string
    {}

    /**
     * Sets the application version.
     */
    public function setVersion(string $version): void
    {}

    /**
     * Returns the long version of the application.
     */
    public function getLongVersion(): string
    {}

    /**
     * Registers a new command.
     */
    public function register(string $name): Command
    {}

    /**
     * Adds an array of command objects.
     *
     * If a Command is not enabled it will not be added.
     *
     * @param callable[]|Command[] $commands An array of commands
     */
    public function addCommands(array $commands): void
    {}

    /**
     * @deprecated since Symfony 7.4, use Application::addCommand() instead
     */
    public function add(Command $command): ?Command
    {}

    /**
     * Adds a command object.
     *
     * If a command with the same name already exists, it will be overridden.
     * If the command is not enabled it will not be added.
     */
    public function addCommand(callable|Command $command): ?Command
    {}

    /**
     * Returns a registered command by name or alias.
     *
     * @throws CommandNotFoundException When given command name does not exist
     */
    public function get(string $name): Command
    {}

    /**
     * Returns true if the command exists, false otherwise.
     */
    public function has(string $name): bool
    {}

    /**
     * Returns an array of all unique namespaces used by currently registered commands.
     *
     * It does not return the global namespace which always exists.
     *
     * @return string[]
     */
    public function getNamespaces(): array
    {}

    /**
     * Finds a registered namespace by a name or an abbreviation.
     *
     * @throws NamespaceNotFoundException When namespace is incorrect or ambiguous
     */
    public function findNamespace(string $namespace): string
    {}

    /**
     * Finds a command by name or alias.
     *
     * Contrary to get, this command tries to find the best
     * match if you give it an abbreviation of a name or alias.
     *
     * @throws CommandNotFoundException When command name is incorrect or ambiguous
     */
    public function find(string $name): Command
    {}

    /**
     * Gets the commands (registered in the given namespace if provided).
     *
     * The array keys are the full names and the values the command instances.
     *
     * @return Command[]
     */
    public function all(?string $namespace = null): array
    {}

    /**
     * Returns an array of possible abbreviations given a set of names.
     *
     * @return string[][]
     */
    public static function getAbbreviations(array $names): array
    {}

    public function renderThrowable(\Throwable $e, OutputInterface $output): void
    {}

    protected function doRenderThrowable(\Throwable $e, OutputInterface $output): void
    {}

    /**
     * Configures the input and output instances based on the user arguments and options.
     */
    protected function configureIO(InputInterface $input, OutputInterface $output): void
    {}

    /**
     * Runs the current command.
     *
     * If an event dispatcher has been attached to the application,
     * events are also dispatched during the life-cycle of the command.
     *
     * @return int 0 if everything went fine, or an error code
     */
    protected function doRunCommand(Command $command, InputInterface $input, OutputInterface $output): int
    {}

    /**
     * Gets the name of the command based on input.
     */
    protected function getCommandName(InputInterface $input): ?string
    {}

    /**
     * Gets the default input definition.
     */
    protected function getDefaultInputDefinition(): InputDefinition
    {}

    /**
     * Gets the default commands that should always be available.
     *
     * @return Command[]
     */
    protected function getDefaultCommands(): array
    {}

    /**
     * Gets the default helper set with the helpers that should always be available.
     */
    protected function getDefaultHelperSet(): HelperSet
    {}

    /**
     * Returns abbreviated suggestions in string format.
     */
    private function getAbbreviationSuggestions(array $abbrevs): string
    {}

    /**
     * Returns the namespace part of the command name.
     *
     * This method is not part of public API and should not be used directly.
     */
    public function extractNamespace(string $name, ?int $limit = null): string
    {}

    /**
     * Finds alternative of $name among $collection,
     * if nothing is found in $collection, try in $abbrevs.
     *
     * @return string[]
     */
    private function findAlternatives(string $name, iterable $collection): array
    {}

    /**
     * Sets the default Command name.
     *
     * @return $this
     */
    public function setDefaultCommand(string $commandName, bool $isSingleCommand = false): static
    {}

    /**
     * @internal
     */
    public function isSingleCommand(): bool
    {}

    private function splitStringByWidth(string $string, int $width): array
    {}

    /**
     * Returns all namespaces of the command name.
     *
     * @return string[]
     */
    private function extractAllNamespaces(string $name): array
    {}

    private function init(): void
    {}
}
