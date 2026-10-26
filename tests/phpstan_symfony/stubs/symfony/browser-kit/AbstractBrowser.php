<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\BrowserKit;

use Symfony\Component\BrowserKit\Exception\BadMethodCallException;
use Symfony\Component\BrowserKit\Exception\InvalidArgumentException;
use Symfony\Component\BrowserKit\Exception\LogicException;
use Symfony\Component\BrowserKit\Exception\RuntimeException;
use Symfony\Component\DomCrawler\Crawler;
use Symfony\Component\DomCrawler\Form;
use Symfony\Component\DomCrawler\Link;
use Symfony\Component\Process\PhpProcess;
use Symfony\Component\Process\Process;

/**
 * Simulates a browser.
 *
 * To make the actual request, you need to implement the doRequest() method.
 *
 * If you want to be able to run requests in their own process (insulated flag),
 * you need to also implement the getScript() method.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 *
 * @template TRequest of object
 * @template TResponse of object
 */
abstract class AbstractBrowser
{
    protected History $history;
    protected CookieJar $cookieJar;
    protected array $server = [];
    protected Request $internalRequest;
    /** @psalm-var TRequest */
    protected object $request;
    protected Response $internalResponse;
    /** @psalm-var TResponse */
    protected object $response;
    protected Crawler $crawler;
    /** @deprecated since Symfony 7.4, to be removed in Symfony 8 */
    protected bool $useHtml5Parser = true;
    protected string|false $wrapContentPattern = false;
    protected bool $insulated = false;
    protected ?string $redirect;
    protected bool $followRedirects = true;
    protected bool $followMetaRefresh = false;

    private int $maxRedirects = -1;
    private int $redirectCount = 0;
    private array $redirects = [];
    private bool $isMainRequest = true;

    /**
     * @param array $server The server parameters (equivalent of $_SERVER)
     */
    public function __construct(array $server = [], ?History $history = null, ?CookieJar $cookieJar = null)
    {}

    /**
     * Sets whether to automatically follow redirects or not.
     */
    public function followRedirects(bool $followRedirects = true): void
    {}

    /**
     * Sets whether to automatically follow meta refresh redirects or not.
     */
    public function followMetaRefresh(bool $followMetaRefresh = true): void
    {}

    /**
     * Returns whether client automatically follows redirects or not.
     */
    public function isFollowingRedirects(): bool
    {}

    /**
     * Sets the maximum number of redirects that crawler can follow.
     */
    public function setMaxRedirects(int $maxRedirects): void
    {}

    /**
     * Returns the maximum number of redirects that crawler can follow.
     */
    public function getMaxRedirects(): int
    {}

    /**
     * Sets the insulated flag.
     *
     * @throws LogicException When Symfony Process Component is not installed
     */
    public function insulate(bool $insulated = true): void
    {}

    /**
     * Sets server parameters.
     */
    public function setServerParameters(array $server): void
    {}

    /**
     * Sets single server parameter.
     */
    public function setServerParameter(string $key, string $value): void
    {}

    /**
     * Gets single server parameter for specified key.
     */
    public function getServerParameter(string $key, mixed $default = ''): mixed
    {}

    public function xmlHttpRequest(string $method, string $uri, array $parameters = [], array $files = [], array $server = [], ?string $content = null, bool $changeHistory = true): Crawler
    {}

    /**
     * Converts the request parameters into a JSON string and uses it as request content.
     */
    public function jsonRequest(string $method, string $uri, array $parameters = [], array $server = [], bool $changeHistory = true): Crawler
    {}

    /**
     * Returns the History instance.
     */
    public function getHistory(): History
    {}

    /**
     * Returns the CookieJar instance.
     */
    public function getCookieJar(): CookieJar
    {}

    /**
     * Returns the current Crawler instance.
     */
    public function getCrawler(): Crawler
    {}

    /**
     * Sets whether parsing should be done using "masterminds/html5".
     *
     * @deprecated since Symfony 7.4, Symfony 8 will unconditionally use the native HTML5 parser
     *
     * @return $this
     */
    public function useHtml5Parser(bool $useHtml5Parser): static
    {}

    /**
     * Sets the content wrapper format.
     *
     * @example <table>%s</table>
     */
    public function wrapContent(false|string $pattern): void
    {}

    /**
     * Returns the current BrowserKit Response instance.
     */
    public function getInternalResponse(): Response
    {}

    /**
     * Returns the current origin response instance.
     *
     * The origin response is the response instance that is returned
     * by the code that handles requests.
     *
     * @psalm-return TResponse
     *
     * @see doRequest()
     */
    public function getResponse(): object
    {}

    /**
     * Returns the current BrowserKit Request instance.
     */
    public function getInternalRequest(): Request
    {}

