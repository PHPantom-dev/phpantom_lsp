<?php

namespace App\Models;

use Illuminate\Foundation\Auth\User as Authenticatable;

/**
 * The authenticated user model, wired up in config/auth.php as the
 * default guard's provider model.
 */
class Customer extends Authenticatable
{
    /** @var list<string> */
    protected $fillable = ['name', 'email'];

    public function isPremium(): bool
    {
        return true;
    }
}
