<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Console\Input;

use Symfony\Component\Console\Command\Command;
use Symfony\Component\Console\Completion\CompletionInput;
use Symfony\Component\Console\Completion\CompletionSuggestions;
use Symfony\Component\Console\Completion\Suggestion;
use Symfony\Component\Console\Exception\InvalidArgumentException;
use Symfony\Component\Console\Exception\LogicException;

/**
 * Represents a command line option.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class InputOption
{
    /**
     * Do not accept input for the option (e.g. --yell). This is the default behavior of options.
     */
    public const VALUE_NONE = 1;

    /**
     * A value must be passed when the option is used (e.g. --iterations=5 or -i5).
     */
    public const VALUE_REQUIRED = 2;

    /**
     * The option may or may not have a value (e.g. --yell or --yell=loud).
     */
    public const VALUE_OPTIONAL = 4;

    /**
     * The option accepts multiple values (e.g. --dir=/foo --dir=/bar).
     */
    public const VALUE_IS_ARRAY = 8;

    /**
     * The option allows passing a negated variant (e.g. --ansi or --no-ansi).
     */
    public const VALUE_NEGATABLE = 16;

    private string $name;
    private ?string $shortcut;
    private int $mode;
    private string|int|bool|array|float|null $default;

    /**
     * @param string|array|null                                                             $shortcut        The shortcuts, can be null, a string of shortcuts delimited by | or an array of shortcuts
     * @param int-mask-of<InputOption::*>|null                                              $mode            The option mode: One of the VALUE_* constants
     * @param string|bool|int|float|array|null                                              $default         The default value (must be null for self::VALUE_NONE)
     * @param array|\Closure(CompletionInput,CompletionSuggestions):list<string|Suggestion> $suggestedValues The values used for input completion
     *
     * @throws InvalidArgumentException If option mode is invalid or incompatible
     */
    public function __construct(
        string $name,
        string|array|null $shortcut = null,
        ?int $mode = null,
        private string $description = '',
        string|bool|int|float|array|null $default = null,
        private array|\Closure $suggestedValues = [],
    ) {}

    /**
     * Returns the option shortcut.
     */
    public function getShortcut(): ?string
    {}

    /**
     * Returns the option name.
     */
    public function getName(): string
    {}

    /**
     * Returns true if the option accepts a value.
     *
     * @return bool true if value mode is not self::VALUE_NONE, false otherwise
     */
    public function acceptValue(): bool
    {}

    /**
     * Returns true if the option requires a value.
     *
     * @return bool true if value mode is self::VALUE_REQUIRED, false otherwise
     */
    public function isValueRequired(): bool
    {}

    /**
     * Returns true if the option takes an optional value.
     *
     * @return bool true if value mode is self::VALUE_OPTIONAL, false otherwise
     */
    public function isValueOptional(): bool
    {}

    /**
     * Returns true if the option can take multiple values.
     *
     * @return bool true if mode is self::VALUE_IS_ARRAY, false otherwise
     */
    public function isArray(): bool
    {}

    /**
     * Returns true if the option allows passing a negated variant.
     *
     * @return bool true if mode is self::VALUE_NEGATABLE, false otherwise
     */
    public function isNegatable(): bool
    {}

    /**
     * Sets the default value.
     */
    public function setDefault(string|bool|int|float|array|null $default): void
    {}

    /**
     * Returns the default value.
     */
    public function getDefault(): string|bool|int|float|array|null
    {}

    /**
     * Returns the description text.
     */
    public function getDescription(): string
    {}

    /**
     * Returns true if the option has values for input completion.
     */
    public function hasCompletion(): bool
    {}

    /**
     * Supplies suggestions when command resolves possible completion options for input.
     *
     * @see Command::complete()
     */
    public function complete(CompletionInput $input, CompletionSuggestions $suggestions): void
    {}

    /**
     * Checks whether the given option equals this one.
     */
    public function equals(self $option): bool
    {}
}
