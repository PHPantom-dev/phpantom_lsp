<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\HttpKernel;

use Symfony\Component\BrowserKit\AbstractBrowser;
use Symfony\Component\BrowserKit\CookieJar;
use Symfony\Component\BrowserKit\History;
use Symfony\Component\BrowserKit\Request as DomRequest;
use Symfony\Component\BrowserKit\Response as DomResponse;
use Symfony\Component\HttpFoundation\File\UploadedFile;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;

/**
 * Simulates a browser and makes requests to an HttpKernel instance.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 *
 * @template-extends AbstractBrowser<Request, Response>
 */
class HttpKernelBrowser extends AbstractBrowser
{
    private bool $catchExceptions = true;
    private ?object $sentResponse = null;
    private string $sentContent = '';

    /**
     * @param array $server The server parameters (equivalent of $_SERVER)
     */
    public function __construct(
        protected HttpKernelInterface $kernel,
        array $server = [],
        ?History $history = null,
        ?CookieJar $cookieJar = null,
    ) {}

    /**
     * Sets whether to catch exceptions when the kernel is handling a request.
     */
    public function catchExceptions(bool $catchExceptions): void
    {}

    /**
     * @param Request $request
     */
    protected function doRequest(object $request): Response
    {}

    /**
     * @param Request $request
     */
    protected function getScript(object $request): string
    {}

    protected function getHandleScript(): string
    {}

    protected function filterRequest(DomRequest $request): Request
    {}

    /**
     * Filters an array of files.
     *
     * This method created test instances of UploadedFile so that the move()
     * method can be called on those instances.
     *
     * If the size of a file is greater than the allowed size (from php.ini) then
     * an invalid UploadedFile is returned with an error set to UPLOAD_ERR_INI_SIZE.
     *
     * @see UploadedFile
     */
    protected function filterFiles(array $files): array
    {}

    /**
     * @param Response $response
     */
    protected function filterResponse(object $response): DomResponse
    {}
}
