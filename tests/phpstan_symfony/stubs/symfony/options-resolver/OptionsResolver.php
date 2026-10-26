<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\OptionsResolver;

use Symfony\Component\OptionsResolver\Exception\AccessException;
use Symfony\Component\OptionsResolver\Exception\InvalidArgumentException;
use Symfony\Component\OptionsResolver\Exception\InvalidOptionsException;
use Symfony\Component\OptionsResolver\Exception\MissingOptionsException;
use Symfony\Component\OptionsResolver\Exception\NoSuchOptionException;
use Symfony\Component\OptionsResolver\Exception\OptionDefinitionException;
use Symfony\Component\OptionsResolver\Exception\UndefinedOptionsException;

/**
 * Validates options and merges them with default values.
 *
 * @author Bernhard Schussek <bschussek@gmail.com>
 * @author Tobias Schultze <http://tobion.de>
 */
class OptionsResolver implements Options
{
    private const VALIDATION_FUNCTIONS = [
        'bool' => 'is_bool',
        'boolean' => 'is_bool',
        'int' => 'is_int',
        'integer' => 'is_int',
        'long' => 'is_int',
        'float' => 'is_float',
        'double' => 'is_float',
        'real' => 'is_float',
        'numeric' => 'is_numeric',
        'string' => 'is_string',
        'scalar' => 'is_scalar',
        'array' => 'is_array',
        'iterable' => 'is_iterable',
        'countable' => 'is_countable',
        'callable' => 'is_callable',
        'object' => 'is_object',
        'resource' => 'is_resource',
    ];

    /**
     * The names of all defined options.
     */
    private array $defined = [];

    /**
     * The default option values.
     */
    private array $defaults = [];

    /**
     * A list of closure for nested options.
     *
     * @var \Closure[][]
     */
    private array $nested = [];

    /**
     * BC layer. Remove in Symfony 8.0.
     *
     * @var array<string, true>
     */
    private array $deprecatedNestedOptions = [];

    /**
     * The names of required options.
     */
    private array $required = [];

    /**
     * The resolved option values.
     */
    private array $resolved = [];

    /**
     * A list of normalizer closures.
     *
     * @var \Closure[][]
     */
    private array $normalizers = [];

    /**
     * A list of accepted values for each option.
     */
    private array $allowedValues = [];

    /**
     * A list of accepted types for each option.
     */
    private array $allowedTypes = [];

    /**
     * A list of info messages for each option.
     */
    private array $info = [];

    /**
     * A list of closures for evaluating lazy options.
     */
    private array $lazy = [];

    /**
     * A list of lazy options whose closure is currently being called.
     *
     * This list helps detecting circular dependencies between lazy options.
     */
    private array $calling = [];

    /**
     * A list of deprecated options.
     */
    private array $deprecated = [];

    /**
     * The list of options provided by the user.
     */
    private array $given = [];

    /**
     * Whether the instance is locked for reading.
     *
     * Once locked, the options cannot be changed anymore. This is
     * necessary in order to avoid inconsistencies during the resolving
     * process. If any option is changed after being read, all evaluated
     * lazy options that depend on this option would become invalid.
     */
    private bool $locked = false;

    private array $parentsOptions = [];

    /**
     * Whether the whole options definition is marked as array prototype.
     */
    private ?bool $prototype = null;

    /**
     * The prototype array's index that is being read.
     */
    private int|string|null $prototypeIndex = null;

    /**
     * Whether to ignore undefined options.
     */
    private bool $ignoreUndefined = false;

    /**
     * Sets the default value of a given option.
     *
     * If the default value should be set based on other options, you can pass
     * a closure with the following signature:
     *
     *     function (Options $options) {
     *         // ...
     *     }
     *
     * The closure will be evaluated when {@link resolve()} is called. The
     * closure has access to the resolved values of other options through the
     * passed {@link Options} instance:
     *
     *     function (Options $options) {
     *         if (isset($options['port'])) {
     *             // ...
     *         }
     *     }
     *
     * If you want to access the previously set default value, add a second
     * argument to the closure's signature:
     *
     *     $options->setDefault('name', 'Default Name');
     *
     *     $options->setDefault('name', function (Options $options, $previousValue) {
     *         // 'Default Name' === $previousValue
     *     });
     *
     * This is mostly useful if the configuration of the {@link Options} object
     * is spread across different locations of your code, such as base and
     * sub-classes.
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function setDefault(string $option, mixed $value): static
    {}

    /**
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function setDefaults(array $defaults): static
    {}

    /**
     * Returns whether a default value is set for an option.
     *
     * Returns true if {@link setDefault()} was called for this option.
     * An option is also considered set if it was set to null.
     */
    public function hasDefault(string $option): bool
    {}

