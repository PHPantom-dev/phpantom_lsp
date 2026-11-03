<?php

namespace App\Http\Requests;

use App\Rules\UppercaseDemo;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Validation\Rule;

class ValidationDemoRequest extends FormRequest
{
    public function authorize(): bool
    {
        return true;
    }

    /** @return array<string, mixed> */
    public function rules(): array
    {
        return [
            // Try: complete or hover a rule; misspell required to see a diagnostic.
            'email' => 'required|email|unique:customers,email',
            'password' => 'required|string|min:8',
            // Try: complete password after same:, or jump to its field key above.
            'password_confirmation' => 'required|same:password',
            // Try: complete the table and column after exists:. The column's
            // Bakery cast supplies JamFlavor values when completing after in:.
            'jam_flavor' => 'required|exists:bakeries,jam_flavor|in:strawberry,raspberry,blueberry',
            'featured_flavor' => [
                'required',
                'exists:bakeries,jam_flavor',
                // Try: complete a value to see the same JamFlavor cases.
                Rule::in(['strawberry', 'raspberry']),
            ],
            // Try: complete a new array item to see the custom rule class.
            'code' => ['required', new UppercaseDemo()],
            // Try: jump to bakery_code in DemoServiceProvider.
            'booking_code' => 'required|bakery_code',
        ];
    }

    /** @return array<string, string> */
    public function messages(): array
    {
        // Try: complete the field and rule in a key, or misspell email.
        return [
            'email.required' => 'Please provide your email address.',
            'email.unique' => 'That email address is already registered.',
        ];
    }

    /** @return array<string, string> */
    public function attributes(): array
    {
        // Try: complete a key from rules(), or jump to its declaration.
        return [
            'email' => 'contact email',
            'password_confirmation' => 'repeated password',
        ];
    }
}
