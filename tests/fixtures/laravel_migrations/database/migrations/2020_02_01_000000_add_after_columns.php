<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

class AddAfterColumns extends Migration
{
    public function up(): void
    {
        Schema::table('users', static function (Blueprint $table) {
            $table->after('name', function (Blueprint $table) {
                $table->ipAddress();
                $table->macAddress('custom_mac_address');
                $table->uuid();
                $table->ulid('custom_ulid');
            });
        });
    }
}
