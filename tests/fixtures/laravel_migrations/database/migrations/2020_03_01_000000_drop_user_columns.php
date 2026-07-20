<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

class DropUserColumns extends Migration
{
    public function up(): void
    {
        Schema::table('users', static function (Blueprint $table) {
            $table->dropColumn(['address1']);
        });
    }
}