    /**
     * Marks one or more options as required.
     *
     * @param string|string[] $optionNames One or more option names
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function setRequired(string|array $optionNames): static
    {}

    /**
     * Returns whether an option is required.
     *
     * An option is required if it was passed to {@link setRequired()}.
     */
    public function isRequired(string $option): bool
    {}

    /**
     * Returns the names of all required options.
     *
     * @return string[]
     *
     * @see isRequired()
     */
    public function getRequiredOptions(): array
    {}

    /**
     * Returns whether an option is missing a default value.
     *
     * An option is missing if it was passed to {@link setRequired()}, but not
     * to {@link setDefault()}. This option must be passed explicitly to
     * {@link resolve()}, otherwise an exception will be thrown.
     */
    public function isMissing(string $option): bool
    {}

    /**
     * Returns the names of all options missing a default value.
     *
     * @return string[]
     */
    public function getMissingOptions(): array
    {}

    /**
     * Defines a valid option name.
     *
     * Defines an option name without setting a default value. The option will
     * be accepted when passed to {@link resolve()}. When not passed, the
     * option will not be included in the resolved options.
     *
     * @param string|string[] $optionNames One or more option names
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function setDefined(string|array $optionNames): static
    {}

    /**
     * Returns whether an option is defined.
     *
     * Returns true for any option passed to {@link setDefault()},
     * {@link setRequired()} or {@link setDefined()}.
     */
    public function isDefined(string $option): bool
    {}

    /**
     * Returns the names of all defined options.
     *
     * @return string[]
     *
     * @see isDefined()
     */
    public function getDefinedOptions(): array
    {}

    /**
     * Defines nested options.
     *
     * @param \Closure(self $resolver, Options $parent): void $nested
     *
     * @return $this
     */
    public function setOptions(string $option, \Closure $nested): static
    {}

    public function isNested(string $option): bool
    {}

    /**
     * Deprecates an option, allowed types or values.
     *
     * Instead of passing the message, you may also pass a closure with the
     * following signature:
     *
     *     function (Options $options, $value): string {
     *         // ...
     *     }
     *
     * The closure receives the value as argument and should return a string.
     * Return an empty string to ignore the option deprecation.
     *
     * The closure is invoked when {@link resolve()} is called. The parameter
     * passed to the closure is the value of the option after validating it
     * and before normalizing it.
     *
     * @param string          $package The name of the composer package that is triggering the deprecation
     * @param string          $version The version of the package that introduced the deprecation
     * @param string|\Closure $message The deprecation message to use
     *
     * @return $this
     */
    public function setDeprecated(string $option, string $package, string $version, string|\Closure $message = 'The option "%name%" is deprecated.'): static
    {}

    public function isDeprecated(string $option): bool
    {}

    /**
     * Sets the normalizer for an option.
     *
     * The normalizer should be a closure with the following signature:
     *
     *     function (Options $options, $value) {
     *         // ...
     *     }
     *
     * The closure is invoked when {@link resolve()} is called. The closure
     * has access to the resolved values of other options through the passed
     * {@link Options} instance.
     *
     * The second parameter passed to the closure is the value of
     * the option.
     *
     * The resolved option value is set to the return value of the closure.
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function setNormalizer(string $option, \Closure $normalizer): static
    {}

    /**
     * Adds a normalizer for an option.
     *
     * The normalizer should be a closure with the following signature:
     *
     *     function (Options $options, $value): mixed {
     *         // ...
     *     }
     *
     * The closure is invoked when {@link resolve()} is called. The closure
     * has access to the resolved values of other options through the passed
     * {@link Options} instance.
     *
     * The second parameter passed to the closure is the value of
     * the option.
     *
     * The resolved option value is set to the return value of the closure.
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function addNormalizer(string $option, \Closure $normalizer, bool $forcePrepend = false): static
    {}

    /**
     * Sets allowed values for an option.
     *
     * Instead of passing values, you may also pass a closures with the
     * following signature:
     *
     *     function ($value) {
     *         // return true or false
     *     }
     *
     * The closure receives the value as argument and should return true to
     * accept the value and false to reject the value.
     *
     * @param mixed $allowedValues One or more acceptable values/closures
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function setAllowedValues(string $option, mixed $allowedValues): static
    {}

    /**
     * Adds allowed values for an option.
     *
     * The values are merged with the allowed values defined previously.
     *
     * Instead of passing values, you may also pass a closures with the
     * following signature:
     *
     *     function ($value) {
     *         // return true or false
     *     }
     *
     * The closure receives the value as argument and should return true to
     * accept the value and false to reject the value.
     *
     * @param mixed $allowedValues One or more acceptable values/closures
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function addAllowedValues(string $option, mixed $allowedValues): static
    {}

    /**
     * Sets allowed types for an option.
     *
     * Any type for which a corresponding is_<type>() function exists is
     * acceptable. Additionally, fully-qualified class or interface names may
     * be passed.
     *
     * @param string|string[] $allowedTypes One or more accepted types
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function setAllowedTypes(string $option, string|array $allowedTypes): static
    {}

    /**
     * Adds allowed types for an option.
     *
     * The types are merged with the allowed types defined previously.
     *
     * Any type for which a corresponding is_<type>() function exists is
     * acceptable. Additionally, fully-qualified class or interface names may
     * be passed.
     *
     * @param string|string[] $allowedTypes One or more accepted types
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function addAllowedTypes(string $option, string|array $allowedTypes): static
    {}

    /**
     * Defines an option configurator with the given name.
     */
    public function define(string $option): OptionConfigurator
    {}

