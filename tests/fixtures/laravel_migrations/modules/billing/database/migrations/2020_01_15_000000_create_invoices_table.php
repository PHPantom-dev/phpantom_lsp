<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

class CreateInvoicesTable extends Migration
{
    protected $connection = 'billing';

    public function up(): void
    {
        Schema::create('invoices', static function (Blueprint $table) {
            $table->id();
            $table->decimal('total')->nullable(false);
            $table->json('metadata')->nullable();
        });
    }
}
