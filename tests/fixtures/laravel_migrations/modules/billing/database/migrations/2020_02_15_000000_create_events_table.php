<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

class CreateEventsTable extends Migration
{
    public function up(): void
    {
        Schema::connection('analytics')->create('events', static function (Blueprint $table) {
            $table->uuid('uuid');
            $table->timestamp('occurred_at')->nullable();
        });
    }
}
