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
use Symfony\Component\Translation\Translator;

/**
 * Wraps errors in forms.
 *
 * @author Bernhard Schussek <bschussek@gmail.com>
 */
class FormError
{
    protected string $messageTemplate;

    /**
     * The form that spawned this error.
     */
    private ?FormInterface $origin = null;

    /**
     * Any array key in $messageParameters will be used as a placeholder in
     * $messageTemplate.
     *
     * @param string      $message              The translated error message
     * @param string|null $messageTemplate      The template for the error message
     * @param array       $messageParameters    The parameters that should be
     *                                          substituted in the message template
     * @param int|null    $messagePluralization The value for error message pluralization
     * @param mixed       $cause                The cause of the error
     *
     * @see Translator
     */
    public function __construct(
        private string $message,
        ?string $messageTemplate = null,
        protected array $messageParameters = [],
        protected ?int $messagePluralization = null,
        private mixed $cause = null,
    ) {}

    /**
     * Returns the error message.
     */
    public function getMessage(): string
    {}

    /**
     * Returns the error message template.
     */
    public function getMessageTemplate(): string
    {}

    /**
     * Returns the parameters to be inserted in the message template.
     */
    public function getMessageParameters(): array
    {}

    /**
     * Returns the value for error message pluralization.
     */
    public function getMessagePluralization(): ?int
    {}

    /**
     * Returns the cause of this error.
     */
    public function getCause(): mixed
    {}

    /**
     * Sets the form that caused this error.
     *
     * This method must only be called once.
     *
     * @throws BadMethodCallException If the method is called more than once
     */
    public function setOrigin(FormInterface $origin): void
    {}

    /**
     * Returns the form that caused this error.
     */
    public function getOrigin(): ?FormInterface
    {}
}
