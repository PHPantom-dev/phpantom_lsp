<?php declare(strict_types = 1);

namespace BrowserKitAssertionTrait;

use Symfony\Bundle\FrameworkBundle\Test\BrowserKitAssertionsTrait;
use Symfony\Component\BrowserKit\AbstractBrowser;
use function PHPStan\Testing\assertType;

class Foo {
    use BrowserKitAssertionsTrait;

	/**
	 * @param mixed $mixed
	 */
    public function test(AbstractBrowser $browser, ?AbstractBrowser $nullableBrowser, $mixed)
    {
        assertType('Symfony\Component\BrowserKit\AbstractBrowser', $this->getClient()); // SKIP: Symfony return types an argument decides
        assertType('null', $this->getClient(null)); // SKIP: Symfony return types an argument decides
        assertType('Symfony\Component\BrowserKit\AbstractBrowser', $this->getClient($browser)); // SKIP: Symfony return types an argument decides
        assertType('Symfony\Component\BrowserKit\AbstractBrowser|null', $this->getClient($nullableBrowser));
        assertType('Symfony\Component\BrowserKit\AbstractBrowser|null', $this->getClient($mixed));

        assertType('Symfony\Component\BrowserKit\AbstractBrowser', $this->getclient()); // SKIP: Symfony return types an argument decides
        assertType('mixed', $mixed->getClient());
    }
}
