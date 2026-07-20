<?php

namespace App\Models;

use Illuminate\Foundation\Auth\User as Authenticatable;

/**
 * The admin-guard user model, wired up in config/auth.php as the
 * `admin` guard's provider model.
 */
class Administrator extends Authenticatable
{
    /** @var list<string> */
    protected $fillable = ['name', 'email'];

    public function isSuperAdmin(): bool
    {
        return true;
    }
}
