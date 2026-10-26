<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Console\Command;

use Symfony\Component\Console\Application;
use Symfony\Component\Console\Attribute\AsCommand;
use Symfony\Component\Console\Completion\CompletionInput;
use Symfony\Component\Console\Completion\CompletionSuggestions;
use Symfony\Component\Console\Completion\Suggestion;
use Symfony\Component\Console\Exception\ExceptionInterface;
use Symfony\Component\Console\Exception\InvalidArgumentException;
use Symfony\Component\Console\Exception\LogicException;
use Symfony\Component\Console\Helper\HelperInterface;
use Symfony\Component\Console\Helper\HelperSet;
use Symfony\Component\Console\Input\InputArgument;
use Symfony\Component\Console\Input\InputDefinition;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Input\InputOption;
use Symfony\Component\Console\Output\OutputInterface;

/**
 * Base class for all commands.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class Command implements SignalableCommandInterface
{
    // see https://tldp.org/LDP/abs/html/exitcodes.html
    public const SUCCESS = 0;
    public const FAILURE = 1;
    public const INVALID = 2;

    private ?Application $application = null;
    private ?string $name = null;
    private ?string $processTitle = null;
    private array $aliases = [];
    private InputDefinition $definition;
    private bool $hidden = false;
    private string $help = '';
    private string $description = '';
    private ?InputDefinition $fullDefinition = null;
    private bool $ignoreValidationErrors = false;
    private ?InvokableCommand $code = null;
    private array $synopsis = [];
    private array $usages = [];
    private ?HelperSet $helperSet = null;

    /**
     * @deprecated since Symfony 7.3, use the #[AsCommand] attribute instead
     */
    public static function getDefaultName(): ?string
    {}

    /**
     * @deprecated since Symfony 7.3, use the #[AsCommand] attribute instead
     */
    public static function getDefaultDescription(): ?string
    {}

    /**
     * @param string|null $name The name of the command; passing null means it must be set in configure()
     *
     * @throws LogicException When the command name is empty
     */
    public function __construct(?string $name = null, ?callable $code = null)
    {}

    /**
     * Ignores validation errors.
     *
     * This is mainly useful for the help command.
     */
    public function ignoreValidationErrors(): void
    {}

    public function setApplication(?Application $application): void
    {}

    public function setHelperSet(HelperSet $helperSet): void
    {}

    /**
     * Gets the helper set.
     */
    public function getHelperSet(): ?HelperSet
    {}

    /**
     * Gets the application instance for this command.
     */
    public function getApplication(): ?Application
    {}

    /**
     * Checks whether the command is enabled or not in the current environment.
     *
     * Override this to check for x or y and return false if the command cannot
     * run properly under the current conditions.
     */
    public function isEnabled(): bool
    {}

    /**
     * Configures the current command.
     *
     * @return void
     */
    protected function configure()
    {}

    /**
     * Executes the current command.
     *
     * This method is not abstract because you can use this class
     * as a concrete class. In this case, instead of defining the
     * execute() method, you set the code to execute by passing
     * a Closure to the setCode() method.
     *
     * @return int 0 if everything went fine, or an exit code
     *
     * @throws LogicException When this abstract method is not implemented
     *
     * @see setCode()
     */
    protected function execute(InputInterface $input, OutputInterface $output): int
    {}

    /**
     * Interacts with the user.
     *
     * This method is executed before the InputDefinition is validated.
     * This means that this is the only place where the command can
     * interactively ask for values of missing required arguments.
     *
     * @return void
     */
    protected function interact(InputInterface $input, OutputInterface $output)
    {}

    /**
     * Initializes the command after the input has been bound and before the input
     * is validated.
     *
     * This is mainly useful when a lot of commands extends one main command
     * where some things need to be initialized based on the input arguments and options.
     *
     * @see InputInterface::bind()
     * @see InputInterface::validate()
     *
     * @return void
     */
    protected function initialize(InputInterface $input, OutputInterface $output)
    {}

    /**
     * Runs the command.
     *
     * The code to execute is either defined directly with the
     * setCode() method or by overriding the execute() method
     * in a sub-class.
     *
     * @return int The command exit code
     *
     * @throws ExceptionInterface When input binding fails. Bypass this by calling {@link ignoreValidationErrors()}.
     *
     * @see setCode()
     * @see execute()
     */
    public function run(InputInterface $input, OutputInterface $output): int
    {}

    /**
     * Supplies suggestions when resolving possible completion options for input (e.g. option or argument).
     */
    public function complete(CompletionInput $input, CompletionSuggestions $suggestions): void
    {}

    /**
     * Gets the code that is executed by the command.
     *
     * @return ?callable null if the code has not been set with setCode()
     */
    public function getCode(): ?callable
    {}

    /**
     * Sets the code to execute when running this command.
     *
     * If this method is used, it overrides the code defined
     * in the execute() method.
     *
     * @param callable $code A callable(InputInterface $input, OutputInterface $output)
     *
     * @return $this
     *
     * @throws InvalidArgumentException
     *
     * @see execute()
     */
    public function setCode(callable $code): static
    {}

    /**
     * Merges the application definition with the command definition.
     *
     * This method is not part of public API and should not be used directly.
     *
     * @param bool $mergeArgs Whether to merge or not the Application definition arguments to Command definition arguments
     *
     * @internal
     */
    public function mergeApplicationDefinition(bool $mergeArgs = true): void
    {}

    /**
     * Sets an array of argument and option instances.
     *
     * @return $this
     */
    public function setDefinition(array|InputDefinition $definition): static
    {}

    /**
     * Gets the InputDefinition attached to this Command.
     */
    public function getDefinition(): InputDefinition
    {}

    /**
     * Gets the InputDefinition to be used to create representations of this Command.
     *
     * Can be overridden to provide the original command representation when it would otherwise
     * be changed by merging with the application InputDefinition.
     *
     * This method is not part of public API and should not be used directly.
     */
    public function getNativeDefinition(): InputDefinition
    {}

    /**
     * Adds an argument.
     *
     * @param                                                                               $mode            The argument mode: InputArgument::REQUIRED or InputArgument::OPTIONAL
     * @param                                                                               $default         The default value (for InputArgument::OPTIONAL mode only)
     * @param array|\Closure(CompletionInput,CompletionSuggestions):list<string|Suggestion> $suggestedValues The values used for input completion
     *
     * @return $this
     *
     * @throws InvalidArgumentException When argument mode is not valid
     */
    public function addArgument(string $name, ?int $mode = null, string $description = '', mixed $default = null, array|\Closure $suggestedValues = []): static
    {}

    /**
     * Adds an option.
     *
     * @param                                                                               $shortcut        The shortcuts, can be null, a string of shortcuts delimited by | or an array of shortcuts
     * @param                                                                               $mode            The option mode: One of the InputOption::VALUE_* constants
     * @param                                                                               $default         The default value (must be null for InputOption::VALUE_NONE)
     * @param array|\Closure(CompletionInput,CompletionSuggestions):list<string|Suggestion> $suggestedValues The values used for input completion
     *
     * @return $this
     *
     * @throws InvalidArgumentException If option mode is invalid or incompatible
     */
    public function addOption(string $name, string|array|null $shortcut = null, ?int $mode = null, string $description = '', mixed $default = null, array|\Closure $suggestedValues = []): static
    {}

    /**
     * Sets the name of the command.
     *
     * This method can set both the namespace and the name if
     * you separate them by a colon (:)
     *
     *     $command->setName('foo:bar');
     *
     * @return $this
     *
     * @throws InvalidArgumentException When the name is invalid
     */
    public function setName(string $name): static
    {}

    /**
     * Sets the process title of the command.
     *
     * This feature should be used only when creating a long process command,
     * like a daemon.
     *
     * @return $this
     */
    public function setProcessTitle(string $title): static
    {}

    /**
     * Returns the command name.
     */
    public function getName(): ?string
    {}

    /**
     * @param bool $hidden Whether or not the command should be hidden from the list of commands
     *
     * @return $this
     */
    public function setHidden(bool $hidden = true): static
    {}

    /**
     * @return bool whether the command should be publicly shown or not
     */
    public function isHidden(): bool
    {}

    /**
     * Sets the description for the command.
     *
     * @return $this
     */
    public function setDescription(string $description): static
    {}

    /**
     * Returns the description for the command.
     */
    public function getDescription(): string
    {}

    /**
     * Sets the help for the command.
     *
     * @return $this
     */
    public function setHelp(string $help): static
    {}

    /**
     * Returns the help for the command.
     */
    public function getHelp(): string
    {}

    /**
     * Returns the processed help for the command replacing the %command.name% and
     * %command.full_name% patterns with the real values dynamically.
     */
    public function getProcessedHelp(): string
    {}

    /**
     * Sets the aliases for the command.
     *
     * @param string[] $aliases An array of aliases for the command
     *
     * @return $this
     *
     * @throws InvalidArgumentException When an alias is invalid
     */
    public function setAliases(iterable $aliases): static
    {}

    /**
     * Returns the aliases for the command.
     */
    public function getAliases(): array
    {}

    /**
     * Returns the synopsis for the command.
     *
     * @param bool $short Whether to show the short version of the synopsis (with options folded) or not
     */
    public function getSynopsis(bool $short = false): string
    {}

    /**
     * Add a command usage example, it'll be prefixed with the command name.
     *
     * @return $this
     */
    public function addUsage(string $usage): static
    {}

    /**
     * Returns alternative usages of the command.
     */
    public function getUsages(): array
    {}

    /**
     * Gets a helper instance by name.
     *
     * @throws LogicException           if no HelperSet is defined
     * @throws InvalidArgumentException if the helper is not defined
     */
    public function getHelper(string $name): HelperInterface
    {}

    public function getSubscribedSignals(): array
    {}

    public function handleSignal(int $signal, int|false $previousExitCode = 0): int|false
    {}

    /**
     * Validates a command name.
     *
     * It must be non-empty and parts can optionally be separated by ":".
     *
     * @throws InvalidArgumentException When the name is invalid
     */
    private function validateName(string $name): void
    {}
}
