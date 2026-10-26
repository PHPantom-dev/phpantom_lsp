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

use PHPUnit\Framework\TestCase;
use Symfony\Component\Config\Resource\SelfCheckingResourceChecker;
use Symfony\Component\DependencyInjection\Container;
use Symfony\Component\DependencyInjection\Exception\ServiceNotFoundException;
use Symfony\Component\Filesystem\Filesystem;
use Symfony\Component\HttpKernel\Kernel;
use Symfony\Component\HttpKernel\KernelInterface;
use Symfony\Contracts\Service\ResetInterface;

/**
 * KernelTestCase is the base class for tests needing a Kernel.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
abstract class KernelTestCase extends TestCase
{
    use MailerAssertionsTrait;
    use NotificationAssertionsTrait;

    protected static ?string $class = null;
    protected static ?KernelInterface $kernel = null;
    protected static bool $booted = false;

    private static bool $kernelHasBeenRebooted = false;

    protected function tearDown(): void
    {}

    public static function tearDownAfterClass(): void
    {}

    /**
     * @throws \RuntimeException
     * @throws \LogicException
     */
    protected static function getKernelClass(): string
    {}

    /**
     * Boots the Kernel for this test.
     */
    protected static function bootKernel(array $options = []): KernelInterface
    {}

    /**
     * Provides a dedicated test container with access to both public and private
     * services. The container will not include private services that have been
     * inlined or removed. Private services will be removed when they are not
     * used by other services.
     *
     * Using this method is the best way to get a container from your test code.
     */
    protected static function getContainer(): Container
    {}

    /**
     * Creates a Kernel.
     *
     * Available options:
     *
     *  * environment
     *  * debug
     */
    protected static function createKernel(array $options = []): KernelInterface
    {}

    /**
     * Shuts the kernel down if it was used in the test - called by the tearDown method by default.
     */
    protected static function ensureKernelShutdown()
    {}
}
