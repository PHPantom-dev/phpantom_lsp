<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Casts\Attribute;
use Illuminate\Database\Eloquent\Model;

class CustomerProfile extends Model
{
    protected $table = 'customers';
    protected $guarded = [];
    protected $casts = ['name' => 'string'];
    protected $appends = ['display_name', 'initials'];

    public function getDisplayNameAttribute(): string
    {
        return (string) $this->name;
    }

    /** @return Attribute<string, never> */
    protected function initials(): Attribute
    {
        return Attribute::get(fn (): string => substr((string) $this->name, 0, 1));
    }

    /** @return Attribute<string, string> */
    protected function fullName(): Attribute
    {
        return Attribute::make(
            get: fn (): string => (string) $this->name,
            set: fn (string $value): array => ['name' => $value],
        );
    }

    /** @return Attribute<string, never> */
    protected function name(): Attribute
    {
        return Attribute::get(fn (string $value): string => strtoupper($value));
    }
}
