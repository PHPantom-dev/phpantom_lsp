<?php

namespace App\Jobs;

use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Queue\Queueable;

class SendBakeryReport implements ShouldQueue
{
    use Queueable;

    public function __construct()
    {
        // Try: complete or navigate redis against config/queue.php.
        $this->connection = 'redis';
    }

    public function handle(): void
    {
    }
}
