# Laravel Demo Project for PHPantom LSP

A standalone Laravel playground that demonstrates PHPantom's Laravel-specific
features against a real Laravel installation.

## What it demos

- **Eloquent models.** Virtual properties from `$fillable`, `$casts`, `$attributes`, relationships, scopes, accessors, custom collections, and query builder forwarding.
- **Config and env navigation.** Go-to-definition and find-references for `config('app.name')` keys (resolves to `config/app.php`) and `env('APP_KEY')` vars (resolves to `.env`).
- **View navigation.** Go-to-definition for `view('welcome')` and `View::make('admin.users.index')` (resolves to Blade templates in `resources/views/`).
- **Route navigation.** Go-to-definition for `route('home')` (resolves to `->name('home')` in route files).
- **Controller action navigation.** Go-to-definition, hover, rename, references, and completion for route action strings in `[Controller::class, 'method']` callables and `Route::controller(...)->group(...)` routes.
- **Translation navigation.** Go-to-definition for `__('messages.welcome')`, `trans('auth.failed')`, and `trans_choice(...)` (resolves to `lang/` PHP files).
- **Blade template intelligence.** Variable completion and hover in `{{ }}` expressions (shown as `e()` calls), go-to-definition on `@include`/`@extends` view references, `@forelse`/`@empty` directives, implicit `$loop` variable in `@foreach`/`@forelse`, implicit `$message` in `@error`, implicit `$value` in `@session`, `@verbatim` block handling, and standalone `@var` docblocks for type narrowing.

## Getting started

1. Run `composer install` in this directory to install Laravel.
2. Open this directory as a project (or workspace folder) in your editor.
3. Open `app/Demo.php` and navigate to any `demo()` method.
4. Trigger completion, hover, or go-to-definition inside the method body.
