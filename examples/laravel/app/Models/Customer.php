<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\{Appends, Fillable, Guarded, Hidden, Visible};
use Illuminate\Foundation\Auth\User as Authenticatable;

// Try: complete the column in Hidden; Fillable and $fillable supply name and email.
/**
 * The authenticated user model, wired up in config/auth.php as the
 * default guard's provider model.
 */
#[Hidden(['email'])]
#[Fillable(['name'])]
#[Guarded(['id'])]
#[Visible(['name', 'email', 'display_name'])]
#[Appends('display_name')]
class Customer extends Authenticatable
{
    /** @var list<string> */
    protected $fillable = ['email'];

    public function getDisplayNameAttribute(): string
    {
        return (string) $this->name;
    }

    public function isPremium(): bool
    {
        return true;
    }
}