    /**
     * Sets an info message for an option.
     *
     * @return $this
     *
     * @throws UndefinedOptionsException If the option is undefined
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function setInfo(string $option, string $info): static
    {}

    /**
     * Gets the info message for an option.
     */
    public function getInfo(string $option): ?string
    {}

    /**
     * Marks the whole options definition as array prototype.
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option, a normalizer or a root definition
     */
    public function setPrototype(bool $prototype): static
    {}

    public function isPrototype(): bool
    {}

    /**
     * Removes the option with the given name.
     *
     * Undefined options are ignored.
     *
     * @param string|string[] $optionNames One or more option names
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function remove(string|array $optionNames): static
    {}

    /**
     * Removes all options.
     *
     * @return $this
     *
     * @throws AccessException If called from a lazy option or normalizer
     */
    public function clear(): static
    {}

    /**
     * Merges options with the default values stored in the container and
     * validates them.
     *
     * Exceptions are thrown if:
     *
     *  - Undefined options are passed;
     *  - Required options are missing;
     *  - Options have invalid types;
     *  - Options have invalid values.
     *
     * @throws UndefinedOptionsException If an option name is undefined
     * @throws InvalidOptionsException   If an option doesn't fulfill the
     *                                   specified validation rules
     * @throws MissingOptionsException   If a required option is missing
     * @throws OptionDefinitionException If there is a cyclic dependency between
     *                                   lazy options and/or normalizers
     * @throws NoSuchOptionException     If a lazy option reads an unavailable option
     * @throws AccessException           If called from a lazy option or normalizer
     */
    public function resolve(array $options = []): array
    {}

    /**
     * Returns the resolved value of an option.
     *
     * @param bool $triggerDeprecation Whether to trigger the deprecation or not (true by default)
     *
     * @throws AccessException           If accessing this method outside of
     *                                   {@link resolve()}
     * @throws NoSuchOptionException     If the option is not set
     * @throws InvalidOptionsException   If the option doesn't fulfill the
     *                                   specified validation rules
     * @throws OptionDefinitionException If there is a cyclic dependency between
     *                                   lazy options and/or normalizers
     */
    public function offsetGet(mixed $option, bool $triggerDeprecation = true): mixed
    {}

    private function verifyTypes(string $type, mixed $value, ?array &$invalidTypes = null, int $level = 0): bool
    {}

    /**
     * @return list<string>
     */
    private function splitOutsideParenthesis(string $type): array
    {}

    /**
     * Returns whether a resolved option with the given name exists.
     *
     * @throws AccessException If accessing this method outside of {@link resolve()}
     *
     * @see \ArrayAccess::offsetExists()
     */
    public function offsetExists(mixed $option): bool
    {}

    /**
     * Not supported.
     *
     * @throws AccessException
     */
    public function offsetSet(mixed $option, mixed $value): void
    {}

    /**
     * Not supported.
     *
     * @throws AccessException
     */
    public function offsetUnset(mixed $option): void
    {}

    /**
     * Returns the number of set options.
     *
     * This may be only a subset of the defined options.
     *
     * @throws AccessException If accessing this method outside of {@link resolve()}
     *
     * @see \Countable::count()
     */
    public function count(): int
    {}

    /**
     * Sets whether ignore undefined options.
     *
     * @return $this
     */
    public function setIgnoreUndefined(bool $ignore = true): static
    {}

    /**
     * Returns a string representation of the value.
     *
     * This method returns the equivalent PHP tokens for most scalar types
     * (i.e. "false" for false, "1" for 1 etc.). Strings are always wrapped
     * in double quotes (").
     */
    private function formatValue(mixed $value): string
    {}

    /**
     * Returns a string representation of a list of values.
     *
     * Each of the values is converted to a string using
     * {@link formatValue()}. The values are then concatenated with commas.
     *
     * @see formatValue()
     */
    private function formatValues(array $values): string
    {}

    private function formatOptions(array $options): string
    {}

    private function getParameterClassName(\ReflectionParameter $parameter): ?string
    {}
}
