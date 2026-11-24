<?php

/**
 * PHP Showcase — Signature Help
 *
 * The parameter hints shown while typing a call's arguments, for functions,
 * methods, and attribute constructors.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Attribute;
use Demo\Scaffolding;

// ── Signature Help ──────────────────────────────────────────────────────────

class SignatureHelpDemo
{
    public function demo(): void
    {
        // Place cursor inside parentheses to see parameter hints.
        // The active parameter updates as you type commas.
        $user = new Scaffolding\User('Alice', 'alice@example.com');
        Scaffolding\createUser('Alice', 'alice@example.com');  // standalone function
        $user->setStatus(Scaffolding\Status::Active);          // instance method
        Scaffolding\User::findByEmail('alice@example.com');    // static method
        new Scaffolding\User('Bob', 'bob@example.com');        // constructor

        // Chains resolve through return types and properties:
        $user->getProfile()->setBio('Hello');                       // method return chain
        (new Scaffolding\User('X', 'x@x.com'))->setStatus(Scaffolding\Status::Active);     // (new ...)->method
        new Scaffolding\User('X', 'x@x.com')->setStatus(Scaffolding\Status::Active);     // PHP 8.4 style

        // Default values appear in parameter labels (e.g. "int $page = 1"):
        $svc = new Scaffolding\ScaffoldingSignatureHelp();
        $svc->paginate(2, 50);

        // Per-parameter @param descriptions appear next to each parameter.
        // When the effective docblock type differs from the native PHP type
        // the description is prefixed with the effective type:
        $svc->search('php', 1, 25);
    }

    public function invokable(Scaffolding\ScaffoldingGreetAction $greet): void
    {
        // Invoking an object shows the parameters of its __invoke() method:
        $greet('Alice', true);                                      // injected instance
        (new Scaffolding\ScaffoldingGreetAction())('Bob');          // (new ...)()
    }
}


// ── Attribute Signature Help ────────────────────────────────────────────────

#[Attribute]
class DemoRoute
{
    public function __construct(
        public string $path,
        public string $method = 'GET',
    ) {}
}

class AttributeSigHelpDemo
{
    // Try: place cursor inside the attribute parens and trigger signature help.
    // Named parameter completion also works: type "method:" after the first arg.
    #[DemoRoute('/users', method: 'POST')]
    public function store(): void {}
}
