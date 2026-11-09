<?php

declare(strict_types=1);

namespace App;

use function Tempest\Database\query;

final class DatabaseQueries
{
    public function create(): DatabaseBook
    {
        // Try: complete or follow title: and isbn: to DatabaseBook's properties.
        return DatabaseBook::create(title: 'The PHP Book', isbn: 'php');
    }

    public function update(): DatabaseBook
    {
        // Try: complete or follow the array keys to the model's properties.
        return DatabaseBook::updateOrCreate(['isbn' => 'php'], ['title' => 'Updated PHP']);
    }

    public function draft(): DatabaseBook
    {
        return DatabaseBook::findOrNew(['isbn' => 'draft'], ['title' => 'Draft PHP']);
    }

    /** @return DatabaseBook[] */
    public function books(): array
    {
        // Try: complete or follow field names through a typed builder chain.
        // Passing the class name keeps all() typed as DatabaseBook[].
        return query(DatabaseBook::class)
            ->select()
            ->whereIn('isbn', ['php'])
            ->where('title', 'Updated PHP')
            ->whereNull('publishedAt')
            ->orderBy('title')
            ->all();
    }

    public function firstBook(): ?DatabaseBook
    {
        // Try: hover first(); the class name produces a DatabaseBook or null.
        return query(DatabaseBook::class)->select()->where('isbn', 'php')->first();
    }
}
