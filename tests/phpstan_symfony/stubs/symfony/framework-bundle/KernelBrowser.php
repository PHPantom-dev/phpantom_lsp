<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Bundle\FrameworkBundle;

use Symfony\Bundle\FrameworkBundle\Test\TestBrowserToken;
use Symfony\Component\BrowserKit\Cookie;
use Symfony\Component\BrowserKit\CookieJar;
use Symfony\Component\BrowserKit\History;
use Symfony\Component\DependencyInjection\ContainerInterface;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpFoundation\Session\SessionInterface;
use Symfony\Component\HttpKernel\HttpKernelBrowser;
use Symfony\Component\HttpKernel\KernelInterface;
use Symfony\Component\HttpKernel\Profiler\Profile as HttpProfile;
use Symfony\Component\Security\Core\User\UserInterface;

/**
 * Simulates a browser and makes requests to a Kernel object.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class KernelBrowser extends HttpKernelBrowser
{
    private bool $hasPerformedRequest = false;
    private bool $profiler = false;
    private bool $reboot = true;

    public function __construct(KernelInterface $kernel, array $server = [], ?History $history = null, ?CookieJar $cookieJar = null)
    {}

    public function getContainer(): ContainerInterface
    {}

    public function getKernel(): KernelInterface
    {}

    /**
     * Gets the profile associated with the current Response.
     */
    public function getProfile(): HttpProfile|false|null
    {}

    public function getSession(): ?SessionInterface
    {}

    /**
     * Enables the profiler for the very next request.
     *
     * If the profiler is not enabled, the call to this method does nothing.
     */
    public function enableProfiler(): void
    {}

    /**
     * Disables kernel reboot between requests.
     *
     * By default, the Client reboots the Kernel for each request. This method
     * allows to keep the same kernel across requests.
     */
    public function disableReboot(): void
    {}

    /**
     * Enables kernel reboot between requests.
     */
    public function enableReboot(): void
    {}

    /**
     * @param UserInterface        $user
     * @param array<string, mixed> $tokenAttributes
     *
     * @return $this
     */
    public function loginUser(object $user, string $firewallContext = 'main', array $tokenAttributes = []): static
    {}

    /**
     * @param Request $request
     */
    protected function doRequest(object $request): Response
    {}

    /**
     * @param Request $request
     */
    protected function doRequestInProcess(object $request): Response
    {}

    /**
     * Returns the script to execute when the request must be insulated.
     *
     * It assumes that the autoloader is named 'autoload.php' and that it is
     * stored in the same directory as the kernel (this is the case for the
     * Symfony Standard Edition). If this is not your case, create your own
     * client and override this method.
     *
     * @param Request $request
     */
    protected function getScript(object $request): string
    {}
}
