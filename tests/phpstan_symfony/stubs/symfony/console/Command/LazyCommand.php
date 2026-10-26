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
use Symfony\Component\Console\Completion\CompletionInput;
use Symfony\Component\Console\Completion\CompletionSuggestions;
use Symfony\Component\Console\Completion\Suggestion;
use Symfony\Component\Console\Helper\HelperInterface;
use Symfony\Component\Console\Helper\HelperSet;
use Symfony\Component\Console\Input\InputDefinition;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Output\OutputInterface;

/**
 * @author Nicolas Grekas <p@tchwork.com>
 */
final class LazyCommand extends Command
{
    private \Closure|Command $command;

    public function __construct(
        string $name,
        array $aliases,
        string $description,
        bool $isHidden,
        \Closure $commandFactory,
        private ?bool $isEnabled = true,
    ) {}

    public function ignoreValidationErrors(): void
    {}

    public function setApplication(?Application $application): void
    {}

    public function setHelperSet(HelperSet $helperSet): void
    {}

    public function isEnabled(): bool
    {}

    public function run(InputInterface $input, OutputInterface $output): int
    {}

    public function complete(CompletionInput $input, CompletionSuggestions $suggestions): void
    {}

    public function setCode(callable $code): static
    {}

    /**
     * @internal
     */
    public function mergeApplicationDefinition(bool $mergeArgs = true): void
    {}

    public function setDefinition(array|InputDefinition $definition): static
    {}

    public function getDefinition(): InputDefinition
    {}

    public function getNativeDefinition(): InputDefinition
    {}

    /**
     * @param array|\Closure(CompletionInput,CompletionSuggestions):list<string|Suggestion> $suggestedValues The values used for input completion
     */
    public function addArgument(string $name, ?int $mode = null, string $description = '', mixed $default = null, array|\Closure $suggestedValues = []): static
    {}

    /**
     * @param array|\Closure(CompletionInput,CompletionSuggestions):list<string|Suggestion> $suggestedValues The values used for input completion
     */
    public function addOption(string $name, string|array|null $shortcut = null, ?int $mode = null, string $description = '', mixed $default = null, array|\Closure $suggestedValues = []): static
    {}

    public function setProcessTitle(string $title): static
    {}

    public function setHelp(string $help): static
    {}

    public function getHelp(): string
    {}

    public function getProcessedHelp(): string
    {}

    public function getSynopsis(bool $short = false): string
    {}

    public function addUsage(string $usage): static
    {}

    public function getUsages(): array
    {}

    public function getHelper(string $name): HelperInterface
    {}

    public function getCommand(): parent
    {}
}
