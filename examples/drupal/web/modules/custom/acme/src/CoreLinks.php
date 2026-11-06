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
}
