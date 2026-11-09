# Laravel Demo Project for PHPantom LSP

A standalone Laravel playground that demonstrates PHPantom's Laravel-specific
features against a real Laravel installation.

## What it demos

- **Collection property types.** Literal property paths determine collection keys and plucked values, including grouped collections and higher-order proxies. See `Demo::collectionPropertyTypes()`.
- **Eloquent models.** Virtual properties from model column attributes, `$fillable`, `$casts`, `$attributes`, relationships, scopes, accessors, custom collections, and query builder forwarding. A method a model or its builder does not answer is reported, since Eloquent throws for it (see the "Try:" in `Demo::eloquentQuery()`).
- **Computed attributes.** Column suggestions distinguish stored values from accessors, while assignment suggestions include attributes with setters. See `Demo::computedAttributes()`.
- **Model factories.** Convention-based factories (no `@extends Factory<Model>` needed) get `create()`/`make()` returning the model, plus the dynamic `has{Relationship}()` / `for{Relationship}()` methods synthesized from the model's relationships, and `trashed()` when the model uses `SoftDeletes` — all chaining fluently (e.g. `BlogAuthor::factory()->hasPosts(3)->create()`). See `Demo::factories()`.
- **Config and env navigation.** Go-to-definition and find-references for `config('app.name')` keys (resolves to `config/app.php`) and `env('APP_KEY')` vars (resolves to `.env`).
- **Typed connection names.** Completion, hover, navigation, references, and diagnostics follow the database, queue, or broadcasting receiver and model or job properties. See `Demo::typedConnections()`.
- **View navigation.** Go-to-definition for `view('welcome')` and `View::make('admin.users.index')` (resolves to Blade templates in `resources/views/`).
- **Asset paths.** `asset('css/app.css')` and `Vite::asset()`/`@vite` paths complete from the files on disk, jump to the file, and are reported when nothing is there. Inertia page names (`Inertia::render('Users/Index')`) work the same way in a project that installs Inertia.
- **Route navigation.** Go-to-definition for `route('home')` (resolves to `->name('home')` in route files).
- **Controller action navigation.** Go-to-definition, hover, rename, references, and completion for route action strings in `[Controller::class, 'method']` callables and `Route::controller(...)->group(...)` routes.
- **Related classes.** Code lenses link a model to its factory, policy, observers, migrations, API resources, and custom collection, link a factory, policy, or observer back to its model, and link an event to its listeners and back. See `app/Models/BlogPost.php` and `app/Events/BlogPostPublished.php`.
- **Routes on controller methods.** A code lens above each controller action lists the routes that reach it (verb, URI, and name) and jumps to where each is registered.
- **Middleware names.** Completion, hover, go-to-definition, find-references, and unknown-name diagnostics for the aliases and groups `->middleware(...)`, `->withoutMiddleware(...)`, and the `#[Middleware]` controller attribute name, from the framework's defaults and the ones `bootstrap/app.php` registers. See `Demo::middlewareNames()`.
- **Translation navigation.** Go-to-definition for `__('messages.welcome')`, `trans('auth.failed')`, and `trans_choice(...)` (resolves to `lang/` PHP files).
- **Artisan command names & signatures.** Completion, go-to-definition, hover, and unknown-name diagnostics for command names in `Artisan::call(...)`, `Artisan::queue(...)`, `Schedule::command(...)`, and `$this->call(...)` (resolves to the command class declared by `$signature` / `$name` / `#[Signature]` / `#[AsCommand]`). Inside a command, `$this->argument(...)` / `$this->option(...)` complete, hover, and validate against that command's own signature, and the parameter array of `Artisan::call('cmd', [...])` completes its argument and `--option` keys. See `Demo::artisanCommands()` and `app/Console/Commands/`.
- **Validation rules.** Completion, hover, navigation, and diagnostics for validation strings and their references. See `Demo::validationRuleStrings()` and `app/Http/Requests/ValidationDemoRequest.php`.
- **Blade template intelligence.** Variable completion and hover in `{{ }}` expressions (shown as `e()` calls), go-to-definition on `@include`/`@extends` view references, `@forelse`/`@empty` directives, implicit `$loop` variable in `@foreach`/`@forelse`, implicit `$message` in `@error`, implicit `$value` in `@session`, `@verbatim` block handling, and standalone `@var` docblocks for type narrowing. Code actions work in templates too: importing a class adds a `@use` directive.

## Getting started

1. Run `composer install` in this directory to install Laravel.
2. Open this directory as a project (or workspace folder) in your editor.
3. Open `app/Demo.php` and navigate to any `demo()` method.
4. Trigger completion, hover, or go-to-definition inside the method body.
