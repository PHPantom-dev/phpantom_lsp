<?php

declare(strict_types=1);

namespace App;

use Tempest\View\View;

use function Tempest\View\view;

final class DynamicViews
{
    public function example(): View
    {
        // Try: follow the selected component and inspect its inferred book data.
        return view(__DIR__ . '/dynamic.view.php', component: 'x-dynamic-book', book: new Book('Dynamic views'));
    }
}
