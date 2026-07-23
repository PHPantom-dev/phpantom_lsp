<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Symfony\Component\Console\Attribute\AsCommand;

/**
 * Artisan command whose name comes from an `#[AsCommand]` attribute.
 *
 * PHPantom recovers the command name (`reports:generate`) from the attribute
 * — the third supported declaration surface alongside `$signature` and
 * `$name`. The `$signature` here still contributes the `--format` option for
 * own-parameter and array-key completion.
 */
#[AsCommand(name: 'reports:generate')]
class GenerateReportCommand extends Command
{
    protected $signature = 'reports:generate {--format=json : Output format (json|csv)}';

    protected $description = 'Generate a bakery report';

    public function handle(): int
    {
        $format = $this->option('format');

        $this->info("Generating report as {$format}");

        return self::SUCCESS;
    }
}