    /**
     * Returns the current origin Request instance.
     *
     * The origin request is the request instance that is sent
     * to the code that handles requests.
     *
     * @psalm-return TRequest
     *
     * @see doRequest()
     */
    public function getRequest(): object
    {}

    /**
     * Clicks on a given link.
     *
     * @param array $serverParameters An array of server parameters
     */
    public function click(Link $link, array $serverParameters = []): Crawler
    {}

    /**
     * Clicks the first link (or clickable image) that contains the given text.
     *
     * @param string $linkText         The text of the link or the alt attribute of the clickable image
     * @param array  $serverParameters An array of server parameters
     */
    public function clickLink(string $linkText, array $serverParameters = []): Crawler
    {}

    /**
     * Submits a form.
     *
     * @param array $values           An array of form field values
     * @param array $serverParameters An array of server parameters
     */
    public function submit(Form $form, array $values = [], array $serverParameters = []): Crawler
    {}

    /**
     * Finds the first form that contains a button with the given content and
     * uses it to submit the given form field values.
     *
     * @param string $button           The text content, id, value or name of the form <button> or <input type="submit">
     * @param array  $fieldValues      Use this syntax: ['my_form[name]' => '...', 'my_form[email]' => '...']
     * @param string $method           The HTTP method used to submit the form
     * @param array  $serverParameters These values override the ones stored in $_SERVER (HTTP headers must include an HTTP_ prefix as PHP does)
     */
    public function submitForm(string $button, array $fieldValues = [], string $method = 'POST', array $serverParameters = []): Crawler
    {}

    /**
     * Calls a URI.
     *
     * @param string $method        The request method
     * @param string $uri           The URI to fetch
     * @param array  $parameters    The Request parameters
     * @param array  $files         The files
     * @param array  $server        The server parameters (HTTP headers are referenced with an HTTP_ prefix as PHP does)
     * @param string $content       The raw body data
     * @param bool   $changeHistory Whether to update the history or not (only used internally for back(), forward(), and reload())
     */
    public function request(string $method, string $uri, array $parameters = [], array $files = [], array $server = [], ?string $content = null, bool $changeHistory = true): Crawler
    {}

    /**
     * Makes a request in another process.
     *
     * @psalm-param TRequest $request
     *
     * @return object
     *
     * @psalm-return TResponse
     *
     * @throws \RuntimeException When processing returns exit code
     */
    protected function doRequestInProcess(object $request)
    {}

    /**
     * Makes a request.
     *
     * @psalm-param TRequest $request
     *
     * @return object
     *
     * @psalm-return TResponse
     */
    abstract protected function doRequest(object $request);

    /**
     * Returns the script to execute when the request must be insulated.
     *
     * @param object $request An origin request instance
     *
     * @psalm-param TRequest $request
     *
     * @return string
     *
     * @throws LogicException When this abstract class is not implemented
     */
    protected function getScript(object $request)
    {}

    /**
     * Filters the BrowserKit request to the origin one.
     *
     * @return object
     *
     * @psalm-return TRequest
     */
    protected function filterRequest(Request $request)
    {}

    /**
     * Filters the origin response to the BrowserKit one.
     *
     * @psalm-param TResponse $response
     *
     * @return Response
     */
    protected function filterResponse(object $response)
    {}

    /**
     * Creates a crawler.
     *
     * This method returns null if the DomCrawler component is not available.
     */
    protected function createCrawlerFromContent(string $uri, string $content, string $type): ?Crawler
    {}

    /**
     * Goes back in the browser history.
     */
    public function back(): Crawler
    {}

    /**
     * Goes forward in the browser history.
     */
    public function forward(): Crawler
    {}

    /**
     * Reloads the current browser.
     */
    public function reload(): Crawler
    {}

    /**
     * Follow redirects?
     *
     * @throws LogicException If request was not a redirect
     */
    public function followRedirect(): Crawler
    {}

    /**
     * @see https://dev.w3.org/html5/spec-preview/the-meta-element.html#attr-meta-http-equiv-refresh
     */
    private function getMetaRefreshUrl(): ?string
    {}

    /**
     * Restarts the client.
     *
     * It flushes history and all cookies.
     */
    public function restart(): void
    {}

    /**
     * Takes a URI and converts it to absolute if it is not already absolute.
     */
    protected function getAbsoluteUri(string $uri): string
    {}

    /**
     * Makes a request from a Request object directly.
     *
     * @param bool $changeHistory Whether to update the history or not (only used internally for back(), forward(), and reload())
     */
    protected function requestFromRequest(Request $request, bool $changeHistory = true): Crawler
    {}

    private function updateServerFromUri(array $server, string $uri): array
    {}

    private function extractHost(string $uri): ?string
    {}
}

// @php-cs-fixer-ignore error_suppression This file is explicitly expected to not silence each of trigger_error calls
