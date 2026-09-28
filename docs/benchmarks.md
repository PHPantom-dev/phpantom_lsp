# Benchmarks

PHPantom is benchmarked on every commit to track performance
regressions. All numbers below were measured on a production Laravel
codebase: 5.1K PHP files (389k lines) and 1.3K Blade templates (88k
lines), with 27k vendor files (1.7M lines).

## Headline Numbers

| Metric | PHPantom | Intelephense | PHP Tools | Phpactor | PHPStorm |
| --- | --- | --- | --- | --- | --- |
| Cold start, CPU time | 17 s | 16 s | 36 s | 3 min 18 s | 17 min 55 s |
| Cold start, wall clock | 2 s | 11 s | 10 s | 3 min 17 s | 1 m 7 s |
| Warm start, CPU time | 17 s | 4 s | 36 s | 3 s | 3 min 23 s |
| Warm start, wall clock | 2 s | 1 s | 10 s | 4 s | 5 s |
| RAM, cold start | 578 MB | 766 MB | 594 MB | 467 MB | 1 GB |
| RAM, warm start | 578 MB | 759 MB | 594 MB | 60 MB | 2 GB |
| Disk cache | 0 | 51 MB | 0 | 2.3 GB | 379 MB |

A cold start is the first index with no disk cache; a warm start reuses
the cache written by a previous session. CPU time is the total consumed
until full type intelligence is available; wall-clock times are
approximate. RAM is the steady resident memory once indexing has
finished, not the peak reached during the index itself. Intelephense and Phpactor do not analyse Blade templates
or Laravel's runtime magic, which makes this codebase cheaper for them
than the numbers alone suggest.

The codebase was stripped to its PHP and Blade files before each run,
with the `.git` directory and all JS/CSS assets removed, so every tool
indexes the same PHP-only surface. PHPStorm's baseline IDE startup time
was measured separately and subtracted from its cold and warm start
figures above, to keep the comparison focused on PHP indexing rather
than general editor overhead.

## Live Charts

Latency and memory usage are tracked on every commit and plotted over
time. These charts are useful for catching regressions and observing
trends across releases.

- [Latency Benchmarks](https://phpantom-dev.github.io/phpantom_lsp/dev/bench/) -- completion response time per commit
- [Memory Benchmarks](https://phpantom-dev.github.io/phpantom_lsp/dev/memory/) -- resident memory after full indexing

## What We Measure

**Latency benchmarks** run `cargo bench` on the completion engine,
measuring end-to-end response time for completion requests against
real-world fixtures. Results are tracked with
[github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark).

**Memory benchmarks** measure peak resident memory (RSS) of
`phpantom_lsp` after fully indexing the benchmark project. THP (Transparent
Huge Pages) is disabled during measurement for consistent results.
