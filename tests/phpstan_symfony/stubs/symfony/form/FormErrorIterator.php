<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Form;

use Symfony\Component\Form\Exception\BadMethodCallException;
use Symfony\Component\Form\Exception\InvalidArgumentException;
use Symfony\Component\Form\Exception\LogicException;
use Symfony\Component\Form\Exception\OutOfBoundsException;
use Symfony\Component\Validator\ConstraintViolation;

/**
 * Iterates over the errors of a form.
 *
 * This class supports recursive iteration. In order to iterate recursively,
 * pass a structure of {@link FormError} and {@link FormErrorIterator} objects
 * to the $errors constructor argument.
 *
 * You can also wrap the iterator into a {@link \RecursiveIteratorIterator} to
 * flatten the recursive structure into a flat list of errors.
 *
 * @author Bernhard Schussek <bschussek@gmail.com>
 *
 * @template T of FormError|FormErrorIterator
 *
 * @implements \ArrayAccess<int, T>
 * @implements \RecursiveIterator<int, T>
 * @implements \SeekableIterator<int, T>
 */
class FormErrorIterator implements \RecursiveIterator, \SeekableIterator, \ArrayAccess, \Countable, \Stringable
{
    /**
     * The prefix used for indenting nested error messages.
     */
    public const INDENTATION = '    ';

    /**
     * @var list<T>
     */
    private array $errors;

    /**
     * @param list<T> $errors
     *
     * @throws InvalidArgumentException If the errors are invalid
     */
    public function __construct(
        private FormInterface $form,
        array $errors,
    ) {}

    /**
     * Returns all iterated error messages as string.
     */
    public function __toString(): string
    {}

    /**
     * Returns the iterated form.
     */
    public function getForm(): FormInterface
    {}

    /**
     * Returns the current element of the iterator.
     *
     * @return T An error or an iterator containing nested errors
     */
    public function current(): FormError|self
    {}

    /**
     * Advances the iterator to the next position.
     */
    public function next(): void
    {}

    /**
     * Returns the current position of the iterator.
     */
    public function key(): int
    {}

    /**
     * Returns whether the iterator's position is valid.
     */
    public function valid(): bool
    {}

    /**
     * Sets the iterator's position to the beginning.
     *
     * This method detects if errors have been added to the form since the
     * construction of the iterator.
     */
    public function rewind(): void
    {}

    /**
     * Returns whether a position exists in the iterator.
     *
     * @param int $position The position
     */
    public function offsetExists(mixed $position): bool
    {}

    /**
     * Returns the element at a position in the iterator.
     *
     * @param int $position The position
     *
     * @return T
     *
     * @throws OutOfBoundsException If the given position does not exist
     */
    public function offsetGet(mixed $position): FormError|self
    {}

    /**
     * Unsupported method.
     *
     * @throws BadMethodCallException
     */
    public function offsetSet(mixed $position, mixed $value): void
    {}

    /**
     * Unsupported method.
     *
     * @throws BadMethodCallException
     */
    public function offsetUnset(mixed $position): void
    {}

    /**
     * Returns whether the current element of the iterator can be recursed
     * into.
     */
    public function hasChildren(): bool
    {}

    public function getChildren(): self
    {}

    /**
     * Returns the number of elements in the iterator.
     *
     * Note that this is not the total number of errors, if the constructor
     * parameter $deep was set to true! In that case, you should wrap the
     * iterator into a {@link \RecursiveIteratorIterator} with the standard mode
     * {@link \RecursiveIteratorIterator::LEAVES_ONLY} and count the result.
     *
     *     $iterator = new \RecursiveIteratorIterator($form->getErrors(true));
     *     $count = count(iterator_to_array($iterator));
     *
     * Alternatively, set the constructor argument $flatten to true as well.
     *
     *     $count = count($form->getErrors(true, true));
     */
    public function count(): int
    {}

    /**
     * Sets the position of the iterator.
     *
     * @throws OutOfBoundsException If the position is invalid
     */
    public function seek(int $position): void
    {}

    /**
     * Creates iterator for errors with specific codes.
     *
     * @param string|string[] $codes The codes to find
     */
    public function findByCodes(string|array $codes): static
    {}

    /**
     * Utility function for indenting multi-line strings.
     */
    private static function indent(string $string): string
    {}
}
