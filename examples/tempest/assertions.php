<?php

declare(strict_types=1);

use App\Book;
use App\BookController;
use Tempest\View\Renderers\TempestViewRenderer;
use Tempest\View\ViewConfig;

use function Tempest\View\view;

require __DIR__ . '/vendor/autoload.php';

$config = new ViewConfig();
$config->addViewComponents(__DIR__ . '/app/components/x-card.view.php');
$renderer = TempestViewRenderer::make(viewConfig: $config);

$html = $renderer->render(new BookController()->index());
foreach (['<h2>The PHP Book</h2>', '<p>Read The PHP Book</p>'] as $expected) {
    if (!str_contains($html, $expected)) {
        throw new RuntimeException("Missing rendered content: {$expected}");
    }
}
if (str_contains($html, 'No books yet.')) {
    throw new RuntimeException('The nonempty loop rendered its forelse branch.');
}
$empty = $renderer->render(view(__DIR__ . '/app/books.view.php', books: []));
if (!str_contains($empty, 'No books yet.')) {
    throw new RuntimeException('The empty loop did not render its forelse branch.');
}

$branches = $renderer->render(view(
    __DIR__ . '/app/branches.view.php',
    books: [new Book('Named'), new Book('')],
));
if (!str_contains($branches, '<p>Read Named</p>') || !str_contains($branches, '<p>Untitled book.</p>')) {
    throw new RuntimeException('A sibling else did not remain inside its foreach.');
}

echo "Tempest view assertions passed.\n";
