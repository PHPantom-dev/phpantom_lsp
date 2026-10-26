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

use Symfony\Component\Console\Cursor;
use Symfony\Component\Console\Exception\MissingInputException;
use Symfony\Component\Console\Exception\RuntimeException;
use Symfony\Component\Console\Formatter\OutputFormatter;
use Symfony\Component\Console\Formatter\OutputFormatterStyle;
use Symfony\Component\Console\Input\InputInterface;
use Symfony\Component\Console\Input\StreamableInputInterface;
use Symfony\Component\Console\Output\ConsoleOutputInterface;
use Symfony\Component\Console\Output\ConsoleSectionOutput;
use Symfony\Component\Console\Output\OutputInterface;
use Symfony\Component\Console\Question\ChoiceQuestion;
use Symfony\Component\Console\Question\Question;
use Symfony\Component\Console\Terminal;

use function Symfony\Component\String\s;

/**
 * The QuestionHelper class provides helpers to interact with the user.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class QuestionHelper extends Helper
{
    private static bool $stty = true;
    private static bool $stdinIsInteractive;

    /**
     * Asks a question to the user.
     *
     * @return mixed The user answer
     *
     * @throws RuntimeException If there is no data to read in the input stream
     */
    public function ask(InputInterface $input, OutputInterface $output, Question $question): mixed
    {}

    public function getName(): string
    {}

    /**
     * Prevents usage of stty.
     */
    public static function disableStty(): void
    {}

    /**
     * Asks the question to the user.
     *
     * @param resource $inputStream
     *
     * @throws RuntimeException In case the fallback is deactivated and the response cannot be hidden
     */
    private function doAsk($inputStream, OutputInterface $output, Question $question): mixed
    {}

    private function getDefaultAnswer(Question $question): mixed
    {}

    /**
     * Outputs the question prompt.
     */
    protected function writePrompt(OutputInterface $output, Question $question): void
    {}

    /**
     * @return string[]
     */
    protected function formatChoiceQuestionChoices(ChoiceQuestion $question, string $tag): array
    {}

    /**
     * Outputs an error message.
     */
    protected function writeError(OutputInterface $output, \Exception $error): void
    {}

    /**
     * Autocompletes a question.
     *
     * @param resource                      $inputStream
     * @param callable(string):list<string> $autocomplete
     */
    private function autocomplete(OutputInterface $output, Question $question, $inputStream, callable $autocomplete): string
    {}

    private function mostRecentlyEnteredValue(string $entered): string
    {}

    /**
     * Gets a hidden response from user.
     *
     * @param resource $inputStream The handler resource
     * @param bool     $trimmable   Is the answer trimmable
     *
     * @throws RuntimeException In case the fallback is deactivated and the response cannot be hidden
     */
    private function getHiddenResponse(OutputInterface $output, $inputStream, bool $trimmable = true): string
    {}

    /**
     * Validates an attempt.
     *
     * @param callable $interviewer A callable that will ask for a question and return the result
     *
     * @throws \Exception In case the max number of attempts has been reached and no valid response has been given
     */
    private function validateAttempts(callable $interviewer, OutputInterface $output, Question $question): mixed
    {}

    private function isInteractiveInput($inputStream): bool
    {}

    /**
     * Reads one or more lines of input and returns what is read.
     *
     * @param resource $inputStream The handler resource
     * @param Question $question    The question being asked
     */
    private function readInput($inputStream, Question $question): string|false
    {}

    private function setIOCodepage(): int
    {}

    /**
     * Sets console I/O to the specified code page and converts the user input.
     */
    private function resetIOCodepage(int $cp, string|false $input): string|false
    {}

    /**
     * Clones an input stream in order to act on one instance of the same
     * stream without affecting the other instance.
     *
     * @param resource $inputStream The handler resource
     *
     * @return resource|null The cloned resource, null in case it could not be cloned
     */
    private function cloneInputStream($inputStream)
    {}

    /**
     * @param resource $inputStream
     */
    private function doReadInput($inputStream, ?string $exitChar = null, ?TerminalInputHelper $helper = null): string
    {}
}
