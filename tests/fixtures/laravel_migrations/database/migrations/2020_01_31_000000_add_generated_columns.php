<?php

declare(strict_types=1);

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

class AddGeneratedColumns extends Migration
{
    public function up(): void
    {
        Schema::table('users', static function (Blueprint $table) {
            $table->string('display_name')->virtualAs("concat(name, ' <', email, '>')");
            $table->string('email_hash')->storedAs('md5(email)');
        });
    }
}
