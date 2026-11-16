<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\Core\Url;

/**
 * Links to pages whose routes core builds without a routing file.
 */
final class CoreLinks
{
    /**
     * @return list<Url>
     */
    public function urls(): array
    {
        // Try: neither route name is reported, though no routing file
        // declares it. Core's entity route provider builds
        // entity.<type>.canonical for every entity type with a canonical
        // link, and Views builds view.<view>.<display> for a page display,
        // here the page_1 display of node's content view. A name of neither
        // shape, such as 'entity.nod', is reported.
        return [
            Url::fromRoute('entity.node.canonical', ['node' => 1]),
            Url::fromRoute('view.content.page_1'),
        ];
    }

    /**
     * The path of the first node, told plainly and with its cacheability.
     */
    public function firstNodePath(): string
    {
        $url = Url::fromRoute('entity.node.canonical', ['node' => 1]);

        // Try: core declares toString() string|GeneratedUrl, but without
        // TRUE it returns a plain string, so passing it to describe() is not
        // reported. With TRUE it returns a GeneratedUrl: complete after
        // `$generated->` to reach getGeneratedUrl().
        $plain = $this->describe($url->toString());
        $generated = $url->toString(TRUE);

        return $plain . ' ' . $generated->getGeneratedUrl();
    }

    private function describe(string $path): string
    {
        return 'Path: ' . $path;
    }
}
