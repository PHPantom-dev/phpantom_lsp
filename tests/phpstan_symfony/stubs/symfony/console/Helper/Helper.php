<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Console\Helper;

use Symfony\Component\Console\Formatter\OutputFormatterInterface;
use Symfony\Component\String\UnicodeString;

/**
 * Helper is the base class for all helper classes.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
abstract class Helper implements HelperInterface
{
    protected ?HelperSet $helperSet = null;

    public function setHelperSet(?HelperSet $helperSet): void
    {}

    public function getHelperSet(): ?HelperSet
    {}

    /**
     * Returns the width of a string, using mb_strwidth if it is available.
     * The width is how many characters positions the string will use.
     */
    public static function width(?string $string): int
    {}

    /**
     * Returns the length of a string, using mb_strlen if it is available.
     * The length is related to how many bytes the string will use.
     */
    public static function length(?string $string): int
    {}

    /**
     * Returns the subset of a string, using mb_substr if it is available.
     */
    public static function substr(?string $string, int $from, ?int $length = null): string
    {}

    public static function formatTime(int|float $secs, int $precision = 1): string
    {}

    public static function formatMemory(int $memory): string
    {}

    public static function removeDecoration(OutputFormatterInterface $formatter, ?string $string): string
    {}
}
