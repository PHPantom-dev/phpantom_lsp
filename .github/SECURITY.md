# Security Policy

## Reporting a vulnerability

Please do not report security problems in public issues, discussions, or
pull requests.

Report them privately through GitHub's
[private vulnerability reporting](https://github.com/PHPantom-dev/phpantom_lsp/security/advisories/new)
form instead. Only the maintainers can see the report, and we can work
on a fix together in a private fork before anything is published.

A useful report includes:

- the PHPantom version or commit, and how it was installed,
- the editor or client you used,
- the steps or a minimal project that reproduces the problem,
- what an attacker gains (code execution, file read, crash, ...).

## What to expect

PHPantom is maintained by a small team, so responses are best effort.
We aim to acknowledge a report within a few days, and we will keep you
updated while a fix is prepared. Once a fixed release is out, we publish
a security advisory. Reporters who disclosed privately are credited in
the advisory and the changelog, unless they ask not to be.

## Supported versions

Fixes go into the next release. Only the latest release receives
security fixes, so please confirm the problem still occurs on it (or on
`main`) before reporting.

## Scope

PHPantom reads and indexes the code of the workspace it is opened on,
and can run tools the project uses (PHPStan, PHP_CodeSniffer, PHPMD,
Mago, formatters). Those tools execute project code by design, so
opening a project you have chosen to trust and having its tooling run is
expected behaviour. Things we do want to hear about include:

- code from a workspace running without that workspace being trusted,
- reading or writing files outside the workspace that a request did not
  ask for,
- crashes or hangs triggered by a crafted file, when they take down the
  whole server rather than a single request.
