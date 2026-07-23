<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;

/**
 * Artisan command declared through a `$signature` string.
 *
 * PHPantom parses the signature grammar so that:
 *  - the command name (`bakery:sync`) completes / navigates / validates
 *    wherever it is referenced as a string (see `Demo::artisanCommands()`);
 *  - `$this->argument(...)` / `$this->option(...)` below complete and hover
 *    against this same signature, and unknown names are flagged.
 */
class SyncBakeryCommand extends Command
{
    /**
     * The signature encodes one argument and two options, each with an
     * inline `:` description that PHPantom surfaces on hover.
     */
    protected $signature = 'bakery:sync
        {bakery : The bakery ID to synchronise}
        {--fresh : Only synchronise freshly baked loaves}
        {--since= : Only loaves baked since this date}';

    protected $description = 'Synchronise a bakery and its loaves';

    public function handle(): int
    {
        // Own-parameter completion + hover: trigger completion inside the
        // string and only `bakery` is offered; hover shows its description.
        $bakery = $this->argument('bakery');

        // Options complete to `fresh` and `since`; hover shows "takes a
        // value" for `--since`.
        $onlyFresh = $this->option('fresh');
        $since = $this->option('since');

        // Referencing a name that is NOT in the signature above is flagged
        // as `invalid_command_parameter` (uncomment to see the diagnostic):
        // $this->argument('kitchen');

        // `$this->call('...')` inside a command runs another Artisan command,
        // so the command name completes / navigates / validates too.
        $this->call('reports:generate', ['--format' => 'json']);

        $this->info("Synced bakery {$bakery} (fresh={$onlyFresh}, since={$since})");

        return self::SUCCESS;
    }
}
