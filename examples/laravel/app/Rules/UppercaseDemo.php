<?php

namespace App\Rules;

use Closure;
use Illuminate\Contracts\Validation\ValidationRule;

/** Require uppercase text for a bakery label. */
class UppercaseDemo implements ValidationRule
{
    public function validate(string $attribute, mixed $value, Closure $fail): void
    {
        if (!is_string($value) || strtoupper($value) !== $value) {
            $fail('The :attribute must be uppercase.');
        }
    }
}
