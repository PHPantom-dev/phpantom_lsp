<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Bundle\FrameworkBundle\Test;

use PHPUnit\Framework\Constraint\Constraint;
use PHPUnit\Framework\Constraint\LogicalAnd;
use PHPUnit\Framework\Constraint\LogicalNot;
use PHPUnit\Framework\ExpectationFailedException;
use Symfony\Component\BrowserKit\AbstractBrowser;
use Symfony\Component\BrowserKit\History;
use Symfony\Component\BrowserKit\Request as BrowserKitRequest;
use Symfony\Component\BrowserKit\Response as BrowserKitResponse;
use Symfony\Component\BrowserKit\Test\Constraint as BrowserKitConstraint;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpFoundation\Test\Constraint as ResponseConstraint;

/**
 * Ideas borrowed from Laravel Dusk's assertions.
 *
 * @see https://laravel.com/docs/5.7/dusk#available-assertions
 */
trait BrowserKitAssertionsTrait
{
    private static bool $defaultVerboseMode = true;

    public static function setBrowserKitAssertionsAsVerbose(bool $verbose): void
    {}

    public static function assertResponseIsSuccessful(string $message = '', ?bool $verbose = null): void
    {}

    public static function assertResponseStatusCodeSame(int $expectedCode, string $message = '', ?bool $verbose = null): void
    {}

    public static function assertResponseFormatSame(?string $expectedFormat, string $message = '', ?bool $verbose = null): void
    {}

    public static function assertResponseRedirects(?string $expectedLocation = null, ?int $expectedCode = null, string $message = '', ?bool $verbose = null): void
    {}

    public static function assertResponseHasHeader(string $headerName, string $message = ''): void
    {}

    public static function assertResponseNotHasHeader(string $headerName, string $message = ''): void
    {}

    public static function assertResponseHeaderSame(string $headerName, string $expectedValue, string $message = ''): void
    {}

    public static function assertResponseHeaderNotSame(string $headerName, string $expectedValue, string $message = ''): void
    {}

    public static function assertResponseHasCookie(string $name, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertResponseNotHasCookie(string $name, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertResponseCookieValueSame(string $name, string $expectedValue, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertResponseIsUnprocessable(string $message = '', ?bool $verbose = null): void
    {}

    public static function assertBrowserHasCookie(string $name, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertBrowserNotHasCookie(string $name, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertBrowserHistoryIsOnFirstPage(string $message = ''): void
    {}

    public static function assertBrowserHistoryIsNotOnFirstPage(string $message = ''): void
    {}

    public static function assertBrowserHistoryIsOnLastPage(string $message = ''): void
    {}

    public static function assertBrowserHistoryIsNotOnLastPage(string $message = ''): void
    {}

    public static function assertBrowserCookieValueSame(string $name, string $expectedValue, bool $raw = false, string $path = '/', ?string $domain = null, string $message = ''): void
    {}

    public static function assertRequestAttributeValueSame(string $name, string $expectedValue, string $message = ''): void
    {}

    public static function assertRouteSame(string $expectedRoute, array $parameters = [], string $message = ''): void
    {}

    public static function assertThatForResponse(Constraint $constraint, string $message = ''): void
    {}

    public static function assertThatForClient(Constraint $constraint, string $message = ''): void
    {}

    protected static function getClient(?AbstractBrowser $newClient = null): ?AbstractBrowser
    {}

    private static function getResponse(): Response
    {}

    private static function getRequest(): Request
    {}
}
