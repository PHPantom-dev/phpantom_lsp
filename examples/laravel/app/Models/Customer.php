<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Hidden;
use Illuminate\Foundation\Auth\User as Authenticatable;

// Try: complete the column in Hidden; name and email come from this model's fillable list.
/**
 * The authenticated user model, wired up in config/auth.php as the
 * default guard's provider model.
 */
#[Hidden(['email'])]
class Customer extends Authenticatable
{
    /** @var list<string> */
    protected $fillable = ['name', 'email'];

    public function isPremium(): bool
    {
        return true;
    }
}
