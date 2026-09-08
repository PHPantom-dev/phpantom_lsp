window.BENCHMARK_DATA = {
  "lastUpdate": 1788905530368,
  "repoUrl": "https://github.com/PHPantom-dev/phpantom_lsp",
  "entries": {
    "PHPantom Memory Usage": [
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "71048b25e1224bca7e671d5468a61b03e76aaddb",
          "message": "Coalesce whole-file requests and offload diagnostics\n\nAdd shared `(kind, uri)` request coalescing for expensive whole-file\nLSP handlers so superseded requests reuse the last result instead of\nstarting new full scans.\n\nMove fast and slow diagnostic collection onto the blocking pool and keep\npull diagnostics cache entries between edits, so pull requests return\ncached results while background recompute runs.\n\nAlso expose the cancel-safe blocking helper for reuse, clean up\ncoalescing state on file close, and update performance/changelog docs.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T10:36:53+02:00",
          "tree_id": "45774e233f4369fbaca20a91079cdc0c82d0b716",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/71048b25e1224bca7e671d5468a61b03e76aaddb"
        },
        "date": 1780735780847,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 53.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f2024c05bfadc97d443ecb9e3b58f84ed90881c8",
          "message": "Use RwLock for resolved class cache reads\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T13:40:01+02:00",
          "tree_id": "a0552fb988c26ef6a234825af8c07d4a383870d3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f2024c05bfadc97d443ecb9e3b58f84ed90881c8"
        },
        "date": 1780746782013,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b209f5efd7d1a2ba1d5b3bb18026999a3495a65c",
          "message": "Build FQN index entries outside write locks\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T14:32:55+02:00",
          "tree_id": "10dca242c371023c88fd4aff3fc2b02b294bb53d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b209f5efd7d1a2ba1d5b3bb18026999a3495a65c"
        },
        "date": 1780749968746,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 52,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "88d95e0a54e303241fed6ba2e454b99a5090950a",
          "message": "Use memmem for block comment terminator scan\n\nReplace block comment skipping in classmap scanning with direct\n`memmem` searches for `*/` in both symbol and class scanners.\n\nRemove completed P4 performance task from todo docs\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T14:37:40+02:00",
          "tree_id": "955813248b53b3b711022c595dea77dde6f65394",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/88d95e0a54e303241fed6ba2e454b99a5090950a"
        },
        "date": 1780750232924,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 51.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2cb0d115ced330777895ce7d8108303f800c5aa8",
          "message": "Reuse parser arena across AST updates\n\nAdd a thread-local reusable bump arena for parse updates and reset it\nper call to avoid repeated mmap/munmap churn on each didChange.\n\nHandle re-entrant parses safely by falling back to a temporary arena\nwhen the shared arena is already borrowed.\n\nRetarget P19 in performance docs to track remaining arena reuse work\nin code action helpers instead of the parse hot path.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T14:48:54+02:00",
          "tree_id": "377a9cd24a6f357c1f9975c0d649ad563955dd15",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2cb0d115ced330777895ce7d8108303f800c5aa8"
        },
        "date": 1780750921033,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 51.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "061b5ba365186d4cc61685e1e6b7dcfa11148b9a",
          "message": "Parse code actions once per request\n\nUse the shared parser cache across code action collectors so the file\nis parsed once instead of once per refactoring candidate. Migrate\nhelper call sites to `with_parsed_program` and drop redundant arena\nallocations, then remove the completed P19 performance todo and record\nthe user-visible speedup in the changelog.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T16:20:31+02:00",
          "tree_id": "62426435032c121c8e70b50c3df772e68398d508",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/061b5ba365186d4cc61685e1e6b7dcfa11148b9a"
        },
        "date": 1780756377224,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6cb6c88dbe1de53a9912a084845a39736955bb15",
          "message": "Preload guarded Composer helper files during init\n\nEagerly full-parse autoload \"files\" entries during indexing so\nfunctions wrapped in `function_exists` guards are available on first\nlookup instead of triggering a serial lazy-parse fallback.\n\nAdd integration coverage for guarded helper preloading, preserving\nalready-parsed files, and updated initialization expectations for\n`session()` and `app()`.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T18:37:21+02:00",
          "tree_id": "818a579c8bfe033d06d474987ab09e52c2a6819b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6cb6c88dbe1de53a9912a084845a39736955bb15"
        },
        "date": 1780764632970,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 54.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "669b0e1f41bd90a6b0c0f1d5dc601c80710b9622",
          "message": "Remove dead linear class lookup fallback\n\nDrop the unreachable O(n) fallback scan in\n`find_class_in_uri_classes_index` and return `None` after the hash\nindex miss. Update performance todo docs to remove P8 and note the\nchange in the changelog.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-06T20:43:52+02:00",
          "tree_id": "9ed78dccd0cf1b0871f0820b802b8bf5dd7a062f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/669b0e1f41bd90a6b0c0f1d5dc601c80710b9622"
        },
        "date": 1780772237151,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 57.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e5e6e32bd7de5c028f643bac954d830ceaf56079",
          "message": "Extract function handles variable read-before-write correctly\n\nExtracting a selection that reads a variable before its first assignment\nnow passes\nthe variable as an argument, not just returning its final value. When\nthe return\nexpression references a local variable or has side effects, it is kept\ninside the\nextracted method and propagated via a null sentinel at the call site.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-10T12:42:15+02:00",
          "tree_id": "f3631c7de5c0dfd5e396749e7301856d12e0ab3d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e5e6e32bd7de5c028f643bac954d830ceaf56079"
        },
        "date": 1781088932141,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "15725d58502f483d94a9eca267e6f0fecd30cb93",
          "message": "**Type resolution.** `@method` tags override inherited methods of the\nsame name, and null-coalesce assignment (`??=`) preserves the resolved\nunion type to avoid false unresolvable diagnostics.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-12T17:10:38+02:00",
          "tree_id": "7d2433dcb8541baee6b91376f346067aed1bd2e5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/15725d58502f483d94a9eca267e6f0fecd30cb93"
        },
        "date": 1781277880867,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "404b7dfcdf65a21777b3b4f15c39a239f58bfcb8",
          "message": "Add `decl_start_offset` to support resolving `self` references inside\nclass-level attributes\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-12T18:01:23+02:00",
          "tree_id": "031de84b8d2157cf4733c42728951ee912c7c75c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/404b7dfcdf65a21777b3b4f15c39a239f58bfcb8"
        },
        "date": 1781280825512,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "djsmits12@gmail.com",
            "name": "Remco Smits",
            "username": "RemcoSmitsDev"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "aaa90928fd9f18d47530092ca3d2ef747f7cfead",
          "message": "Add constructor reference support for attributes (#155)",
          "timestamp": "2026-06-20T15:12:17+02:00",
          "tree_id": "2a1978cbae6098f04966e8d370a73354aadd26f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/aaa90928fd9f18d47530092ca3d2ef747f7cfead"
        },
        "date": 1781961861872,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e8b829e74311ba5efc3d834abc88565a88055f82",
          "message": "fix(docblock): support phpstan require tag navigation\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-20T15:14:16+02:00",
          "tree_id": "a482368bb1fd7b8c40d3d103f7edb7e7606c739b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e8b829e74311ba5efc3d834abc88565a88055f82"
        },
        "date": 1781961966515,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 56,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "martin@srsen.sk",
            "name": "Martin Sršeň",
            "username": "MrSrsen"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "655f99baff0747f89d411d6d9345496c61dd8b64",
          "message": "Substitute generic return types when the native hint is nullable\n\n`should_override_type_typed` used `unwrap_nullable()`, which only strips\nthe\n`?Foo` (Nullable) form and not the `Foo|null` (Union-with-null) form. A\nnullable-union native such as `object|null` therefore reached the union\nbranch with its `null` member attached; since `object` and `null` are\nboth\nscalar names, the branch judged the type unrefinable and discarded a\ngeneric\ndocblock return like `@psalm-return ?T`, leaving the bare native.\n\nUse `non_null_type()` (strips null from both Nullable and Union forms)\nso the\nnon-null part is analysed, matching the function's documented `Foo|null\n→ Foo`\nintent. Fixes inherited generic returns such as Doctrine's\n`ServiceEntityRepository<T>::find(): ?T` resolving to `object|null`.\n\nAdds an assert_type regression fixture.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-20T15:24:05+02:00",
          "tree_id": "00447edb78a67ac4ce2666e12eae37d434190d58",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/655f99baff0747f89d411d6d9345496c61dd8b64"
        },
        "date": 1781962621220,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 58.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "martin@srsen.sk",
            "name": "Martin Sršeň",
            "username": "MrSrsen"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f7f7b0bbdedb8d8ba9c8864c008db17038efcb35",
          "message": "Index class-likes declared inside conditional blocks",
          "timestamp": "2026-06-20T15:45:31+02:00",
          "tree_id": "45802dd593304d008ea5dfa8fefc91715d9f10eb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f7f7b0bbdedb8d8ba9c8864c008db17038efcb35"
        },
        "date": 1781963883300,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2c591344f33ed994015a9396274a18dfd1033e28",
          "message": "fix(unused-variables): treat get_defined_vars() as using all variables in scope (#158)",
          "timestamp": "2026-06-20T16:00:45+02:00",
          "tree_id": "a711a819aefbc68cf8be07c3d77cbe05311cd5f9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2c591344f33ed994015a9396274a18dfd1033e28"
        },
        "date": 1781964813942,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 56.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "60ce94cd7d7b991297ac660fc9f46706a79a0f15",
          "message": "feat(rename): link compact strings to local variables\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-20T16:12:31+02:00",
          "tree_id": "fd90a239cebb43ca90911f6c50f042ad7224e0f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/60ce94cd7d7b991297ac660fc9f46706a79a0f15"
        },
        "date": 1781965501393,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "05b3ab9a0ab40593c4b3c3846396463ee5345f7f",
          "message": "fix(rename): scope member renames to resolved declarations",
          "timestamp": "2026-06-20T16:27:04+02:00",
          "tree_id": "7fc2039564b6fbfb134f8d44c5144b5db5bd0dca",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/05b3ab9a0ab40593c4b3c3846396463ee5345f7f"
        },
        "date": 1781966420838,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dereuromark@users.noreply.github.com",
            "name": "Mark Scherer",
            "username": "dereuromark"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8a288cd9765215c9b72623539f0caf3d29844e28",
          "message": "Index framework global helpers loaded outside Composer autoload\n\nSome frameworks ship their global function aliases in a `*_global.php`\nfile that sits beside an autoloaded `functions.php` but is pulled in by\nthe application bootstrap rather than Composer's `files` autoload, so it\nnever appears in `autoload_files.php`. CakePHP is the canonical case:\n`src/Core/functions_global.php` defines `__`, `h`, `env`, `pr`, ... and\nis loaded via `require CAKE . 'functions.php'` in\n`config/bootstrap.php`.\n\nThe autoload-file scan only followed `autoload_files.php` and their\nrequire_once chains, so these globals were invisible and every `__()`\ncall reported \"unknown function\". On a real CakePHP 5 app this was about\n1000 of 1330 analyze findings (968 of them just `__`).\n\nSeed the scan with any `*_global.php` sibling of an autoload entry. The\nlookup is anchored to existing autoload entries (one read_dir per unique\ndirectory) instead of a blind walk of the vendor tree, and the existing\nfull-parse pass picks up the function_exists-guarded definitions.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-29T17:53:45+02:00",
          "tree_id": "7f62ff13e0a6b288481f4d2196e382183e0e64f0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8a288cd9765215c9b72623539f0caf3d29844e28"
        },
        "date": 1782749213228,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cf244568be6e2e58902f0ba5f00fbe67095d5b22",
          "message": "Fix type hierarchy registration and subtype expansion\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-06-30T20:07:28+02:00",
          "tree_id": "4cd4e50b2c23078906cb24d001262851d4e5dcbd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cf244568be6e2e58902f0ba5f00fbe67095d5b22"
        },
        "date": 1782843627902,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 54.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "21a0105758821d18a1a2df78bad92707cb37a487",
          "message": "Array-callable navigation\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-01T02:32:37+02:00",
          "tree_id": "be1fd221d06cef2e98e481ff93716e9766dad6a7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/21a0105758821d18a1a2df78bad92707cb37a487"
        },
        "date": 1782866738206,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 55.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a3fb686d159af34d01ca95801254e0d248ac7ee2",
          "message": "Handle case sensetivity (or rather the lack there off)\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-02T07:33:11+02:00",
          "tree_id": "0606eee278b55eb19e923c1152792d4e52ac286b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a3fb686d159af34d01ca95801254e0d248ac7ee2"
        },
        "date": 1782971177820,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 60.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4633ede8620844e1ad55d995a206e81909d9250e",
          "message": "Add a test\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T00:49:37+02:00",
          "tree_id": "bfb67fc3e63417c917315d963e79ceb266d18adb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4633ede8620844e1ad55d995a206e81909d9250e"
        },
        "date": 1783119766291,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9c304a5b9d20bde39bff940130b9ed8caed02026",
          "message": "Fix style\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T03:05:37+02:00",
          "tree_id": "cb01acfc35d3aed773301801457ad2e41ce4043e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9c304a5b9d20bde39bff940130b9ed8caed02026"
        },
        "date": 1783127850897,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d0f9077c5360d81c92b5a030eae2cdd63cefb429",
          "message": "feat(completion): array callable method completion\n\nAdd method name completion inside array callable strings. When the\ncursor is inside the method-name string of [Class::class, '|'] or\n[$obj, '|'], the LSP now offers method completions from the resolved\nclass.\n\nDetection handles ::class constants, $this, typed variables,\nnamespaced classes, whitespace variations, and unclosed strings.\n\nCompletion reuses the standard member-completion builder so items have\nidentical formatting to regular -> / :: member completions: label\ndetails, return type, deprecation tags, and data for lazy documentation\nresolve. Insert text is post-processed to strip snippet parentheses\nsince we are inserting into a string literal.\n\nWorks with both instance and static methods.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T03:22:49+02:00",
          "tree_id": "9538d0b3974cf362f553010d292db6ebeceb5265",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d0f9077c5360d81c92b5a030eae2cdd63cefb429"
        },
        "date": 1783128945406,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d04de94fa8f589f662d793329685413b5819b544",
          "message": "fix(rename): handle variables in dynamic property selectors\n\nDynamic property selectors like `$message->{$attribute}` were only\npartially\nwalked. The scope collector skipped selector expressions when tracking\nlocal\nvariable reads, which caused false `unused_variable` diagnostics for the\nselector variable.\n\nThe symbol map extractor also ignored non-identifier property selectors,\nso\nfind-references and rename missed variable occurrences inside dynamic\nproperty\naccesses. Renaming `$attribute` updated plain variable uses but not the\nselector inside `$message->{$attribute}`.\n\nWalk selector expressions during scope collection and emit variable\nsymbol\nspans for dynamic property selectors in the symbol map. Add regression\ntests\ncovering unused-variable diagnostics, references, and rename behavior\nfor this\npattern.\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T03:30:09+02:00",
          "tree_id": "b133a776a7449de17b6968c8a135e8242db90e04",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d04de94fa8f589f662d793329685413b5819b544"
        },
        "date": 1783129389117,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 60,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "399b2b15afa9aa1f5c0fc3a671970b08da30ea38",
          "message": "feat(completion): include static methods on instance access\n\nCloses #162\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T03:36:47+02:00",
          "tree_id": "215b769798e49acb6c905c7be06e08af676dd66e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/399b2b15afa9aa1f5c0fc3a671970b08da30ea38"
        },
        "date": 1783129803957,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "49514663cb372223ed2e90912f3882602cae8e30",
          "message": "feat(code-action): convert arrow function to closure\n\nAdd a refactor.rewrite code action that converts arrow functions to\nanonymous closures: `fn($x) => $x * 2` becomes\n`function($x) { return $x * 2; }`.\n\nVariables from the outer scope used in the expression are automatically\ndetected and captured via a `use()` clause. `$this` is excluded since\nclosures bind it automatically (unless static). Nested arrow functions\nare handled by extending the parameter exclusion set.\n\nPreserves:\n- static keyword\n- Return type hints\n- Parameter type hints\n\nIncludes 10 unit tests covering: simple expressions, type hints,\nstatic arrows, single/multiple variable capture, $this exclusion,\nparameter exclusion, method calls with captures, and deduplication.\n\nCloses #148\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T04:44:50+02:00",
          "tree_id": "6275e493402db7f8e802c2b9281d2e3d6452c0d2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/49514663cb372223ed2e90912f3882602cae8e30"
        },
        "date": 1783133875592,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "31cc5e22506b38502c6285c99968bf55dcfdcfad",
          "message": "fix(diagnostics): resolve literal types for argument type checking\n\nFixes #180 — string/int/float literal expressions passed as arguments\nare now narrowed to their precise literal type (e.g. `'desc'` becomes\n`PhpType::Literal(\"'desc'\")`) instead of the widened base type\n(`string`). This allows literal values to correctly match PHPDoc\nliteral-union parameter types like `'asc'|'desc'`.\n\nAlso fixes `is_string_subtype()` and `is_int_subtype()` to handle\n`PhpType::Union` — a union of string/int literals (e.g.\n`'asc'|'desc'`) is now recognised as a string/int subtype, allowing\nPHPDoc `@param` annotations with literal unions to properly override\nnative scalar type hints.\n\nThe narrowing is applied only in the diagnostic argument-collection\npath (not in general variable resolution) to avoid changing inferred\ntypes for array shapes and list element tracking.\n\nIncludes 8 new integration tests covering correct and incorrect\nstring, integer, and float literals against literal-union parameters,\nplus precise numeric-string checking (`'42'` passes, `'hello'` is\nflagged).\n\nCloses #180\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T06:04:59+02:00",
          "tree_id": "404d4ae1f0957556ac34ea0df1d4bb95a479c855",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/31cc5e22506b38502c6285c99968bf55dcfdcfad"
        },
        "date": 1783138693883,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0c17fedbcc7fbc4a8254809c855542fcc78b0214",
          "message": "Update bug list\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-04T06:10:17+02:00",
          "tree_id": "fa29648592f540b76e31e686a72b378d1e8f404c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0c17fedbcc7fbc4a8254809c855542fcc78b0214"
        },
        "date": 1783139011539,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "23bbbfb7409bbab11bceebf5128f61f654fa1bbe",
          "message": "Move to immutable releases\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-06T23:21:33+02:00",
          "tree_id": "949e82613a22c121e64ff351dc9f6c00a1ffb43c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/23bbbfb7409bbab11bceebf5128f61f654fa1bbe"
        },
        "date": 1783373682347,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2705233e40b859b765e8bd9f35f323acd1207b3d",
          "message": "Add update command for self-updating the binary",
          "timestamp": "2026-07-07T00:24:42+02:00",
          "tree_id": "71817c1a6ef3f405409cc007182aa680f299f711",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2705233e40b859b765e8bd9f35f323acd1207b3d"
        },
        "date": 1783377521571,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f946d9ecbd68e0e2aad616e63def6634aad55582",
          "message": "Update todo",
          "timestamp": "2026-07-07T00:25:09+02:00",
          "tree_id": "17b2e3f0442f51933c26c80121c36d331373c07b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f946d9ecbd68e0e2aad616e63def6634aad55582"
        },
        "date": 1783377583402,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "eccf57fc18136e342e968842a463e10953b7eadd",
          "message": "feat(docblock): recognize @phpstan-sealed tag for class references (#190)",
          "timestamp": "2026-07-06T21:58:14-05:00",
          "tree_id": "6a9134bd8551effb695fc69a66d60b6b6b09497e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eccf57fc18136e342e968842a463e10953b7eadd"
        },
        "date": 1783393922036,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d83e2e1ecd0a560fab2c274280d381c4e9913bb4",
          "message": "fix(diagnostics): resolve callable return type for call-result invocations (#189)\n\nSigned-off-by: Anders Jenbo <anders@jenbo.dk>\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-06T22:02:01-05:00",
          "tree_id": "6c0b2ce1156bdf1c1d12c9482cffc01b70473441",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d83e2e1ecd0a560fab2c274280d381c4e9913bb4"
        },
        "date": 1783394187922,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "6ab58cb2c5e6cc358f479c4cec2ccddf80e41f61",
          "message": "feat(inference): infer array_map callback return type from body expression\n\nWhen the callback passed to `array_map` has no explicit return type\nhint, the LSP now infers the return type by resolving the body\nexpression against the input array's element type.\n\nFor example, `array_map(fn($item) => $item->id, $items)` where\n`$items` is `list<Item>` now correctly produces `list<string>`\n(from `Item::$id`'s type) instead of falling back to `list<Item>`.\n\nImplementation:\n- Extracts the first callback parameter name and maps it to the\n  input array's element type via a synthetic `scope_var_resolver`\n- Loads the `ClassInfo` for the element type so property access\n  resolution can find class members\n- For arrow functions: resolves `arrow.expression` directly\n- For closures: finds the first `return` statement's expression\n\nIncludes 1 new integration test for inferred return types.",
          "timestamp": "2026-07-06T22:20:15-05:00",
          "tree_id": "89ef23c3b3b2777cca75839afd1b48f3dd07a1ff",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6ab58cb2c5e6cc358f479c4cec2ccddf80e41f61"
        },
        "date": 1783395279124,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "14beee7d0f45ef99e25dde9ed363b3a3385f3495",
          "message": "fix(types): accept integer literals for int range arguments\n\nLiteral integer arguments now satisfy int range parameter types when\ntheir values fall within the declared bounds, including symbolic min\nand max limits.\n\nThis fixes false-positive type_mismatch_argument diagnostics for calls\nlike usleep(10_000) and Laravel-style repeatEvery(1).\n\nCloses #197",
          "timestamp": "2026-07-07T11:51:35-05:00",
          "tree_id": "a09bad6201517985e2106e6885531b9c7f519d12",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/14beee7d0f45ef99e25dde9ed363b3a3385f3495"
        },
        "date": 1783443949587,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "719ffd782bece53e42754736b8816796c9aacf64",
          "message": "fix(inference): avoid null default fallback for method templates\n\nMethod template inference no longer binds template parameters from\nomitted null defaults except in the narrow cases where defaults are\nmeaningful for template resolution.\n\nThis fixes Laravel Conditionable::when() false positives such as\nwhen(->integer(...), fn (, ) => ...), where the callback\nparameter type could collapse to null instead of the concrete argument\ntype.",
          "timestamp": "2026-07-07T15:33:08-05:00",
          "tree_id": "c094782fcbcdc78c9bb97bea2a016f4e3c8a67a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/719ffd782bece53e42754736b8816796c9aacf64"
        },
        "date": 1783457248342,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "740618265307c54eb07f8acb2cf2c7f03ad28cb2",
          "message": "Fix a couple of bugs",
          "timestamp": "2026-07-09T01:22:41+02:00",
          "tree_id": "53710f5c7b9e8f2677c15ee314d88d2d0bcd17b5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/740618265307c54eb07f8acb2cf2c7f03ad28cb2"
        },
        "date": 1783553818483,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b983f6fceb137cddcfa189641f288d74619a7116",
          "message": "feat(diagnostics): detect declare(strict_types=1) for stricter type\nchecking\n\nRead declare(strict_types=1) from the calling file when checking\ncall argument compatibility so PHP's strict scalar call semantics are\nreflected in type_mismatch_argument diagnostics.\n\nThis hardens the implementation by preserving literal kinds for int,\nfloat, and string values instead of flattening them to raw strings.\nThat keeps float literals from being misclassified as ints and lets\nnumeric literals retain PHP-specific forms such as underscores,\nscientific notation, and hex/binary/octal integers during checks.\n\nNon-strict mode continues to allow PHP's call-time scalar coercions\nsuch as int/float to string and numeric-string to int/float, while\nstrict mode flags those mismatches. Concatenation and other non-call\ncontexts are unchanged.\n\nCloses #203",
          "timestamp": "2026-07-09T01:37:12+02:00",
          "tree_id": "4b96c9a7ad9192d0047ffdf516e83ff073470843",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b983f6fceb137cddcfa189641f288d74619a7116"
        },
        "date": 1783554691687,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a944c6f09dbaad02653e3f61f04ffd9a38478a6a",
          "message": "ci: add publish.yml",
          "timestamp": "2026-07-09T01:52:28+02:00",
          "tree_id": "b059bb9b159cc39a649374ce97db1caae4567ff3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a944c6f09dbaad02653e3f61f04ffd9a38478a6a"
        },
        "date": 1783555594381,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4f87b45221867d71235d730ec28515ecda7591e9",
          "message": "Fix running full diagnostics on file open, and write lock",
          "timestamp": "2026-07-09T01:52:48+02:00",
          "tree_id": "0a8c906d97a247b93aed74550f970cab5190bacc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4f87b45221867d71235d730ec28515ecda7591e9"
        },
        "date": 1783555626076,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cf1580969c48dd855d4321e85774750729015496",
          "message": "Reloaded files no longer leave ghost",
          "timestamp": "2026-07-09T03:43:41+02:00",
          "tree_id": "dbaa1153411d814fe9ac4b5bc8d80a90d0d9dc93",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cf1580969c48dd855d4321e85774750729015496"
        },
        "date": 1783562269409,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e5221b28292e40e6eaa7c1002723fdb7ec73743e",
          "message": "A rare internal parser error no longer permanently breaks a file",
          "timestamp": "2026-07-09T03:55:04+02:00",
          "tree_id": "1042f56b053cd8e229defdd32d06799d2fbba8f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e5221b28292e40e6eaa7c1002723fdb7ec73743e"
        },
        "date": 1783562977192,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "faba6f00533f4988e8a0e7966976020d8e7a0abc",
          "message": "Add diagnostics test",
          "timestamp": "2026-07-09T06:18:02+02:00",
          "tree_id": "0c81b43dcc6c81cd5c618b85ea61d009de29979d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/faba6f00533f4988e8a0e7966976020d8e7a0abc"
        },
        "date": 1783571857956,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "3a102b316856faa4a38f20f2f16c9a19d5e2ecf6",
          "message": "fix: preserve string type on bracket-indexed assignment\n\nString indexed assignment ($str[0] = 'z') incorrectly widened the\nvariable's type from string to array<int, string>. In PHP, bracket\nindexing on a string modifies it in-place — the variable remains a\nstring.\n\nAdd an early return in process_array_key_assignment when the base type\nis a string subtype, mirroring the existing guard for object types.\n\nFixes #207",
          "timestamp": "2026-07-09T08:41:02-05:00",
          "tree_id": "247945539cde234c86db5f55a0eda18551cd56f9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3a102b316856faa4a38f20f2f16c9a19d5e2ecf6"
        },
        "date": 1783605325985,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "39e00ce5c9c8d4e6a6715d7345f8e837b942d568",
          "message": "chore: fix clippy",
          "timestamp": "2026-07-09T08:54:22-05:00",
          "tree_id": "b75b3cd8f63276450a5323e67442e492bdf23462",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/39e00ce5c9c8d4e6a6715d7345f8e837b942d568"
        },
        "date": 1783606007183,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "745059c070bf888fad9952915e5c6559b5c34672",
          "message": "fix: stop classifying mixed as scalar",
          "timestamp": "2026-07-09T10:16:04-05:00",
          "tree_id": "438ad06d3e3bc8740cb3c510a84f38637ce49b79",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/745059c070bf888fad9952915e5c6559b5c34672"
        },
        "date": 1783610933964,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "675496a9618e29b80c845cb0ab73fdf14a7dcb1a",
          "message": "fix: support self member references in @see tags\n\nDocblock @see extraction already handled ClassName::member references,\nbut dropped self::member, static::member, and parent::member during\nsymbol extraction because those keywords were filtered out as non-\nnavigable class names.\n\nTeach @see extraction to treat self/static/parent like the rest of the\nsymbol map does, emitting SelfStaticParent for the left-hand side and a\nnormal docblock MemberAccess for the referenced member. This restores\ngo-to-definition for class docblocks that refer to their own members.\n\nCloses #211",
          "timestamp": "2026-07-09T12:25:41-05:00",
          "tree_id": "31b1e7fd9b7dee275c329305c6d7edfc94c2b3ec",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/675496a9618e29b80c845cb0ab73fdf14a7dcb1a"
        },
        "date": 1783618693939,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "91f2f35e402ad2dd1f6bf2320918a117a626b971",
          "message": "fix: resolve grouped imports correctly\n\nGrouped use imports had two separate issues.\n\nFirst, unknown-class diagnostics skipped only single-line import\ndeclarations. When a grouped use statement was split across multiple\nlines, the imported names on continuation lines were treated as ordinary\nclass references and incorrectly flagged as missing.\n\nSecond, go-to-definition on a class name inside a grouped use\ndeclaration could fail because symbol extraction recorded the grouped\nitem without its namespace prefix, so the declaration-site reference did\nnot carry the correct fully-qualified name.\n\nTrack pending multiline use declarations until their terminating\nsemicolon, and record grouped use items with their full namespace\nprefix. Add regression coverage for parser/resolved-name handling,\nunknown-class diagnostics, and go-to-definition on both grouped import\nusage sites and grouped use declarations.\n\nRelates to #128",
          "timestamp": "2026-07-09T13:20:17-05:00",
          "tree_id": "97ef8aed85e797c6aa32fa3dfe9b902ac3c6fb9b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/91f2f35e402ad2dd1f6bf2320918a117a626b971"
        },
        "date": 1783621961230,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "efb56fc6b7866f15d56d9197288798368c30ded8",
          "message": "fix: add missing schema for phpcs and mago\n\nRelated #135",
          "timestamp": "2026-07-09T13:35:17-05:00",
          "tree_id": "732a44a0a0ba2d3f889f396f1fc76470fb89e053",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/efb56fc6b7866f15d56d9197288798368c30ded8"
        },
        "date": 1783622854815,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "a8fa3a501894a8a8bfe3694b4a377225ef48fdfc",
          "message": "fix: infer array type for += when operands are arrays\n\nThe compound assignment operator += was unconditionally routed to\narithmetic type inference, producing int|float even when both operands\nwere arrays. PHP overloads + / += for array union, and the binary +\noperator already handled this correctly.\n\nSplit AssignmentOperator::Addition out of the arithmetic match arm in\nboth process_compound_assignment and the compound-assignment-as-RHS\npath, checking is_array_like() on either operand before falling through\nto numeric inference.\n\nCloses #150",
          "timestamp": "2026-07-09T14:00:00-05:00",
          "tree_id": "b90a478c86d9cc1d67e8c9406306ce99f4ef238f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a8fa3a501894a8a8bfe3694b4a377225ef48fdfc"
        },
        "date": 1783624328127,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "c3f5770ed6677d6a45a420d43dddfa4d566001c6",
          "message": "fix: preserve [] array suffix after generic, shape, and paren types in docblocks\n\nsplit_type_token returned early after consuming the closing `>`, `}`,\nor `)\\ without checking for trailing `[]` suffixes.  This caused types\nlike `ReflectionAttribute<T>[]` to be parsed as bare\n`ReflectionAttribute<T>`, losing the array wrapper.\n\nAdd consume_array_suffix() and call it in all three closing-delimiter\npaths (angle, brace, paren) before consume_union_intersection_suffix().\nThis also fixes stacked suffixes like `Foo<T>[][]` and combined forms\nlike `array{id: int}[]|null`.\n\nCloses #157",
          "timestamp": "2026-07-09T14:19:15-05:00",
          "tree_id": "18ef5b22f6b790177f76228aa0d0ad914a013138",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c3f5770ed6677d6a45a420d43dddfa4d566001c6"
        },
        "date": 1783625457250,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "a2ff1b5efd1c4fc4d6b6d72bee5f7a66c6c4d8df",
          "message": "fix: infer foreach key type from class implements_generics\n\nWhen iterating over a class like Finder that implements\nIteratorAggregate<non-empty-string, SplFileInfo>, the foreach key\nvariable fell back to int|string because bind_foreach_key only\nchecked the type's own generic parameters via extract_key_type().\n\nAdd extract_iterable_key_type_from_class() in foreach_resolution.rs\n(mirrors the existing element/value extractor) and\nresolve_iterable_key_via_class() in forward_walk.rs. bind_foreach_key\nnow uses this as a Strategy 2 fallback, matching the pattern already\nused by bind_foreach_value.\n\nRelates to #144",
          "timestamp": "2026-07-09T15:12:35-05:00",
          "tree_id": "dec7218eb0f0031573cb5b6677ce30bc83e11566",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a2ff1b5efd1c4fc4d6b6d72bee5f7a66c6c4d8df"
        },
        "date": 1783628691010,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 60.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "24eeceee37b96782b52aee476586b34ed289f57d",
          "message": "Fix FQN for @see",
          "timestamp": "2026-07-10T11:01:20+02:00",
          "tree_id": "c53823b0a6a8e33303c771d01e2b9238fc15b6d8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/24eeceee37b96782b52aee476586b34ed289f57d"
        },
        "date": 1783674797343,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c90500405f0787031524a0578026aafa61b45050",
          "message": "feat: infer Generator type from closure yield expressions for template binding (#217)\n\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-10T11:33:09+02:00",
          "tree_id": "9e863eecc5a9cd11da888a38006bd6040e78fa23",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c90500405f0787031524a0578026aafa61b45050"
        },
        "date": 1783676720318,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 61.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "b67ad0dabce543c5d4d551732eec804e4121cf4c",
          "message": "feat: rank completion candidates by dependency provenance (#218)\n\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-10T11:39:48+02:00",
          "tree_id": "352b12aaeb6b4eaaa15b9f34bb4349b22808095a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b67ad0dabce543c5d4d551732eec804e4121cf4c"
        },
        "date": 1783677122673,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7849069c2de1c55b10b10912c2536ddc48dfa3fa",
          "message": "Add test for non PRS-4 vendor loading",
          "timestamp": "2026-07-10T11:54:32+02:00",
          "tree_id": "de1e7fe209dac7867d75644bb6543cb20c7a6305",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7849069c2de1c55b10b10912c2536ddc48dfa3fa"
        },
        "date": 1783677980176,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1cf5c86528755553cbd6783163563948c0eefe14",
          "message": "\"Go to Declaration or Usages\" from a declaration now lists usages",
          "timestamp": "2026-07-10T12:11:23+02:00",
          "tree_id": "dd16878d150de0a03e5d385bd151f2aa6cc554d1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1cf5c86528755553cbd6783163563948c0eefe14"
        },
        "date": 1783678996689,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e600605445cb13e7e65967c5862845b5cab0fe3e",
          "message": "External formatters no longer corrupt the connection",
          "timestamp": "2026-07-10T12:20:02+02:00",
          "tree_id": "10e519ebb236b4e5ac48fe57d28de67a0a91e153",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e600605445cb13e7e65967c5862845b5cab0fe3e"
        },
        "date": 1783679546163,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ea54c7467d6655a375a4a8ec8b9140ed211855ef",
          "message": "Resource-to-object migrated handles no longer trigger false argument\ntype errors",
          "timestamp": "2026-07-10T14:27:38+02:00",
          "tree_id": "3c73cd68f93e09e4ae9dd243f2a7d7667f624eb5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ea54c7467d6655a375a4a8ec8b9140ed211855ef"
        },
        "date": 1783687204443,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "e501c6da5a6113f74c25b6e66f0d32ff285bb361",
          "message": "fix: int ranges compatible with refined-int pseudo-types (#170)\n\nint<0,max> passed to a non-negative-int parameter no longer triggers\na false type_mismatch_argument diagnostic.\n\nAdded comprehensive subtype relationships between IntRange types and\nrefined-int pseudo-types:\n- IntRange <: refined-int (int<0,max> <: non-negative-int)\n- refined-int <: IntRange (positive-int <: int<0,max>)\n- IntRange <: IntRange (int<1,50> <: int<0,100>)\n- refined-int <: refined-int (positive-int <: non-negative-int)\n\nFixes #170.",
          "timestamp": "2026-07-10T11:45:33-05:00",
          "tree_id": "37378d981687a735b01a6b35cc34f565255315cb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e501c6da5a6113f74c25b6e66f0d32ff285bb361"
        },
        "date": 1783702667480,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 51,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "147a46cd4e6d642d7edc420298d17a3d056b6756",
          "message": "fix: variable assignments in if-branches no longer leak into elseif\n\nThe diagnostic scope cache already correctly records a clean scope\nsnapshot at each elseif condition boundary (forward_walk.rs:5628-5633),\npreventing assignments from preceding if/elseif bodies from affecting\ntype resolution in elseif conditions and bodies.\n\nAdd regression tests to prevent future regressions:\n- #167: $value = true in if-branch must not make elseif see bool\n- #168: instanceof narrowing must not leak into elseif body\n\nFixes #167, #168.",
          "timestamp": "2026-07-10T12:29:08-05:00",
          "tree_id": "6877954880aa0182f29a418de5a54fbe1bd69238",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/147a46cd4e6d642d7edc420298d17a3d056b6756"
        },
        "date": 1783705290424,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "741d0db1eea2cb61395bd2acd0a35a8edeb28880",
          "message": "fix: reassign from own array offset now updates variable type\n\n$value = $value[0] after $value held list<string>|false now correctly\nnarrows $value to string instead of keeping the old array|false type.\n\nRoot cause: resolve_rhs_array_access called extract_value_type(true)\nwhich skips scalar element types (designed for completion filtering).\nChanged to extract_element_type() (= extract_value_type(false)) so\nscalar types like string are preserved during assignment resolution.\n\nFixes #169.",
          "timestamp": "2026-07-10T12:54:31-05:00",
          "tree_id": "822ae9207506ecf76c165162ce0e3d80c171abb0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/741d0db1eea2cb61395bd2acd0a35a8edeb28880"
        },
        "date": 1783706733711,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "c77dc347ca5df981b2d31d45daec3c3e0e2228c9",
          "message": "fix: expand @phpstan-type aliases before argument type checking\n\nType aliases declared via @phpstan-type / @psalm-type (e.g. Payload,\nField, ValueType) are now expanded to their underlying types before\nargument compatibility checking. Previously the alias name was kept\nas-is and treated as an unknown class, producing false diagnostics\nlike 'expects ?array, got Payload'.\n\nTwo-part fix:\n1. Call resolve_type_alias_typed() during arg type resolution in\n   type_errors.rs to expand aliases to their underlying types.\n2. Add a safety-net escape hatch in is_type_compatible() for arg\n   types that are unresolvable non-class names (unexpanded aliases\n   from other files).\n\nFixes #166.",
          "timestamp": "2026-07-10T13:04:42-05:00",
          "tree_id": "08cc85c4cdf7a2fc1487ff33fb3acaafe0e08002",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c77dc347ca5df981b2d31d45daec3c3e0e2228c9"
        },
        "date": 1783707415075,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3bc8743c21c31b387499549580b8fdff827a5486",
          "message": "A class named after a pseudo-type is no longer shadowed by it",
          "timestamp": "2026-07-10T20:07:04+02:00",
          "tree_id": "a782177e5d727128ed258e9f4869f8de7be20dfc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3bc8743c21c31b387499549580b8fdff827a5486"
        },
        "date": 1783707581733,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "68f83e78604468fbea92bf19405cdc3a09460853",
          "message": "fix: iterator_to_array now returns array instead of iterator type\n\niterator_to_array($iter) where $iter is Iterator<Foo> now correctly\nresolves to list<Foo> (or array<K, V> when both key and value generic\nparams are available) instead of returning the raw Iterator type.\n\nRoot cause: resolve_array_func_raw_type returned the raw iterator\ntype directly instead of wrapping the extracted element type in an\narray. Also used extract_value_type(true) which skipped scalar\nelement types.\n\nFix: extract key/value types from the iterator and construct the\nappropriate array type (generic_array for key+value, list for\nvalue-only, bare array as fallback).\n\nFixes #173.",
          "timestamp": "2026-07-10T13:11:18-05:00",
          "tree_id": "796c77045f60dd8cfa03ac2027041e8b0fface81",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/68f83e78604468fbea92bf19405cdc3a09460853"
        },
        "date": 1783707822142,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fc3542849169e23208a73784723bb807386dd68f",
          "message": "A bit of clean up and regresion testing",
          "timestamp": "2026-07-10T20:29:25+02:00",
          "tree_id": "f8b44d9bc166b600688c4f0adc81597ec7bb8896",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fc3542849169e23208a73784723bb807386dd68f"
        },
        "date": 1783708921426,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "5a429cc876704d8c6c97c33dd3f797dcce55a1d3",
          "message": "feat: support overloaded function signatures in type checking\n\nFunctions with multiple PHP signatures (like strtr, implode,\narray_keys) now store alternate parameter lists in FunctionInfo.\noverloads. During stub parsing, duplicate function declarations\nwith different parameter counts are merged into overloads.\n\nThe type checker tries all overloads before emitting a diagnostic:\nif the call matches ANY signature, no error is reported.\n\nFixes #165",
          "timestamp": "2026-07-10T13:46:16-05:00",
          "tree_id": "3597449d80711cffdcdf262ed77114cd28fa385f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5a429cc876704d8c6c97c33dd3f797dcce55a1d3"
        },
        "date": 1783709958566,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "abc47aec0f4ac67baef7de917b3d454542841c02",
          "message": "Type narrowing works through the alternate `if:`/`endif;` syntax",
          "timestamp": "2026-07-10T22:52:30+02:00",
          "tree_id": "dcc66f024b7e041044be6d90c98bc5305360a202",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/abc47aec0f4ac67baef7de917b3d454542841c02"
        },
        "date": 1783717483988,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d4056a0b9623f94e91f625be151ee8f83c382cb1",
          "message": "Fix message",
          "timestamp": "2026-07-10T23:36:34+02:00",
          "tree_id": "ce78c2a2a347b9ebc1e0ba9f7a76496e5b64eb15",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d4056a0b9623f94e91f625be151ee8f83c382cb1"
        },
        "date": 1783720130552,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "e837dcd0e71062b2590ef2345e7907ae551e6729",
          "message": "feat: Laravel route controller method navigation and completion\n\nAdd go-to-definition, find-references, rename, hover, diagnostics, and\nautocompletion for method-name strings inside\nRoute::controller(X::class)->group(fn(){...}) closures.\n\nThe extraction layer emits standard MemberAccess spans during AST\nextraction, so all existing navigation features work automatically.\nA separate completion module detects the cursor context via text\nscanning and AST parsing to offer controller method completions.\n\nHandles ->controller() anywhere in the fluent chain, chained route\ncalls (->name(), etc.), nested groups where an inner ->controller()\nshadows the outer, and groups without ->controller() that inherit\nthe parent controller.",
          "timestamp": "2026-07-10T16:41:02-05:00",
          "tree_id": "189a0cec8bc721e73575dc7894042c2c2470f081",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e837dcd0e71062b2590ef2345e7907ae551e6729"
        },
        "date": 1783720404721,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8973d4bb930ea5a4bb3262150ff18a37bbb7d100",
          "message": "Improve semantic highlighting",
          "timestamp": "2026-07-11T00:16:42+02:00",
          "tree_id": "c4e92233d279aa976ef55548fb3b872499250bb7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8973d4bb930ea5a4bb3262150ff18a37bbb7d100"
        },
        "date": 1783722567169,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "8f4a79d4f44567ccab49900087cfce537ef33a57",
          "message": "feat: display package provenance in hover\n\nShow a colored badge in hover indicating where a symbol comes from:\n- 🟢 `package/name` for direct Composer dependencies\n- 🟠 `package/name` *(transitive)* for transitive dependencies\n- 🟣 `PHP` for core/extension symbols\n- No badge for project-local symbols\n\nThe vendor_package_origin_roots data structure now stores the Composer\npackage name alongside the origin tier and install path, read from\nvendor/composer/installed.json. New package_info_for_path/uri methods\non Backend return both the origin and the package name.\n\nProvenance is shown in hover for classes, methods, properties,\nconstants, and standalone functions.\n\nCloses #228.",
          "timestamp": "2026-07-10T17:27:57-05:00",
          "tree_id": "70e332196d0d6e598165195327ff380893004edd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8f4a79d4f44567ccab49900087cfce537ef33a57"
        },
        "date": 1783723217340,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "20a9aa1e54fe902df147ce05a258e1b5d8046486",
          "message": "fix: rank imported/same-namespace symbols above non-imported in completion\n\nSwap source_tier before origin_tier in the sort_text key for class,\nfunction, and constant completions. Previously origin_tier (project >\ncore > vendor) came first, so a non-imported project class could\noutrank an already-imported vendor class. Now an imported symbol always\nranks above a non-imported one regardless of provenance.\n\nAffects class_sort_text in class_completion.rs and\nflat_symbol_sort_text in symbol_ranking.rs.",
          "timestamp": "2026-07-10T17:28:16-05:00",
          "tree_id": "4056dac9650291fb7ad9167d85bfda59ee4accc9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/20a9aa1e54fe902df147ce05a258e1b5d8046486"
        },
        "date": 1783723219691,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "5b54443ddd19c1dece8de4f42725f47d0e985c7f",
          "message": "fix: classify symfony/polyfill-* packages as PHP core stubs\n\nPackages like symfony/polyfill-php83 backport PHP core classes and\nextension functions (e.g. \\Override). They were classified as\ntransitive vendor dependencies, which gave them low sort priority\nin completion and incorrect provenance display.\n\nNow any package whose name starts with 'symfony/polyfill-' is\nclassified as CoreStub, matching the built-in PHP stubs.",
          "timestamp": "2026-07-10T17:38:17-05:00",
          "tree_id": "d89287786582e8dda9e6e47c5c3d64c46c2614b5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5b54443ddd19c1dece8de4f42725f47d0e985c7f"
        },
        "date": 1783723946956,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2839ea246774c0e025ad62ee09d9f0e9cf2e6ff2",
          "message": "Add test and correct 🟣",
          "timestamp": "2026-07-11T00:45:49+02:00",
          "tree_id": "588f62129e775720cf73b3f6c786bc468e39a99c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2839ea246774c0e025ad62ee09d9f0e9cf2e6ff2"
        },
        "date": 1783724465320,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a6a709b3a25d3703d2a78c76c7bedbd17bb9bbc2",
          "message": "Fix both caes",
          "timestamp": "2026-07-11T00:56:49+02:00",
          "tree_id": "3a1c344ea5065bb65742fb7d42a556ce04174c45",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a6a709b3a25d3703d2a78c76c7bedbd17bb9bbc2"
        },
        "date": 1783724935443,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bf94f87af85cd309a999c5daa9ab16e17cfceb23",
          "message": "Fix callable string detection",
          "timestamp": "2026-07-11T01:58:02+02:00",
          "tree_id": "b41e80f6ff40848fba39f3706bcd7d9d64abafef",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bf94f87af85cd309a999c5daa9ab16e17cfceb23"
        },
        "date": 1783728643040,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e8fe8a8499b91f3dc163d6a60148253d23a42d96",
          "message": "Add diagnostic ignore rules in `.phpantom.toml`",
          "timestamp": "2026-07-11T07:43:47+02:00",
          "tree_id": "75a39969749869fb2dbf71e1e3b985ec025abd3d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e8fe8a8499b91f3dc163d6a60148253d23a42d96"
        },
        "date": 1783749377359,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a94b054c519165d509f06f785904f7f2d7024d9f",
          "message": "Assertion methods narrow types through inheritance",
          "timestamp": "2026-07-11T18:05:22+02:00",
          "tree_id": "8fe70d8175b5236cc44bbd4d391139bc1416de66",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a94b054c519165d509f06f785904f7f2d7024d9f"
        },
        "date": 1783786677242,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1ed3056c69a1666659b73f4201181858b3d27de9",
          "message": "Ternary conditions narrow property and method-call subjects",
          "timestamp": "2026-07-11T18:28:32+02:00",
          "tree_id": "80e561360f65fb5ceaf39cca8752f15b653d5bb6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1ed3056c69a1666659b73f4201181858b3d27de9"
        },
        "date": 1783788021954,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6abc068a78e1e885043845d3c725328245d1a508",
          "message": "Short-circuit conditions narrow their later operands",
          "timestamp": "2026-07-11T18:41:51+02:00",
          "tree_id": "87a5cf5a6ead2e96fbbedee965747a265658a378",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6abc068a78e1e885043845d3c725328245d1a508"
        },
        "date": 1783788855274,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8b0c9be846a2f7bc1151b5d2cd1621a5a35bed16",
          "message": "Assignments written inside a condition are now tracked",
          "timestamp": "2026-07-11T19:14:04+02:00",
          "tree_id": "c71d49f1021ddc29ee719d8ae5a6a6fee7263c98",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8b0c9be846a2f7bc1151b5d2cd1621a5a35bed16"
        },
        "date": 1783790782494,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ba903641b00f773fd52a09801a213f74e38340ca",
          "message": "The error-suppression operator (`@`) no longer blocks type resolution",
          "timestamp": "2026-07-11T19:21:53+02:00",
          "tree_id": "0030ad6cb175bd30015046cdcbdfc6d1e31eeec8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ba903641b00f773fd52a09801a213f74e38340ca"
        },
        "date": 1783791277571,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "621e6b7aeb4136049ba1528ecf7f3868388bedb0",
          "message": "Iterating an object that implements `Iterator` directly now resolves the\nloop variable's type",
          "timestamp": "2026-07-11T20:06:04+02:00",
          "tree_id": "3559812dcc0499d248628fde1c378076916433e8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/621e6b7aeb4136049ba1528ecf7f3868388bedb0"
        },
        "date": 1783793919340,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "12a707e6efe7dba7ba67bf8b44c5adee93c28d25",
          "message": "Integer literals rejected by refined-int pseudo-types",
          "timestamp": "2026-07-11T20:31:23+02:00",
          "tree_id": "cc8fd302d85d20f507059e919d6034a891c26243",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/12a707e6efe7dba7ba67bf8b44c5adee93c28d25"
        },
        "date": 1783795387674,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2dfaa2aa3a4c9d176c5e267cd36e349fc8f1ae6d",
          "message": "A string literal naming a class satisfies a `class-string<Bound>`\nparameter",
          "timestamp": "2026-07-11T23:56:31+02:00",
          "tree_id": "21b57642281556096507bc491d78eea285e2e4bc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2dfaa2aa3a4c9d176c5e267cd36e349fc8f1ae6d"
        },
        "date": 1783807737054,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "89a0067c8b8058b345c1e7ff233a62c4d4d4da27",
          "message": "Passing a class name to a `class-string<T>` generic parameter infers the\nclass, not the string type",
          "timestamp": "2026-07-12T00:27:32+02:00",
          "tree_id": "43f2153113c65e90596b5e70066731beab5dfe6e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/89a0067c8b8058b345c1e7ff233a62c4d4d4da27"
        },
        "date": 1783809604473,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8f83571dccfa60ceb7aa383341d9e16ab7bb958b",
          "message": "Passing a class constant to a generic parameter infers the constant's\nvalue type",
          "timestamp": "2026-07-12T00:40:23+02:00",
          "tree_id": "7a1f461b200ccbc08cb1de46aadae52d58ad62a1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8f83571dccfa60ceb7aa383341d9e16ab7bb958b"
        },
        "date": 1783810357007,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e056f01ab3a6381afbcc7bfb3ded0529b346c870",
          "message": "*Method calls handled by `__call` / `__callStatic` are no longer flagged\nas unknown",
          "timestamp": "2026-07-12T01:07:22+02:00",
          "tree_id": "e656ed1201d96f2e3c18bb095e25b2ef841297c9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e056f01ab3a6381afbcc7bfb3ded0529b346c870"
        },
        "date": 1783811998025,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "89d417eb04ab91d072ac52ac6b984e3626bf0282",
          "message": "PHPStan's `__benevolent<T>` wrapper type is recognized",
          "timestamp": "2026-07-12T01:35:21+02:00",
          "tree_id": "5c9a1c77db7f38b9d5432193fa60642a62cb52d6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/89d417eb04ab91d072ac52ac6b984e3626bf0282"
        },
        "date": 1783813630769,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3ac3a0583f0357ee3b814c37c5cdaa1661a01583",
          "message": "Passing `null` to an implicitly-nullable parameter is no longer flagged",
          "timestamp": "2026-07-12T01:57:55+02:00",
          "tree_id": "2ee6a7f649da177ef11573aac35933f8cea342ec",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3ac3a0583f0357ee3b814c37c5cdaa1661a01583"
        },
        "date": 1783815026956,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "15cd37f74fbf2cb4f9514e136e8f5cd0ccd07878",
          "message": "Indexing a positional array shape resolves the element type",
          "timestamp": "2026-07-12T04:15:15+02:00",
          "tree_id": "18077acafb5f2b0148ee21cc00fba05c3b4c072f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/15cd37f74fbf2cb4f9514e136e8f5cd0ccd07878"
        },
        "date": 1783823264984,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2e2bef9c59fc36e405b00447522c82a16ef5a596",
          "message": "A project class sharing a global interface's short name no longer breaks\nsubtype checks",
          "timestamp": "2026-07-12T04:30:34+02:00",
          "tree_id": "bfe9f22c9289863fb3613be5b629fe741893a8c6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2e2bef9c59fc36e405b00447522c82a16ef5a596"
        },
        "date": 1783824183788,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7fbd9828f3b42930486836a007c3c9dc43650db1",
          "message": "Values returned from a callback passed to a generic helper now resolve",
          "timestamp": "2026-07-12T05:07:05+02:00",
          "tree_id": "37d9d35e85b4f5f9a013045d1dab4ea5adcfc6af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7fbd9828f3b42930486836a007c3c9dc43650db1"
        },
        "date": 1783826361056,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d7b2f09664a154886818384bcc67ab57f9eef0be",
          "message": "IMprove resolving to a class-string",
          "timestamp": "2026-07-12T06:09:21+02:00",
          "tree_id": "fb2e5bad86e1892a7eea115e426ee61e153c8848",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d7b2f09664a154886818384bcc67ab57f9eef0be"
        },
        "date": 1783830121793,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d0a02e8df9012d3f1ddc65569d4dce567dd8c2d3",
          "message": "Bind each member for generic class-strings",
          "timestamp": "2026-07-12T07:22:07+02:00",
          "tree_id": "cff827b5579204b1a92a0be3f705d9bd15829d5c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d0a02e8df9012d3f1ddc65569d4dce567dd8c2d3"
        },
        "date": 1783834468933,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "15ee1267223b8884dcfa9c551b1d6a277eaf28db",
          "message": "Fix extracting type from `class-string<T>|T`",
          "timestamp": "2026-07-12T08:05:34+02:00",
          "tree_id": "35cb70491b9fd403ca71c0b7feb119f809684097",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/15ee1267223b8884dcfa9c551b1d6a277eaf28db"
        },
        "date": 1783837068086,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e699a4ed33c15283650c05e6185ab0143a35865e",
          "message": "Authenticated user resolves to the configured model",
          "timestamp": "2026-07-12T22:38:15+02:00",
          "tree_id": "953cb394bd0a3b607e0fd2db01c33abfbf7b2476",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e699a4ed33c15283650c05e6185ab0143a35865e"
        },
        "date": 1783889404367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 62.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "44e3387821b037d2efc0e9a01014b85214ec5d08",
          "message": "`$this` narrowed by `assert()` resolves inside closures with no\nenclosing class",
          "timestamp": "2026-07-13T00:47:38+02:00",
          "tree_id": "c0322a154e23625d19f295b3ba00d628b0ecea62",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/44e3387821b037d2efc0e9a01014b85214ec5d08"
        },
        "date": 1783897233584,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bb7868a6cf4b27273e1a49e84f2438562dc06dc3",
          "message": "`@mixin` of an Eloquent model exposes the model's synthesized members",
          "timestamp": "2026-07-13T01:12:01+02:00",
          "tree_id": "f08b7c9b3b6fbf7c6fb8fc223895242a57f64796",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bb7868a6cf4b27273e1a49e84f2438562dc06dc3"
        },
        "date": 1783898677189,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 39.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "41be3555d129d62ed88b6f2c4315a76918bf094b",
          "message": "Container string aliases",
          "timestamp": "2026-07-13T04:00:13+02:00",
          "tree_id": "65b9e8c7d0e4f8570c1504efd61ef8fa0d4bc76d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/41be3555d129d62ed88b6f2c4315a76918bf094b"
        },
        "date": 1783908810932,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8c29dba5400f84dcc46f2ffd35b94742e1dc91bb",
          "message": "Narrow return type of auth('guard')",
          "timestamp": "2026-07-13T12:08:58+02:00",
          "tree_id": "932a95e9f26983c3fd61f94374e7ebcc0016f03f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8c29dba5400f84dcc46f2ffd35b94742e1dc91bb"
        },
        "date": 1783938075103,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "27d4ef3198fbfe0635d091de0f4207cbaf6c2f9b",
          "message": "Add support for `@see Class#method` docblock references",
          "timestamp": "2026-07-13T12:25:37+02:00",
          "tree_id": "1267d3f47f930cd605aa25bfe4f8b48fb8663178",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/27d4ef3198fbfe0635d091de0f4207cbaf6c2f9b"
        },
        "date": 1783939105153,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8a2e6a83529e4441491012f13b438aa8d997f2b1",
          "message": "`stream_bucket_make_writeable()` results resolve on PHP versions before\n8.4",
          "timestamp": "2026-07-13T12:38:53+02:00",
          "tree_id": "41eb66a0089d905c5ef5f2c581836820739e9600",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8a2e6a83529e4441491012f13b438aa8d997f2b1"
        },
        "date": 1783939896201,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7f5f0cc7db67815e603a00891776b9aeb229bd80",
          "message": "Indexing an object implementing `ArrayAccess` resolves through\n`offsetGet`",
          "timestamp": "2026-07-13T12:58:30+02:00",
          "tree_id": "449c1d7fbaac4a22737667117db526c8178fd6f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7f5f0cc7db67815e603a00891776b9aeb229bd80"
        },
        "date": 1783941051684,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0646137970885cefc4aae3ca9877027a9fae67bc",
          "message": "Reassigning a variable using its own previous value resolves the\nreference correctly",
          "timestamp": "2026-07-13T13:10:08+02:00",
          "tree_id": "5d9b1ca98d557e472e2f1b3856b61a55dbb06773",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0646137970885cefc4aae3ca9877027a9fae67bc"
        },
        "date": 1783941743813,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0970a6462cc72906df426008118a614331bc093a",
          "message": "`array-key` satisfies an `int|string`",
          "timestamp": "2026-07-13T13:15:18+02:00",
          "tree_id": "d62d419d5ec223ae80ad4949de806d345b18eaa1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0970a6462cc72906df426008118a614331bc093a"
        },
        "date": 1783942069175,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9a8838a6c13fb47b46d572102879049e06e63df1",
          "message": "A generic helper call no longer borrows a type from an unrelated call\nsite",
          "timestamp": "2026-07-13T14:18:52+02:00",
          "tree_id": "6afa6e6d15eac619a228000eb1fd9f9686a76283",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9a8838a6c13fb47b46d572102879049e06e63df1"
        },
        "date": 1783945876620,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "893fe0fe8894af04f552a543da8340811bdea59a",
          "message": "Type-guard narrowing survives compound conditions",
          "timestamp": "2026-07-13T17:29:57+02:00",
          "tree_id": "35fcb7a522e17bedba843db985b802a7c972f4af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/893fe0fe8894af04f552a543da8340811bdea59a"
        },
        "date": 1783957314928,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 40.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d746fc9a17c6a30193234e0ac7d99ba039f2fa8a",
          "message": "A `class-string<A|B>` value satisfies a `class-string<T>` template\nparameter",
          "timestamp": "2026-07-13T17:52:10+02:00",
          "tree_id": "bd94ee510aaecdf1f5cefbf5f0e37a712e550e5f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d746fc9a17c6a30193234e0ac7d99ba039f2fa8a"
        },
        "date": 1783958664459,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7eb25787fdd0c2a161116082ff407d2b84beb0af",
          "message": "An `assertInstanceOf` with a variable class keeps the subject's type",
          "timestamp": "2026-07-13T18:15:43+02:00",
          "tree_id": "d23756dc542bd3d959b838dceb114bc43d07cfbd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7eb25787fdd0c2a161116082ff407d2b84beb0af"
        },
        "date": 1783960045367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "15e902ed2c9881d0fc1cb6cc288777771e158aad",
          "message": "`isset()` and `empty()` guard their own access",
          "timestamp": "2026-07-13T19:37:18+02:00",
          "tree_id": "160a503df6bf08bc01abb32ed50f3e6679c30a37",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/15e902ed2c9881d0fc1cb6cc288777771e158aad"
        },
        "date": 1783965019600,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c54e1cf232d05625f56414157e69d9f32254388a",
          "message": "PHPDoc tags indented with extra spaces after the asterisk are honored",
          "timestamp": "2026-07-13T20:18:33+02:00",
          "tree_id": "71137fb539883310cbb9c262c51c9b27678faba3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c54e1cf232d05625f56414157e69d9f32254388a"
        },
        "date": 1783967452688,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d1f7fb76d21366ab5adc4dd3a3af724753ae59c1",
          "message": "A method returning `object` or `?object` allows member access on its\nresult",
          "timestamp": "2026-07-13T22:56:06+02:00",
          "tree_id": "ffd95dc00f29d72fa462a4e5dbf348aec25ed3ed",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d1f7fb76d21366ab5adc4dd3a3af724753ae59c1"
        },
        "date": 1783976904876,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "147699cc1a687ce55b8e21f999c8e2194de268f2",
          "message": "`@mixin` of a template parameter resolves through its bound",
          "timestamp": "2026-07-14T00:47:03+02:00",
          "tree_id": "14420d3c6d69df4fd6c1cce65113a02012acc3db",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/147699cc1a687ce55b8e21f999c8e2194de268f2"
        },
        "date": 1783983559321,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cf7bd8b46db8b4c1608b10efc45efd67e9e176c1",
          "message": "A class named after a built-in resolves to the project's version",
          "timestamp": "2026-07-14T03:10:23+02:00",
          "tree_id": "19d3c83c9f571e9ac33154ec51aa34d16278db30",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cf7bd8b46db8b4c1608b10efc45efd67e9e176c1"
        },
        "date": 1783992177253,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "35a7f52c8a323420b733be57d8905b5c102ccf05",
          "message": " Assigning an object to a property tracks that property's type",
          "timestamp": "2026-07-14T03:58:53+02:00",
          "tree_id": "b961e447c58b344a31633ffdcc4d13321885d2e3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/35a7f52c8a323420b733be57d8905b5c102ccf05"
        },
        "date": 1783995124365,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "755d40bd232ce5acc711ee3a18934a8043e51fad",
          "message": "Assigning `null` to a property tracks",
          "timestamp": "2026-07-14T04:30:51+02:00",
          "tree_id": "149d87bf0ea3fa1179166d7ac1c3dbea5ad2ec49",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/755d40bd232ce5acc711ee3a18934a8043e51fad"
        },
        "date": 1783997008068,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a6ee6fd0a8c90a13f1fe41d58f198402b35009ed",
          "message": "Fix performance regression for auth()",
          "timestamp": "2026-07-14T12:24:36+02:00",
          "tree_id": "695986220f08980ad2ef64e87a7e301e2bcf815c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a6ee6fd0a8c90a13f1fe41d58f198402b35009ed"
        },
        "date": 1784025411543,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ddc102c2aadcf0c24b320210b13b61c8946f700c",
          "message": "A namespaced class name passed as a string literal resolves",
          "timestamp": "2026-07-14T23:20:24+02:00",
          "tree_id": "5cfca84ba4617ad7e94782ae47ec287226735d28",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ddc102c2aadcf0c24b320210b13b61c8946f700c"
        },
        "date": 1784064772747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f191d25e4f1c2c363d85885efb5864585188092b",
          "message": "A mock built with a test helper keeps the mocked class",
          "timestamp": "2026-07-15T03:01:49+02:00",
          "tree_id": "77ad899c8bf51670a523486b945a49f86ffa3a89",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f191d25e4f1c2c363d85885efb5864585188092b"
        },
        "date": 1784078059342,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "71c259dced403eae46517db923a694bdf4a5156f",
          "message": "Mockery pathc return types",
          "timestamp": "2026-07-15T05:04:28+02:00",
          "tree_id": "58ce1d13809927e299fd78d4fdb10c5ad503ed39",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/71c259dced403eae46517db923a694bdf4a5156f"
        },
        "date": 1784085436497,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "54e91318bda30dec719c3caa0fb51dc132c2311e",
          "message": "`$this` inside an anonymous class resolves to that class",
          "timestamp": "2026-07-15T05:27:19+02:00",
          "tree_id": "921e1f071f9065e18cb213777fcc89d28820905f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/54e91318bda30dec719c3caa0fb51dc132c2311e"
        },
        "date": 1784086765346,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "69317a0c0821c7edf7d616ab3b3a785f88901664",
          "message": "Eloquent relations resolve regardless of the case used to access them",
          "timestamp": "2026-07-15T05:38:37+02:00",
          "tree_id": "d7e00e1dec0546de24ebaaa4b70b032b1a679442",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/69317a0c0821c7edf7d616ab3b3a785f88901664"
        },
        "date": 1784087462178,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "37f12fa6290d26f9fc9d4bdfc00884b37f09dc75",
          "message": "A variable used only as a dynamic member name is no longer reported\nunused",
          "timestamp": "2026-07-15T05:44:19+02:00",
          "tree_id": "93b09ea6e3178966507f55f0badac7cbb511460e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/37f12fa6290d26f9fc9d4bdfc00884b37f09dc75"
        },
        "date": 1784087820360,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5ddfe9447452ebbeec12d61de0b8290c9b6dec34",
          "message": "`compact()` with an array argument counts its variables as used",
          "timestamp": "2026-07-15T05:56:22+02:00",
          "tree_id": "66bab6887121568315d71c27b7b30ec3ce3c6c69",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5ddfe9447452ebbeec12d61de0b8290c9b6dec34"
        },
        "date": 1784088512271,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "895fc94593bad84c8abbe8069ae152e84a07b327",
          "message": "Fix clippy",
          "timestamp": "2026-07-15T15:29:09+02:00",
          "tree_id": "34e458403bb7b73c5f93d070663b93515ea62572",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/895fc94593bad84c8abbe8069ae152e84a07b327"
        },
        "date": 1784122813285,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "lj.moritorii@web.de",
            "name": "Moritz Wirger",
            "username": "enwi"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "55c05db6c657a1affe4420d0abc9dcba0496a022",
          "message": "Respect mago toml",
          "timestamp": "2026-07-15T15:43:13+02:00",
          "tree_id": "fbb5487f50e9dc6d72e3e338a405b6ca2903a96d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/55c05db6c657a1affe4420d0abc9dcba0496a022"
        },
        "date": 1784123736747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "5d921e836cf07de76ed2c48726ed26aac08adff3",
          "message": "feat(rename): update $param references in conditional return types\n\nExtend extract_param_var_spans() to scan @return / @phpstan-return /\n@psalm-return tags for parameter references inside conditional return\ntype annotations (e.g. `@return ($param is true ? T : U)`).\n\nPreviously, renaming a function parameter updated the signature,\n@param tag, and body usages but left the $param reference in the\n@return conditional stale. Now all occurrences — including nested\nconditionals — are renamed together.",
          "timestamp": "2026-07-15T09:14:35-05:00",
          "tree_id": "bcc434a9feef84ff3ae15571265e09a83f7e4780",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5d921e836cf07de76ed2c48726ed26aac08adff3"
        },
        "date": 1784125617436,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "495373f148a6e2d0f74f29d56dbded35b586d07c",
          "message": "feat: resolve @param-closure-this in hover, go-to-definition, and go-to-type-definition\n\nWhen $this is inside a closure whose enclosing call site declares\n@param-closure-this, hover now shows the overridden type instead of\nthe lexically enclosing class. Go-to-definition and\ngo-to-type-definition on $this likewise jump to the overridden\nclass declaration. Previously only completion resolved the override.\n\nAdds 3 integration tests covering the override, fallback, and\nstandalone-function cases.",
          "timestamp": "2026-07-15T11:42:36-05:00",
          "tree_id": "0bd981ec2fed99cbdb0d2baa4834ffcacd605ae4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/495373f148a6e2d0f74f29d56dbded35b586d07c"
        },
        "date": 1784134454521,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b1063ffd2d0d950eb8079bcbc004563e45d127af",
          "message": "Laravel macros registered in your code are recognized as real methods",
          "timestamp": "2026-07-15T18:43:14+02:00",
          "tree_id": "1d2cbb77639d6c5b073a19147455b6890206f8e2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b1063ffd2d0d950eb8079bcbc004563e45d127af"
        },
        "date": 1784134548926,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "01d6a6986af28af8b1a1005ab9e7fc49bc418f51",
          "message": "Laravel macros are recognized as real methods",
          "timestamp": "2026-07-15T20:49:58+02:00",
          "tree_id": "6f02403b0ed0b2d297ad0a22b23fa0b90a44635d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/01d6a6986af28af8b1a1005ab9e7fc49bc418f51"
        },
        "date": 1784142146489,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "62ffa645b2614b1eff44b5517d15808bd8253529",
          "message": "feat: show provenance badges for external path-repository packages\n\nProvenance detection (package_info_for_path) no longer assumes every\npath outside vendor/ is project code. Symlinked path-repository\npackages whose canonical path resolves outside the workspace root now\nshow the package name badge in hover instead of being silently treated\nas project code. Path-repo packages inside the workspace (e.g. modular\napp modules) continue to show no badge.\n\nPreviously, canonicalize() followed the symlink to a path outside\nvendor/, the starts_with(vendor_path) check failed, and the function\nreturned Project unconditionally.",
          "timestamp": "2026-07-15T14:13:16-05:00",
          "tree_id": "cc1bff12bdcf7f20f6eaf13a98813ee10565aeff",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/62ffa645b2614b1eff44b5517d15808bd8253529"
        },
        "date": 1784143509809,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3a1c6e7fe66fb1043e4bb49ee7a9abb6b31664f5",
          "message": "Fix longest-prefix-first invariant",
          "timestamp": "2026-07-15T21:53:42+02:00",
          "tree_id": "ce71e77438943c5ce87d02ce473be48ea0e69701",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3a1c6e7fe66fb1043e4bb49ee7a9abb6b31664f5"
        },
        "date": 1784145991842,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ae3197d1d57e8cc8ec635fd5cffa42621d196125",
          "message": "Handle macro registered through a facade",
          "timestamp": "2026-07-15T22:30:05+02:00",
          "tree_id": "88987121f855d1b887cd9e7ced8a6b18fe24d443",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ae3197d1d57e8cc8ec635fd5cffa42621d196125"
        },
        "date": 1784148174014,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1329ed844b7f08323065c00af361b218b6bb73c2",
          "message": "Laravel: Resolve $request->user()",
          "timestamp": "2026-07-15T23:29:45+02:00",
          "tree_id": "5052e003feaff8d86ce43d5ea19c208fc0872049",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1329ed844b7f08323065c00af361b218b6bb73c2"
        },
        "date": 1784151751479,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0caf399fc414958e41c7a8e0f373843a6ec96b71",
          "message": "ach `if`/`elseif` branch narrows a property path to its own type",
          "timestamp": "2026-07-16T00:45:55+02:00",
          "tree_id": "100498b5cb58dac81a9d4dfa4bed3b310bfdf9cf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0caf399fc414958e41c7a8e0f373843a6ec96b71"
        },
        "date": 1784156298862,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "22326d04eb307cd200407dd1b7d58646423cf0ce",
          "message": "A  `class_exists()` guard keeps a variable's concrete class-string type",
          "timestamp": "2026-07-16T01:12:37+02:00",
          "tree_id": "cfb5eb77755c8f0444478f6129e84c86ddb99cc6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/22326d04eb307cd200407dd1b7d58646423cf0ce"
        },
        "date": 1784157920068,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3ee0bab943634d344e9068a82182aca182359515",
          "message": "`property_exists()` and `method_exists()` guards prove the member exists",
          "timestamp": "2026-07-16T03:55:52+02:00",
          "tree_id": "feca41f1cc4734645898d0f8d497c167beeab8bd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3ee0bab943634d344e9068a82182aca182359515"
        },
        "date": 1784167717985,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2f6e6a85140170b61ad816245d4e47a7b575a0e6",
          "message": "`foreach` over SPL iterators resolves the element type",
          "timestamp": "2026-07-16T04:37:04+02:00",
          "tree_id": "3aa447018b606a88efe615cba8f8b16f875f3ebf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2f6e6a85140170b61ad816245d4e47a7b575a0e6"
        },
        "date": 1784170169997,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c73e4101f6b8435a1f5afed76077f3077a21d55e",
          "message": "Resolve shapes written across multiple lines",
          "timestamp": "2026-07-16T04:56:04+02:00",
          "tree_id": "ad2a700706d9d583714eb06681848c8626c75c47",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c73e4101f6b8435a1f5afed76077f3077a21d55e"
        },
        "date": 1784171303226,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bbc84a9737690e9612c8dae982cbd9107fd942a8",
          "message": "Simit what is seen as a application component",
          "timestamp": "2026-07-16T14:01:47+02:00",
          "tree_id": "2debdebd4da2e42f4268341ba765c4d25ab86a90",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bbc84a9737690e9612c8dae982cbd9107fd942a8"
        },
        "date": 1784204037899,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "db4025e7a8bf2e7b48f8497506f6b756ece690c0",
          "message": "Parenthesized return types resolve instead of being dropped",
          "timestamp": "2026-07-16T15:37:25+02:00",
          "tree_id": "fb76d438c45118207ebd2a4baf7b99d0f851f91a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/db4025e7a8bf2e7b48f8497506f6b756ece690c0"
        },
        "date": 1784209930401,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8f1eaf52b4cbab78125d349da558ab9611f8c5ee",
          "message": "`@phpstan-require-extends` gives `$this` the base class's members inside\na trait",
          "timestamp": "2026-07-16T16:02:29+02:00",
          "tree_id": "d8ddda6b55f852aa5b4fe74bf78a71c5fad7083f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8f1eaf52b4cbab78125d349da558ab9611f8c5ee"
        },
        "date": 1784211425132,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 63.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ec7e96c4b69d03ae5e06875cd2f7f6dbe1fff496",
          "message": "A leading-backslash type resolves to the global class even when a\nsame-named class is imported",
          "timestamp": "2026-07-16T16:32:16+02:00",
          "tree_id": "4bd393536e075d64cc898d641a7434e2b0637ffb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ec7e96c4b69d03ae5e06875cd2f7f6dbe1fff496"
        },
        "date": 1784213216346,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "449789cdba21191d4abcd22d8e97d5a3f2e910a7",
          "message": "fix: duplicate native diagnostics in pull-mode editors\n\nSwitch native diagnostics to a pull-only delivery model when the client\nsupports pull diagnostics. Cache and refresh after both the fast pass\nand the full slow pass so quick diagnostics stay responsive without\nduplicating squiggles in clients that keep pushed and pulled diagnostics\nseparate.",
          "timestamp": "2026-07-16T11:15:07-05:00",
          "tree_id": "444b3ba08acf76e7217a5a1a96fdbe2748a06925",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/449789cdba21191d4abcd22d8e97d5a3f2e910a7"
        },
        "date": 1784219381245,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "67a925ceac31be1717e9ef841070671a2ccd6f2c",
          "message": "An inline `@var` before a `foreach` refines a broad iterable variable",
          "timestamp": "2026-07-16T20:05:02+02:00",
          "tree_id": "a036a23f21a4683ad477d90ec2bf3f5a79edb01f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/67a925ceac31be1717e9ef841070671a2ccd6f2c"
        },
        "date": 1784225907355,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "421f71ad2826302a5d20e8096c321a1abf31daa7",
          "message": "Gate class_name_mismatch on PSR-4 and a fe clean ups",
          "timestamp": "2026-07-16T21:22:34+02:00",
          "tree_id": "230d40904ef64f98b6ccb3f1ac7101a2fdfb3fdd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/421f71ad2826302a5d20e8096c321a1abf31daa7"
        },
        "date": 1784230637750,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2f523ddeb5e12835be7e2c6c01c4f5b1c7902372",
          "message": "Case-sensitive autoloading diagnostic",
          "timestamp": "2026-07-16T22:02:31+02:00",
          "tree_id": "e442e9903b4ac0523f540c3933e8579096aeeab8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2f523ddeb5e12835be7e2c6c01c4f5b1c7902372"
        },
        "date": 1784233025635,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "1272ab55b837dd85562edf20b23945e8d331c990",
          "message": "fix: suppress PSR-4 mismatch warnings for inline test fixtures\n\nSkip the namespace and filename mismatch diagnostics in files that mix\ntop-level executable statements with an inline enum, trait, class, or\ninterface. This keeps embedded test fixtures and stubs quiet while\npreserving the diagnostics for normal single-class PSR-4 files, and\nadds regression tests covering the inline-fixture shape.",
          "timestamp": "2026-07-16T16:10:33-05:00",
          "tree_id": "1f015ce1db5a9f3e28ac42095c19139430a86e9f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1272ab55b837dd85562edf20b23945e8d331c990"
        },
        "date": 1784237050696,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "49f400b3d4388feec67550a10d7438a5fe978737",
          "message": "`instanceof` narrows a parameter inside an arrow-function body",
          "timestamp": "2026-07-17T00:08:45+02:00",
          "tree_id": "f39609b59d1e6e57f13bb66580833bc80e5d1226",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/49f400b3d4388feec67550a10d7438a5fe978737"
        },
        "date": 1784240587763,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "577f5fea6feae445d6633d0440b2fbceb264ad3a",
          "message": "Inline test fixtures no longer trigger PSR-4 mismatch warnings",
          "timestamp": "2026-07-17T00:46:59+02:00",
          "tree_id": "eeb859504abe93926ee3fc65d09d0ef0792a9d0b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/577f5fea6feae445d6633d0440b2fbceb264ad3a"
        },
        "date": 1784242878866,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "17714c1af4ec8ee05449761cb3cbe40e786c2125",
          "message": "Fluent chains through a trait's `return $this` keep the using class",
          "timestamp": "2026-07-17T03:25:07+02:00",
          "tree_id": "c8da357165ec5df010975553a5130e37775b7dec",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/17714c1af4ec8ee05449761cb3cbe40e786c2125"
        },
        "date": 1784252414131,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6517804eca50cd210e78711a7a4deac7535e6c7f",
          "message": "Conditional return types with generic `static<...>` branches keep their\ntype arguments",
          "timestamp": "2026-07-17T03:59:52+02:00",
          "tree_id": "ed14be40e5e832b440eb5f2b4e5826c4fc626839",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6517804eca50cd210e78711a7a4deac7535e6c7f"
        },
        "date": 1784254461767,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "02026e275f1f417b67188282263347e390788764",
          "message": "docs: clarify native diagnostic transport policy\n\nDocument that native diagnostics prefer pull delivery when the client supports it and use push only as a fallback for push-only clients. Make the server comments and architecture notes explicit that PHPantom intentionally avoids mixing both native transport models for the same client because that creates competing diagnostic streams with client-dependent merge behavior.",
          "timestamp": "2026-07-16T22:00:38-05:00",
          "tree_id": "1a8b904448c49262a667cad13d46d02dc0aca884",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/02026e275f1f417b67188282263347e390788764"
        },
        "date": 1784258076714,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 51.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1cee0ba5609bc5996740962aa2584bc60d56c5e4",
          "message": "Conditional return types whose selected branch is `mixed` stay usable",
          "timestamp": "2026-07-17T06:52:11+02:00",
          "tree_id": "694c71915d98a0491d08fba313416073fcb26c3f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1cee0ba5609bc5996740962aa2584bc60d56c5e4"
        },
        "date": 1784264792539,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7d288e3cceb12b44bea1db38d0bef49f9d6ee610",
          "message": "Leading-backslash global function calls resolve in member chains",
          "timestamp": "2026-07-17T07:23:08+02:00",
          "tree_id": "882480f76bed7f043d632963d68a28ba3b661051",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7d288e3cceb12b44bea1db38d0bef49f9d6ee610"
        },
        "date": 1784266634896,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 41.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fca9e96c9bfdcb13a1af232d98828adaf0a5d830",
          "message": "An `array<T>|false` return keeps its element type after a `false` check",
          "timestamp": "2026-07-17T07:38:08+02:00",
          "tree_id": "7e991ebf3fe1a5839962bedd5724139db90b84f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fca9e96c9bfdcb13a1af232d98828adaf0a5d830"
        },
        "date": 1784267575607,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "099a50fa6c5de92bbeb5fcbcf03dd04095398e73",
          "message": "feat: improve laravel macro discovery\n\nDiscover Laravel macros from provider-rooted helper classes while\npreserving real vendor package registrations such as Livewire, Inertia,\nNightwatch, and Laraflake. Cache cheap macro-token checks to avoid\nrepeated scans, and support statically typed variable macro\nregistrations so patterns like Builder  followed by\n->macro(...) resolve correctly.",
          "timestamp": "2026-07-17T07:33:38-05:00",
          "tree_id": "f23ee73f3ae5a5fc4eae30ee1786a9c97998b43d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/099a50fa6c5de92bbeb5fcbcf03dd04095398e73"
        },
        "date": 1784292520693,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "e7172f5f4f5981ad854036e4a34cecbf067c6835",
          "message": "feat: improve laravel macro rename and references\n\nTreat Laravel macro registration strings as first-class rename and\nreference targets so edits propagate between ::macro('name', ...)\nand matching call sites, including collection-style fluent chains.\nLand macro go-to-definition on the macro name itself, index\nmember-access names for cheaper workspace scans, and warm workspace\nsymbol maps in the background so repeated rename and reference\nrequests avoid reparsing unopened files.",
          "timestamp": "2026-07-17T10:54:23-05:00",
          "tree_id": "ed30e35a01aa1a1349010f55c0de62306745f5b7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e7172f5f4f5981ad854036e4a34cecbf067c6835"
        },
        "date": 1784304548175,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c250428c6eca8b2a00ba4c4fa4a0fd29beced9a6",
          "message": "Analysis results no longer vary between runs of the same project",
          "timestamp": "2026-07-17T18:02:44+02:00",
          "tree_id": "96361e5876b1e449415b32ca5a2d1d2556ed0762",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c250428c6eca8b2a00ba4c4fa4a0fd29beced9a6"
        },
        "date": 1784305041993,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f07694c8eea7280a5457659153e30582265a5f5e",
          "message": "Fix caching issues",
          "timestamp": "2026-07-17T18:28:26+02:00",
          "tree_id": "0c445ffb553e2950d6c1c582049789207ebbf4a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f07694c8eea7280a5457659153e30582265a5f5e"
        },
        "date": 1784306576830,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bebbfb8385bfdc3d1e7d2a0504bf35169cc5e3a7",
          "message": "Indexing an array with a dynamic key resolves the element type",
          "timestamp": "2026-07-17T18:54:49+02:00",
          "tree_id": "cb73924562c900b00da883d5e21023059d6ddfaa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bebbfb8385bfdc3d1e7d2a0504bf35169cc5e3a7"
        },
        "date": 1784308153810,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "131f63c67fc0b21b5b190f7a444efdaacea5e8ca",
          "message": "Callable return templates bind from an unannotated closure's typed\nparameters",
          "timestamp": "2026-07-17T19:33:09+02:00",
          "tree_id": "2d1ac731bee7fa5e94d8caf7800f52aa3c70f819",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/131f63c67fc0b21b5b190f7a444efdaacea5e8ca"
        },
        "date": 1784310456891,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "867c5a1522e6cf053dac1910b5ae167a5ffbff81",
          "message": "A templated helper with a `class-string` default resolves when called\nwith no arguments",
          "timestamp": "2026-07-17T20:05:42+02:00",
          "tree_id": "f34b2e568028ec729a89ff8c23f61f598cd9b4cf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/867c5a1522e6cf053dac1910b5ae167a5ffbff81"
        },
        "date": 1784312400089,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9d2e8495b96b2eec146ab5dc14bf0092a78b0a14",
          "message": "Handle  `isset($obj->prop)` guards",
          "timestamp": "2026-07-17T20:20:02+02:00",
          "tree_id": "6c5635b291b0573d250d3c3eddc9ed9e63f291ce",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9d2e8495b96b2eec146ab5dc14bf0092a78b0a14"
        },
        "date": 1784313269121,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f240bfc4730a53edd4f97efbd5cb9a15b126ee20",
          "message": "A closure parameter's declared type is kept when the collection's\nelement type is a partial union",
          "timestamp": "2026-07-17T20:39:46+02:00",
          "tree_id": "ab3f0ed9cb1813b009cccd639972870c5fdb5067",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f240bfc4730a53edd4f97efbd5cb9a15b126ee20"
        },
        "date": 1784314448723,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "carnage@users.noreply.github.com",
            "name": "carnage",
            "username": "carnage"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3eb8d330c068ac2b8c63b1f2ffb7177447522498",
          "message": "Add opencode setup instructions",
          "timestamp": "2026-07-17T20:49:37+02:00",
          "tree_id": "ddecb7eb26b14c37542a22b334d8d0d10c8e97df",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3eb8d330c068ac2b8c63b1f2ffb7177447522498"
        },
        "date": 1784315027174,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f0390d975c87869212d52482ea9695b5cf96caa2",
          "message": "A `for` loop's init-clause variable resolves in the condition and update\nclauses",
          "timestamp": "2026-07-17T20:52:44+02:00",
          "tree_id": "9a3e1e7570ab795157b3a18e6e1c94b56f0b3412",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f0390d975c87869212d52482ea9695b5cf96caa2"
        },
        "date": 1784315213591,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2dce5e053fbfa7da2152d165ff617ff18b240245",
          "message": "A variable destructured from an untyped array can be narrowed by a later\nassertion",
          "timestamp": "2026-07-17T22:34:44+02:00",
          "tree_id": "860564cbd177541461a8f508260312cab2df4b4f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2dce5e053fbfa7da2152d165ff617ff18b240245"
        },
        "date": 1784321344485,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "14fc0b8a63c61e01b99917d0d1d31ed4b50a59a0",
          "message": "feat: macro hover with origin indicator and inferred return types\n\nMacro methods now show a \"macro\" origin indicator in hover instead\nof the generic \"virtual\" label, helping users distinguish ::macro()\nregistrations from @method/@mixin synthesized members.\n\nWhen a macro closure has no explicit return type hint, the return\ntype is inferred from the closure body and displayed with an\n\"(inferred)\" annotation. This also applies to regular methods\nwhose return types are inferred at hover time.\n\nA new preserve_static flag on ResolutionCtx controls whether\n$this/self/static resolve to their keyword form or the concrete\nclass name. For method chains like $this->transform(...), the\nlast method's declared return type is used directly via\nresolve_chain_declared_return(), preserving $this, static, and\ngeneric parameters that the general expression resolver would\nflatten to a bare class name. Raw class methods are checked\nbefore the fully-resolved class so template parameter names\n(e.g. TValue) are preserved instead of being replaced with\ntheir bounds.\n\nKey changes:\n- is_macro and is_inferred_return fields on MethodInfo\n- MemberOrigin::Macro variant for hover rendering\n- preserve_static on ResolutionCtx with chain-aware resolution\n- resolve_chain_declared_return() for declared return type lookup\n- 4 new integration tests for hover behavior",
          "timestamp": "2026-07-17T15:59:40-05:00",
          "tree_id": "e3b13f2e4b381f2baf6c6703ff27f5c339c1de83",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/14fc0b8a63c61e01b99917d0d1d31ed4b50a59a0"
        },
        "date": 1784322855204,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e0c40f54eff26018d36217ce6ab2c87d18ccf56d",
          "message": "`assertInstanceOf` narrows when the expected class is held in a variable",
          "timestamp": "2026-07-18T01:41:16+02:00",
          "tree_id": "488e566edd2c2bdc962b0f9bd3ab084549bbfd1f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e0c40f54eff26018d36217ce6ab2c87d18ccf56d"
        },
        "date": 1784332517942,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c349a58037d4bfcae1e4b3db8784432390372e27",
          "message": "Preserve static context",
          "timestamp": "2026-07-18T01:48:17+02:00",
          "tree_id": "f32a5221caf6095fd840f255358913dceebc1260",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c349a58037d4bfcae1e4b3db8784432390372e27"
        },
        "date": 1784332958646,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "dde234fea1872d41ce1ff05ed05a8928333d993a",
          "message": "Fix assertInstanceOf on list-destructured",
          "timestamp": "2026-07-18T02:07:17+02:00",
          "tree_id": "4cfd111ecdac2e327bfdf2cca74e2b72e93c2640",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dde234fea1872d41ce1ff05ed05a8928333d993a"
        },
        "date": 1784334053849,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ffc25b79cf5cf5c7723b8061e46d23e9632e18b3",
          "message": "Container string aliases and global facades resolve",
          "timestamp": "2026-07-18T02:13:45+02:00",
          "tree_id": "f8ac637f71e5cb1b8c9477b2aaaa617282687836",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ffc25b79cf5cf5c7723b8061e46d23e9632e18b3"
        },
        "date": 1784334489537,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b7509ebe45389bf3ab7486941bd14e2223d7656e",
          "message": "Fix crash on property self assignment",
          "timestamp": "2026-07-18T02:41:45+02:00",
          "tree_id": "2a665d5fa06679300925772cf46d01c83dbd2715",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b7509ebe45389bf3ab7486941bd14e2223d7656e"
        },
        "date": 1784336214608,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "85396fce560e67fb0c5543e030fd1d950c970041",
          "message": "feat: add return type and property type mismatch diagnostics (#220)\n\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-18T08:07:57+02:00",
          "tree_id": "e7e9834b9c4f9c9824828dc1cc641b25f1b78a8a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/85396fce560e67fb0c5543e030fd1d950c970041"
        },
        "date": 1784355733435,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e1d023cd7786cb35309712bc1d02690a7e8948ec",
          "message": "Clean up changelog",
          "timestamp": "2026-07-18T08:55:30+02:00",
          "tree_id": "da98fa4fb52e51df9c288c1aa2feca002a306fcd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e1d023cd7786cb35309712bc1d02690a7e8948ec"
        },
        "date": 1784358597632,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "de6fa15b665af270a69bcab611ade15d879531f4",
          "message": "Clean up stack size and make notes on perf",
          "timestamp": "2026-07-18T20:00:38+02:00",
          "tree_id": "d29b8b471c67fbdfc90b9413d7d6ec446c553966",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/de6fa15b665af270a69bcab611ade15d879531f4"
        },
        "date": 1784398516156,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "86301251759f0e67cbdad0064a626a60f1ccf32e",
          "message": "fix: convert HTML list tags to markdown in hover rendering\n\nstrip_html_tags() was silently dropping <ul>, <ol>, <li> tags, losing\nall list structure. html_to_markdown() passed them through raw, rendering\nas literal HTML text in hover popups.\n\nBoth functions now emit markdown equivalents: <li> becomes '- ', </li>\nbecomes a newline, and <ul>/<ol> boundaries emit newlines. Also added\nhandling for <strong>, <em>, <dl>, <dt>, <dd>, and <span> tags in\nhtml_to_markdown().\n\nRefactored strip_html_tags() from a boolean is_html flag to a match\nwith per-tag replacement strings.",
          "timestamp": "2026-07-18T14:24:12-05:00",
          "tree_id": "eabb27f27992f3ab7d1ed10058b19904ca9d4739",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/86301251759f0e67cbdad0064a626a60f1ccf32e"
        },
        "date": 1784403509781,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "38aa72bf1018b9ae8b93bf810e27df2a9bd13898",
          "message": "fix: convert HTML list tags to markdown in hover rendering\n\nstrip_html_tags() was silently dropping <ul>, <ol>, <li> tags, losing\nall list structure. html_to_markdown() passed them through raw, rendering\nas literal HTML text in hover popups.\n\nBoth functions now emit markdown equivalents: <li> becomes '- ', </li>\nbecomes a newline, and <ul>/<ol> boundaries emit newlines. Also added\nhandling for <strong>, <em>, <dl>, <dt>, <dd>, and <span> tags in\nhtml_to_markdown().\n\nRefactored strip_html_tags() from a boolean is_html flag to a match\nwith per-tag replacement strings.",
          "timestamp": "2026-07-18T14:25:07-05:00",
          "tree_id": "82d55829e2332941b05c127cea1ef146cb454b64",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/38aa72bf1018b9ae8b93bf810e27df2a9bd13898"
        },
        "date": 1784403526243,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ac1797cdebc0f50d3e01e0248f715c381a58d584",
          "message": "Set a reasonable stack size for threads",
          "timestamp": "2026-07-18T22:08:01+02:00",
          "tree_id": "b4be48649d06f094e25f9975fcfde76b28a6bb04",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ac1797cdebc0f50d3e01e0248f715c381a58d584"
        },
        "date": 1784406153242,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 44,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2f1238b1f7bde140511a90764a883c7605195014",
          "message": "Convert some more HTML to MD",
          "timestamp": "2026-07-18T22:18:09+02:00",
          "tree_id": "5c36c31f9a48cef63955a31cdbf09bca2a083ec2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2f1238b1f7bde140511a90764a883c7605195014"
        },
        "date": 1784406769638,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a066987013315ce95d54e5a1eeccee128ec95a3c",
          "message": "Memory no longer grows for the whole session as files are closed",
          "timestamp": "2026-07-18T22:28:48+02:00",
          "tree_id": "c5670300599937a8433a3ba7ab9cb82378458a3c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a066987013315ce95d54e5a1eeccee128ec95a3c"
        },
        "date": 1784407409429,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2baf71c9317e696f119c52a4e23bb5d4f7d38697",
          "message": "Lower memory use while indexing",
          "timestamp": "2026-07-18T23:01:58+02:00",
          "tree_id": "78b3f9d7e05c1fac02004941517496e008fc67a3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2baf71c9317e696f119c52a4e23bb5d4f7d38697"
        },
        "date": 1784409399071,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f1ef4a20476eb21ae63e1c70159a2b502b404a59",
          "message": "Correct repo references",
          "timestamp": "2026-07-19T00:03:45+02:00",
          "tree_id": "a3814fc9191c7a3a6cb5ca0b8183466ca7352c00",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f1ef4a20476eb21ae63e1c70159a2b502b404a59"
        },
        "date": 1784413101424,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d61d3559cebba1c26025fcbe7f56bfc0d56a3743",
          "message": "Method completion no longer inserts a duplicate pair of parentheses",
          "timestamp": "2026-07-19T02:32:02+02:00",
          "tree_id": "195083f64869cf81d97099c14df7d6265a12a73c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d61d3559cebba1c26025fcbe7f56bfc0d56a3743"
        },
        "date": 1784422012686,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "ec64d3bada6bd175bb30a1fd0c065f52c653a22c",
          "message": "fix: infer configured date factory class",
          "timestamp": "2026-07-18T22:01:53-05:00",
          "tree_id": "acf32ef59d2a34bcce0c49793b9b6839c7a11829",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ec64d3bada6bd175bb30a1fd0c065f52c653a22c"
        },
        "date": 1784430994367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ea9c01a292ecd12180f6620dbe3c25f5d2939697",
          "message": "Update readme",
          "timestamp": "2026-07-19T19:20:50+02:00",
          "tree_id": "f8d159ef0ce083efcd195667703d84657e75b2b8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ea9c01a292ecd12180f6620dbe3c25f5d2939697"
        },
        "date": 1784482536803,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6b89f16c814b6812314379b48c51cb5914039e77",
          "message": "Track Date::use()",
          "timestamp": "2026-07-19T20:19:23+02:00",
          "tree_id": "0b928ff05bfdcc5b343760022d842da94dae8327",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6b89f16c814b6812314379b48c51cb5914039e77"
        },
        "date": 1784486059813,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fab76b0945ce8097fc584fa974d5796b8ce07728",
          "message": "Expand on Laravel support",
          "timestamp": "2026-07-19T23:33:52+02:00",
          "tree_id": "b5d6036de05eba48781039ea91b2d802ea2df895",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fab76b0945ce8097fc584fa974d5796b8ce07728"
        },
        "date": 1784497718235,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "77fa16144e4753a158930d314d2734e818b19d02",
          "message": "Update release.yml",
          "timestamp": "2026-07-20T00:10:03+02:00",
          "tree_id": "e31d30ff0808dc8220ac2598b888f0710e804cc2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77fa16144e4753a158930d314d2734e818b19d02"
        },
        "date": 1784499872060,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a1053bce4a9df104a03f814bda4a8f3d70772f31",
          "message": "Bump version to 0.9.0",
          "timestamp": "2026-07-20T00:18:19+02:00",
          "tree_id": "c6212e5fd106708d7259c0b1a72f872c095e59d9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a1053bce4a9df104a03f814bda4a8f3d70772f31"
        },
        "date": 1784500399563,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "3911f1bb926d71f5e84c1fce278fe17d9ab79a29",
          "message": "add examples",
          "timestamp": "2026-07-19T20:04:20-05:00",
          "tree_id": "c5c4db7d16156a23ddeb2f486c900bb67ab4a1ac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3911f1bb926d71f5e84c1fce278fe17d9ab79a29"
        },
        "date": 1784510359418,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1dbbcdd4fd5317c322a67fa99ab600df57d2aaa5",
          "message": "feat: discover package resources from service providers\n\nScan service providers for mergeConfigFrom(), loadViewsFrom(),\nloadTranslationsFrom(), loadJsonTranslationsFrom(), and\nloadRoutesFrom() calls. Resolve __DIR__-relative paths to the\nactual package files on disk and feed the results into the\nexisting string key infrastructure.\n\nThis enables completion, go-to-definition, and hover for config\nkeys, view templates, translation keys, and named routes that\nare registered by installed packages rather than defined in the\napp itself. For example, config(\"horizon.environments\") now\ncompletes and jumps to the key in vendor/laravel/horizon/config/\nhorizon.php, and view(\"horizon::layout\") resolves to the\npackage view directory.\n\nThe scanner reuses the same provider list and one-level-deep\nhelper class traversal already used by macro discovery.\nDiscovered resources are cached on Backend and invalidate the\nstring key caches when populated.\n\nKey changes:\n- New provider_resources.rs with ProviderResource,\n  ProviderResources, and extract_provider_resources()\n- extract_dir_concat_path() moved from route_names.rs to\n  helpers.rs for shared use\n- build_provider_resources() in server.rs walks providers\n  and their referenced classes\n- enumerate_all_{config_keys,view_names,trans_keys}() and\n  enumerate_all_route_names() extended to include package\n  resources\n- resolve_{config_key,view,trans,route}_definitions() extended\n  to handle namespaced package keys (ns::key, ns.key)",
          "timestamp": "2026-07-20T06:02:59+02:00",
          "tree_id": "e4d48f830e2bb453f3ff86e92ed173e79d4a847c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1dbbcdd4fd5317c322a67fa99ab600df57d2aaa5"
        },
        "date": 1784521050362,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "aea2d5e3853750eb885ad987d974a266a6571eff",
          "message": "Add Claude Code setup instructions",
          "timestamp": "2026-07-20T06:36:11+02:00",
          "tree_id": "a83edcab91f8d5124bce3f510b0320f89f911860",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/aea2d5e3853750eb885ad987d974a266a6571eff"
        },
        "date": 1784523079493,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b55fe12e7448e60b16374bc4775177470d62fb68",
          "message": "Fix blade parsing errors",
          "timestamp": "2026-07-20T07:51:04+02:00",
          "tree_id": "bd030868bc659579d0b0a4b8ce220c4abe5793d3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b55fe12e7448e60b16374bc4775177470d62fb68"
        },
        "date": 1784527538422,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 42.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "lj.moritorii@web.de",
            "name": "Moritz Wirger",
            "username": "enwi"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d1fbea4b1886a4bcac94726afa30e825e6cf0f8d",
          "message": "Bump mago to 1.43.0",
          "timestamp": "2026-07-20T08:14:32+02:00",
          "tree_id": "820c6bfce38113ae5d4172acba662ed31f36b041",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d1fbea4b1886a4bcac94726afa30e825e6cf0f8d"
        },
        "date": 1784528949359,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "10b9d2e062834abef4f790249bd5178e5aaab874",
          "message": "Add task for more Mago migration",
          "timestamp": "2026-07-20T08:24:36+02:00",
          "tree_id": "e037a2afe8ee007717a7ab3dced5669829665812",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/10b9d2e062834abef4f790249bd5178e5aaab874"
        },
        "date": 1784529543837,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "31686a75cb2d6519f61320678c9563516b0ef647",
          "message": "feat: propagate by-reference closure capture types\n\nTrack assignments made inside closures that capture variables by reference\nwhen the closure is passed to an immediately-invoked callable. This updates\nthe outer variable scope so diagnostics do not keep the pre-call type, such\nas null, after the closure assigns a concrete value.\n\nFollow PHPStan's callable invocation defaults: standalone function callable\nparameters are treated as immediately invoked unless marked with\n@param-later-invoked-callable, while method callable parameters are treated\nas later invoked unless marked with @param-immediately-invoked-callable.\n\nAlso process assignments embedded in return expressions so patterns like\nreturn $foo = 1; update scope the same way as a standalone $foo = 1;\nstatement.\n\nFixes #246.",
          "timestamp": "2026-07-20T10:26:15-05:00",
          "tree_id": "b8ce1fdf0c0d6a08a21525953327c5a2e7f03c06",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/31686a75cb2d6519f61320678c9563516b0ef647"
        },
        "date": 1784562058766,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 43.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2128552b573018555d67a5b2270c00eea20ef5ff",
          "message": "Post-rebase cleanup for full workspace reference index (#186)\n\nAdds the changelog entries this PR was missing, removes the now-shipped\nX4/X8 backlog items and rewords their cross-references in other todo\ndocs, simplifies a redundant branch in find_implementors, and fixes a\nclippy warning left over from the rebase merge.\n\nContributed by @sidux in #186.",
          "timestamp": "2026-07-20T21:28:07+02:00",
          "tree_id": "e00b548501f8281f3e12dbbf5455738243e9a3ba",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2128552b573018555d67a5b2270c00eea20ef5ff"
        },
        "date": 1784576508111,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 51.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6e301787ddb81763d640577795e68a3bfba0eb74",
          "message": "Fix namespaced propagate by-reference closure capture types",
          "timestamp": "2026-07-20T21:43:03+02:00",
          "tree_id": "c8a95a9a1e4c36be00a7119f911485fe75f71374",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6e301787ddb81763d640577795e68a3bfba0eb74"
        },
        "date": 1784577388678,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7786f56db7b834bedfe6358bb924cee240447942",
          "message": "Better intergrate full indexing",
          "timestamp": "2026-07-20T22:22:35+02:00",
          "tree_id": "db37fc44eac86d773fab08e6071f170e6c072d13",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7786f56db7b834bedfe6358bb924cee240447942"
        },
        "date": 1784579795595,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 51.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4b0efe437a4fc48ee2a95e064cc70166ec6e96b5",
          "message": "Implement granular progress indication",
          "timestamp": "2026-07-20T23:11:17+02:00",
          "tree_id": "00d502bb512c1463d637ab1d674b47900a401ab4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4b0efe437a4fc48ee2a95e064cc70166ec6e96b5"
        },
        "date": 1784582740235,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6ce825cc850566c3ee2638e1d714e932a1c170ba",
          "message": "Add workspace diagnostics",
          "timestamp": "2026-07-21T00:13:29+02:00",
          "tree_id": "b1e7d4f912a745fc7190561c27cac90412ade1a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6ce825cc850566c3ee2638e1d714e932a1c170ba"
        },
        "date": 1784586472664,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "f93c1e22e47f79a66de41d764c1094974bbd5fbb",
          "message": "fix(laravel): resolve facade macro callback this\n\nResolve Laravel macro callback $this through the facade accessor so callbacks\nregistered on facades use the concrete container-bound class instead of the\nsurrounding provider.\n\nCloses #250",
          "timestamp": "2026-07-20T17:47:37-05:00",
          "tree_id": "2f304c36092cf4eb078bca6ffac8bfe0500fe4fe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f93c1e22e47f79a66de41d764c1094974bbd5fbb"
        },
        "date": 1784588543217,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "6e7bf1857a2ec311243b08bf8e72ae41dfc560bf",
          "message": "fix: toml schema syntax",
          "timestamp": "2026-07-20T17:58:12-05:00",
          "tree_id": "6bbeceadcb29441e22e72578d334fb264825969b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6e7bf1857a2ec311243b08bf8e72ae41dfc560bf"
        },
        "date": 1784589179185,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3c93b915af98ce47ebe0f71ceff77c19af2c3e6a",
          "message": "Remove bespoke Zed extension",
          "timestamp": "2026-07-21T01:29:53+02:00",
          "tree_id": "e798fce3fe976b097dc29b18e36a9a18d7286c80",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3c93b915af98ce47ebe0f71ceff77c19af2c3e6a"
        },
        "date": 1784591046569,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 51.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4874c7e545b807dbfa75106980e26080870130a2",
          "message": "fix: retain overlapping external diagnostics (#245)\n\nCo-authored-by: Anders Jenbo <anders@jenbo.dk>",
          "timestamp": "2026-07-21T01:57:03+02:00",
          "tree_id": "fce5fdff589c6d200bc87f9b1b110d45e90af5e1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4874c7e545b807dbfa75106980e26080870130a2"
        },
        "date": 1784592661257,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "c832773e19d0a6d159079e8af348a5432ff486fa",
          "message": "fix: test",
          "timestamp": "2026-07-20T19:05:57-05:00",
          "tree_id": "3ab5acc9966d711f265b71370f33ef060ea0fa3e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c832773e19d0a6d159079e8af348a5432ff486fa"
        },
        "date": 1784593244174,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "ed2cb483deb22c9a8751339a9a0974be7df1df55",
          "message": "feat(schema): add diagnostics.ignore rules to config JSON schema\n\nAdd the [[diagnostics.ignore]] array to config-schema.json to match\nthe new ignore rules added in e8fe8a8. Each rule supports optional\nmessage (regex), path (glob), and identifier (diagnostic code)\nconstraints for suppressing matching diagnostics project-wide.",
          "timestamp": "2026-07-20T19:09:06-05:00",
          "tree_id": "1917eb01c3048f4de5f6c6b7ebfb041331c27e48",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ed2cb483deb22c9a8751339a9a0974be7df1df55"
        },
        "date": 1784593423203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 108.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "87ee14075c11fffdfbdb3b6fd92a7b9c0bf4fa18",
          "message": "feat: infer Laravel model properties from migrations\n\nParse Laravel migration files using Mago CST to infer\nEloquent model database columns. Migrations are discovered\nfrom non-vendor database/migrations directories (including\nnested modules), applied in global basename order, and\nlayered on top of schema dumps.\n\nFeatures:\n- Named and anonymous migration class support\n- $connection property and Schema::connection() routing\n- Blueprint column types, nullable, virtualAs/storedAs\n- Blueprint::after() nested closure scanning\n- Blueprint macro expansion via existing macro scanner\n- Incremental rebuild: editing one migration re-reads only\n  that file and replays the cached plan over base schema\n- Non-recursive directory scanning (skips archive subdirs)\n- Config: [laravel.migrations] enabled/paths in .phpantom.toml\n- JSON schema for editor TOML completion",
          "timestamp": "2026-07-20T20:07:33-05:00",
          "tree_id": "d3b86bad2b8cec86f714378bb130cb1492e040d9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/87ee14075c11fffdfbdb3b6fd92a7b9c0bf4fa18"
        },
        "date": 1784596911641,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 112,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "384af6b699e9ea27428bb3686a398e335c98ec8c",
          "message": "Break up php_type",
          "timestamp": "2026-07-21T05:08:26+02:00",
          "tree_id": "a68a1c9b74139c871b22222887b5158adcafb60c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/384af6b699e9ea27428bb3686a398e335c98ec8c"
        },
        "date": 1784604256909,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 57.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c59cd96df1422918220e3cd51393c0bd050d2d44",
          "message": "update dependencies",
          "timestamp": "2026-07-21T07:55:53+02:00",
          "tree_id": "d0c29f06e7f04ca5defeb3d0c55ddaeca05a16ef",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c59cd96df1422918220e3cd51393c0bd050d2d44"
        },
        "date": 1784614310692,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bbb0b10969e235162777c784950fa9ae469fe5c7",
          "message": "Reduce number of redundant walkers",
          "timestamp": "2026-07-21T09:48:21+02:00",
          "tree_id": "80b7eefa5ffc218c3847bf68f4b92cc057a82367",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bbb0b10969e235162777c784950fa9ae469fe5c7"
        },
        "date": 1784621049946,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3321b718036ac8a4ebec741d02adafe8cd844880",
          "message": "Workaround for https://github.com/phpstan/phpstan/issues/14982",
          "timestamp": "2026-07-21T14:27:14+02:00",
          "tree_id": "d5355a04940dbf0f5c10d9314e08f126ddd0bb5d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3321b718036ac8a4ebec741d02adafe8cd844880"
        },
        "date": 1784637746412,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "882387d238367e8d3a5ed9b512728b290b2d2ce6",
          "message": "Read view folder from config",
          "timestamp": "2026-07-21T14:48:32+02:00",
          "tree_id": "88eff1904128e558a9f30b6bc2e9afeda54b2891",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/882387d238367e8d3a5ed9b512728b290b2d2ce6"
        },
        "date": 1784639073742,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ba164fb747d29358bf1b3a1287763a95301c1d2a",
          "message": "Routes registered from a service provider are recognized",
          "timestamp": "2026-07-21T15:09:59+02:00",
          "tree_id": "c5d2d7444746c77847db43429fc2f23b99702e0b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ba164fb747d29358bf1b3a1287763a95301c1d2a"
        },
        "date": 1784640413478,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 58.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "50b1148c094a964953972abf3270af97da64db42",
          "message": "Fix Blade parsing issues",
          "timestamp": "2026-07-21T16:17:55+02:00",
          "tree_id": "ff7a6c3e27df21bc1fc3f02ee235c5e94f079bd6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/50b1148c094a964953972abf3270af97da64db42"
        },
        "date": 1784644414390,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "10ccb3942bf66540b36502589cf832f43a680712",
          "message": "Add support for more Blade directives",
          "timestamp": "2026-07-21T17:13:25+02:00",
          "tree_id": "0df520416c21fe2310698b5170ea882591d9e6b9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/10ccb3942bf66540b36502589cf832f43a680712"
        },
        "date": 1784647744243,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4a712b29ac7238734d86cd4d4dc5fa356ad7e5b7",
          "message": "Blade component bound attributes are analysed as PHP",
          "timestamp": "2026-07-22T00:43:54+02:00",
          "tree_id": "50b64e6e05514bf934bbabe49bea17db3132e9fd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4a712b29ac7238734d86cd4d4dc5fa356ad7e5b7"
        },
        "date": 1784674724057,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1daceb2fed7f520104efaaf07effc8a348b8be87",
          "message": "Fix @use and @inject handeling in Blade fiels",
          "timestamp": "2026-07-22T01:01:25+02:00",
          "tree_id": "154842728eaad2484ce4a3da3f43a73112f00bf7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1daceb2fed7f520104efaaf07effc8a348b8be87"
        },
        "date": 1784675852165,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 113.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f7edebdef4c51e438c167d8a054c571ba1daa29c",
          "message": "Eloquent models always expose their primary key",
          "timestamp": "2026-07-22T04:38:22+02:00",
          "tree_id": "0dc40e9ef3a3aced0a6ee1b7f08556dd6efe1afa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f7edebdef4c51e438c167d8a054c571ba1daa29c"
        },
        "date": 1784688850412,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1a7814a40ba43f50550041c2c55f9a2a5f17e9a2",
          "message": "Go-to-implementation and type hierarchy return the same results every\ntime",
          "timestamp": "2026-07-22T05:47:39+02:00",
          "tree_id": "0c49e29b90ff440d31d29a938acde69c81c1ef00",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1a7814a40ba43f50550041c2c55f9a2a5f17e9a2"
        },
        "date": 1784693006679,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 112.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "63d32f9d13f15c5e07967b23244097884b1aff40",
          "message": "Editing a file while the workspace is indexing no longer shows stale\nresults",
          "timestamp": "2026-07-22T05:56:27+02:00",
          "tree_id": "515d7df0833a0122b81cae0318ac79a7a959e3c2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/63d32f9d13f15c5e07967b23244097884b1aff40"
        },
        "date": 1784693528366,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 56,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 112.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "b877d962045be937185fe20dccf76ed7ec9c93e3",
          "message": "Recognize Laravel `Macroable::mixin()` registrations (#256)",
          "timestamp": "2026-07-22T06:15:34+02:00",
          "tree_id": "26c1083e70c256249d58b8c346e514b792422999",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b877d962045be937185fe20dccf76ed7ec9c93e3"
        },
        "date": 1784694677636,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 112.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ef0a97048c29f3eaa2ea6e09e6c88d0b411cd514",
          "message": "Split up scope collector",
          "timestamp": "2026-07-22T06:54:16+02:00",
          "tree_id": "db89a798523f5f185632b455cb3386d5acc87633",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ef0a97048c29f3eaa2ea6e09e6c88d0b411cd514"
        },
        "date": 1784697011667,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 57.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "96efc36aecd0315d7d29b631a8801a20b4123db6",
          "message": "Fix array shape parsing issue",
          "timestamp": "2026-07-22T07:04:11+02:00",
          "tree_id": "7e439ebe6628872bd053c41d56a4b6724ddc0d8e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/96efc36aecd0315d7d29b631a8801a20b4123db6"
        },
        "date": 1784697521175,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 56,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "83f2e23c2979d18647078b2d99ea4c5f58fa6e03",
          "message": "Split forward_walk.rs",
          "timestamp": "2026-07-22T07:21:10+02:00",
          "tree_id": "5ff1e4b412a2419996e36b94557424936a5aa54d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/83f2e23c2979d18647078b2d99ea4c5f58fa6e03"
        },
        "date": 1784698643371,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "kirnevartem30@gmail.com",
            "name": "Arti",
            "username": "AbyssWaIker"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "ccbc866ad94d98e5e6b64d0ebdf7ee742765918b",
          "message": "Fix setup instruction for officiail php extension",
          "timestamp": "2026-07-22T08:05:57-05:00",
          "tree_id": "faf77704763d6107832e962b2d92694a03345644",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ccbc866ad94d98e5e6b64d0ebdf7ee742765918b"
        },
        "date": 1784726443785,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 109.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "82df3a767bbb1d9b7cf041b0236c2bfc843c883f",
          "message": "Deduplicate byte scanners",
          "timestamp": "2026-07-22T15:22:28+02:00",
          "tree_id": "128d088b4313b96d05afc467d16d2341cb623773",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/82df3a767bbb1d9b7cf041b0236c2bfc843c883f"
        },
        "date": 1784727432031,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "abeccab64ab348e801f33b04a7ce61eaa2070edc",
          "message": "Fix overflow in parameter resolve",
          "timestamp": "2026-07-22T15:32:24+02:00",
          "tree_id": "6f4ba8c71c6797cedbf9e212c35687d8cce93e1b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/abeccab64ab348e801f33b04a7ce61eaa2070edc"
        },
        "date": 1784728091407,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "26cfb2f9980879f786d3e1518f3271749afbde27",
          "message": " Factory has*/for* relationship methods (L6) (#260)",
          "timestamp": "2026-07-22T15:52:30+02:00",
          "tree_id": "69cee7c9855d63bcc06777050335c5f2b942f65a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/26cfb2f9980879f786d3e1518f3271749afbde27"
        },
        "date": 1784729295164,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 52.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "87ef7827ea8eab3ff1158bb46c24a0403ddcfb0f",
          "message": "feat: support Carbon trait-based mixin() registrations\n\nCarbon supports mixing in traits via `mixin(Trait::class)` since\nv2.23.0, where each public method of the trait becomes a method on\nthe target directly using its own signature. This is distinct from\nLaravel class-based mixins, where each mixin method is a factory that\nreturns the closure to register.\n\nThe mixin scanner now handles `Statement::Trait` in addition to\n`Statement::Class` when synthesizing macros from a mixin source file.\nClass mixins keep the existing closure-factory behavior. Trait mixins\nalways use the trait method signature directly, including methods that\nreturn `Closure`, because Carbon mixes the trait implementation into\nthe target instead of invoking the method as a closure factory.\n\nThis also confirms that Carbon `macro()` calls work through the same\ngeneric `X::macro(name, closure)` pipeline as Laravel Macroable.\n\nAdds unit tests for trait direct methods, Closure-returning trait\nmethods, method filtering, FQN matching, and Carbon macro/mixin\nextraction, plus integration tests for completion and diagnostics.\nAdds Carbon mixin/macro examples to the Laravel example project.",
          "timestamp": "2026-07-22T08:55:54-05:00",
          "tree_id": "f3530c0765d2d4dc072efd9b9533ddeec8c79ab7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/87ef7827ea8eab3ff1158bb46c24a0403ddcfb0f"
        },
        "date": 1784729523232,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 107.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f541c1b57e8dce6f5e208245e582260986ef1a6d",
          "message": "Fix a bit of migration parsing",
          "timestamp": "2026-07-22T16:10:04+02:00",
          "tree_id": "cb3d0c07adf5fc93a9a699ad8039843f23e3e858",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f541c1b57e8dce6f5e208245e582260986ef1a6d"
        },
        "date": 1784730335151,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 53.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "c0ffa26d147176b5e58d582042e6f25bbd83b4e8",
          "message": "feat: support phpstan require-implements trait bounds\n\nTraits annotated with `@phpstan-require-implements InterfaceName`\nnow resolve `$this` against the required interface while editing the\ntrait itself. This mirrors the existing `@phpstan-require-extends`\nsupport for required base classes and makes required interface\nmethods available in completion, hover, and member resolution.\n\n`require_implements` is stored as a list on `ClassInfo` because a\nclass can implement multiple interfaces and traits may declare multiple\nrequire-implements tags. The parser extracts phpstan, psalm, and bare\nrequire-implements variants, and post-processing resolves them to fully\nqualified names alongside other class-like references.",
          "timestamp": "2026-07-22T09:14:33-05:00",
          "tree_id": "ae4f4a0590f7d3c8cbc1a7a73a504b19198f621c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c0ffa26d147176b5e58d582042e6f25bbd83b4e8"
        },
        "date": 1784730612590,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bdc6686075677a117a3f2dad4a9ad53e1d1479c7",
          "message": "Extract more tests",
          "timestamp": "2026-07-22T16:27:02+02:00",
          "tree_id": "32ed4e57a01a2ea275d8f630e1905e0090598edc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bdc6686075677a117a3f2dad4a9ad53e1d1479c7"
        },
        "date": 1784731375089,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 112,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c62ac393ff97296ba9ff1c789ae4ddd1484f5b67",
          "message": "extract dock block info for @phpstan-require-implements",
          "timestamp": "2026-07-22T16:40:09+02:00",
          "tree_id": "7ce902777246d5199bce62efe207bcf9eeec93a0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c62ac393ff97296ba9ff1c789ae4ddd1484f5b67"
        },
        "date": 1784732121507,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 54,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 110.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6e1dec2db0254bd6876a778171e4ae6c63d0cf9d",
          "message": "linux -gnu are now actually -musl builds",
          "timestamp": "2026-07-22T17:39:25+02:00",
          "tree_id": "7238248396a9225c0d8d9de5611e3c7e9ddb4776",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6e1dec2db0254bd6876a778171e4ae6c63d0cf9d"
        },
        "date": 1784735743848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 114,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 163.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8ed87e8b93003fef93dffc4deb922c0ad4dc52da",
          "message": "More refactoring",
          "timestamp": "2026-07-22T18:34:20+02:00",
          "tree_id": "9f3abd17ebe55e109182d5a9010c5196a0e7add9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ed87e8b93003fef93dffc4deb922c0ad4dc52da"
        },
        "date": 1784739025963,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 102.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 174.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "0d799b69c5d4671ddd5c11dde227b865141106c6",
          "message": "feat: resolve Larastan model-property<Model> type against model properties\n\nAdd type validation, completion, and hover for Larastan's\nmodel-property<Model> pseudo-type.\n\nDiagnostics: string literals passed where model-property<Model> is\nexpected are checked against the model's known properties. Invalid\nproperty names produce a type mismatch diagnostic; non-literal\nstrings are accepted conservatively. Array arguments are also\nvalidated: string literals inside array or list arguments whose\nparameter type wraps model-property<Model> in a generic position\nare checked against the model's properties.\n\nCompletion: typing inside a string argument whose parameter is\ntyped as model-property<Model> (including array/list wrappers)\nsuggests the model's property names with partial filtering, using\nthe same fully-resolved property list as regular member completion.\n\nHover: hovering over a string literal inside a model-property<Model>\nparameter shows the same property info as hovering over the\ncorresponding $model->property access.\n\nThe type parser now handles hyphenated pseudo-type names that\nmago_type_syntax cannot parse. Known hyphenated names are replaced\nwith underscore placeholders before parsing and restored in the\nresult, so types like array<model-property<T>, mixed> parse\ncorrectly instead of falling back to Raw.\n\nThe completion infrastructure was refactored to extract a shared\ndetect_string_call_context() from the existing Eloquent string\ndetection, reused by completion, hover, and diagnostics.\n\nCloses #33",
          "timestamp": "2026-07-22T11:48:29-05:00",
          "tree_id": "427318e496114e8632b0ddd109c847b9c9356976",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0d799b69c5d4671ddd5c11dde227b865141106c6"
        },
        "date": 1784739858814,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 104,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 159.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "3d6653b16e951ca8af778d86da555479860b0fa3",
          "message": "fix: use FQN for Eloquent TModel substitutions\n\nBuilder forwarding and where{Property}() virtual methods were\nsubstituting TModel with the model short name. At call sites that\nimport the model under an alias alongside another class with the same\nshort name, that bare name resolved through the wrong use-map entry.\n\nUse class.fqn() so return types stay independent of call-site imports.\nFixes #258.",
          "timestamp": "2026-07-22T12:00:42-05:00",
          "tree_id": "4212c3f5a4d9f50a24c8e68b19cbfd85a73fa883",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3d6653b16e951ca8af778d86da555479860b0fa3"
        },
        "date": 1784740616066,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 106.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 158,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "ead56b5806535bf1d3b17d69497b46e227e5393e",
          "message": "fix: preserve model type through ForwardsCalls mixin chains\n\nQuery-only fluents like lockForUpdate() are mixed onto Eloquent\nBuilder and Relation via @mixin. Call-site $this expansion was\nstripping Builder<TModel> generics and matching the short name\n\"Builder\", so firstOrFail() resolved as Model|stdClass.\n\nDetect ForwardsCalls and apply decorated-forward return semantics\nwhen merging mixin methods (self/mixin-class returns stay the\nforwarder's $this; other returns pass through). Preserve the\nreceiver's full generic type when expanding $this at call sites,\nmatching owners by FQN. Covers Builder and Relation chains.\n\nFixes #257.",
          "timestamp": "2026-07-22T12:59:20-05:00",
          "tree_id": "9f70866a051e9aa4d0d0161fcb1d221332b14e52",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ead56b5806535bf1d3b17d69497b46e227e5393e"
        },
        "date": 1784744130458,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 106.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 165.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "5858e963a2d3f33a1e24867d3a38645b561496fe",
          "message": "fix: treat by-ref instance method args as defined\n\nUndefined-variable analysis already marked by-ref out-params as\nwrites for free functions, static methods, constructors, and\n$this->method(). Instance calls on new ClassName() still walked\nargs as reads, so new A()->dosmth($y, $foo) falsely flagged $foo.\n\nResolve the receiver class from the AST when it is obvious ($this,\nnew Class, (new Class), new self) and mark by-ref positions as\nReadWrite, same as preg_match out-params.\n\nFixes #253.",
          "timestamp": "2026-07-22T13:09:56-05:00",
          "tree_id": "4e3b269185df14950c42a373c185ac55b52e0659",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5858e963a2d3f33a1e24867d3a38645b561496fe"
        },
        "date": 1784744716649,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 97.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 162.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "dea4516d46a70b57d0c2b60e0935f5a9986b753f",
          "message": "fix: seed mixed for untyped foreach loop values\n\nWhen the iterable has no known element type (e.g. an untyped\nparameter), foreach left $value with empty types. Assignments like\n$x = $value after $x = null were then no-ops, so is_null early-return\nnarrowing still saw pure null and flagged later uses.\n\nSeed mixed for undetermined foreach values, matching bare array\nhandling, so post-loop merge and null guards work.\n\nFixes #252.",
          "timestamp": "2026-07-22T13:25:34-05:00",
          "tree_id": "bbb939cebf2b802748992784b0af721c8d63ccf6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dea4516d46a70b57d0c2b60e0935f5a9986b753f"
        },
        "date": 1784745688003,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 106.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 164.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b275b12cdb701881477071e781b8c5f1eaa66ad6",
          "message": "Fix musl memory regression",
          "timestamp": "2026-07-22T20:35:00+02:00",
          "tree_id": "5550f4cfa7d36aeda67d340cce2e1f9424f4092f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b275b12cdb701881477071e781b8c5f1eaa66ad6"
        },
        "date": 1784746243876,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 91.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 151.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "608153125d773873064e5907d38b63f8848903b0",
          "message": "fix: suppress class completion in method/const name positions\n\nTyping a member name after `function` or `const` (e.g. protected\nfunction getC) was still falling through to bare class/constant/\nfunction completion with ClassNameContext::Any, so project classes\nmatching the partial flooded the list.\n\nDetect function/const name positions and skip that strategy.",
          "timestamp": "2026-07-22T14:08:13-05:00",
          "tree_id": "476cf03f35d06abc21f4213dc65d2cb75003399c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/608153125d773873064e5907d38b63f8848903b0"
        },
        "date": 1784748258187,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 91.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 147.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "cfcb06a1a7759f7ffa2ce1d7b9d1ff3560703fcf",
          "message": "fix: allow use const/function completion after name-position guard\n\nThe member-name suppression for `const`/`function` also matched\n`use const FOO` and `use function bar`, which still need symbol\ncompletion. Skip the guard when those keywords are preceded by `use`.",
          "timestamp": "2026-07-22T14:20:11-05:00",
          "tree_id": "6feda5991491689ecd97981f35267f363cad872b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cfcb06a1a7759f7ffa2ce1d7b9d1ff3560703fcf"
        },
        "date": 1784748993106,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 87.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 145.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3664dc0c86ec02eafcbfb022995951da1364e7ea",
          "message": "Only use mimalloc on musl",
          "timestamp": "2026-07-22T21:25:45+02:00",
          "tree_id": "1d509f879f20cde560bc951f43b610a1c30e30cc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3664dc0c86ec02eafcbfb022995951da1364e7ea"
        },
        "date": 1784749440646,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 55.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 111.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "abcf567eef162b5fc226a09d081270be8e733249",
          "message": "Run all tests on musl + mimalloc",
          "timestamp": "2026-07-22T22:35:28+02:00",
          "tree_id": "df9a44cfac02aa7b6e421f511a6a26d034e30a4a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/abcf567eef162b5fc226a09d081270be8e733249"
        },
        "date": 1784753525735,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 91.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ef240420d52cc5fda9adcb80557bac3215e11f4d",
          "message": "Split laravel module",
          "timestamp": "2026-07-22T22:55:28+02:00",
          "tree_id": "dc54becb6f2ccc7db4453b45479af0ea25b19845",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ef240420d52cc5fda9adcb80557bac3215e11f4d"
        },
        "date": 1784754969650,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "30da66a590818def48116b6d14176ad0d895da9f",
          "message": "Split more large files",
          "timestamp": "2026-07-22T23:31:20+02:00",
          "tree_id": "9b0ae86df41df1b6669c2f85b24d0abdc5fd9d6b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/30da66a590818def48116b6d14176ad0d895da9f"
        },
        "date": 1784756992501,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4e436ff9e448ba15eb385a7b86ba0f3df3dbd221",
          "message": "Move mimalloc to lib",
          "timestamp": "2026-07-22T23:52:31+02:00",
          "tree_id": "b930aae7ca9a1ce71a0c426d084f4c1dc9075dc8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4e436ff9e448ba15eb385a7b86ba0f3df3dbd221"
        },
        "date": 1784758061787,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "25953d7b60d898f989d0af699a44a6d46dc80b58",
          "message": "Some more refactoring",
          "timestamp": "2026-07-23T01:07:16+02:00",
          "tree_id": "ac15ce04ed6da2394caac53b9131df910b1b84dd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/25953d7b60d898f989d0af699a44a6d46dc80b58"
        },
        "date": 1784762579896,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "66f470d5d0d74fb78e12b8a88efe9c8150f3f110",
          "message": "Split the other resolution-pipeline giants",
          "timestamp": "2026-07-23T02:14:29+02:00",
          "tree_id": "02dfb2fa690b6da5856d5ef8727fbb13e872f5bf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/66f470d5d0d74fb78e12b8a88efe9c8150f3f110"
        },
        "date": 1784766619830,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a1225acb3e85b630fef93dff9236fb86c7ba9d7a",
          "message": "Split extract function into multiple files",
          "timestamp": "2026-07-23T02:32:15+02:00",
          "tree_id": "4e02e828b7529a5af78161411d16d4de5bb36c7c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a1225acb3e85b630fef93dff9236fb86c7ba9d7a"
        },
        "date": 1784767692269,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "155db1db76fe19f66e250b972f33ffa93f6cf9ac",
          "message": "Split up narrowing",
          "timestamp": "2026-07-23T02:52:30+02:00",
          "tree_id": "8140edfd77b70013f6c87e2f7cda44ad1f417363",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/155db1db76fe19f66e250b972f33ffa93f6cf9ac"
        },
        "date": 1784768886746,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 98.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0e664c5511f9535734a6fd0f55c7e896b02d4ec9",
          "message": "Plit refactoring",
          "timestamp": "2026-07-23T03:26:02+02:00",
          "tree_id": "73661a839d7244a3eff9dda358920eab7646f1af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0e664c5511f9535734a6fd0f55c7e896b02d4ec9"
        },
        "date": 1784770918925,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8ebcc9c3e2b0b1e8f87f363d3f8d2ddeaa65630a",
          "message": "Split refactoring",
          "timestamp": "2026-07-23T03:27:38+02:00",
          "tree_id": "73661a839d7244a3eff9dda358920eab7646f1af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ebcc9c3e2b0b1e8f87f363d3f8d2ddeaa65630a"
        },
        "date": 1784770970640,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "496ead75220ee3a3db0c7e4aec49f5d11c48f71f",
          "message": "Split throw analysis",
          "timestamp": "2026-07-23T15:24:37+02:00",
          "tree_id": "b70e764b9863d0958afa0216bfd544f7c8b04be2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/496ead75220ee3a3db0c7e4aec49f5d11c48f71f"
        },
        "date": 1784813876214,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "4d34c85e0d56145ca2208fc5b5b78eb77e27954f",
          "message": "L7. `$pivot` property on BelongsToMany related models (#266)",
          "timestamp": "2026-07-23T15:43:20+02:00",
          "tree_id": "05bb6e9528760a8c5661277c04a3ac48480a53af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4d34c85e0d56145ca2208fc5b5b78eb77e27954f"
        },
        "date": 1784815095005,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fb4cb0b268ce2af35eee8f4cdd74e8bf29f110f1",
          "message": "Split class_completion",
          "timestamp": "2026-07-23T16:23:01+02:00",
          "tree_id": "8be3da1af2c625ee95a4200baa13b85f0a702af8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fb4cb0b268ce2af35eee8f4cdd74e8bf29f110f1"
        },
        "date": 1784817545786,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "4563f529cf272d51769a9b5841a2e1618b088460",
          "message": "feat: parent member override completion\n\nSuggest public/protected methods, properties, and constants from parent\nclasses and interfaces when typing a member name in a class body\n(protected function get, protected $attr, public const ST).\n\nMethod snippets insert a full native signature (with $ params escaped\nfor LSP snippets) and an empty body. On PHP 8.3+ (composer.json /\nconfig.platform.php), also insert #[\\Override] above the declaration.\nProperty/const inserts include parent default values when present.\n\nPrivate members and ones already defined on the class are omitted.\nClass-name completion remains suppressed at these name positions.\n\nCloses #267",
          "timestamp": "2026-07-23T09:44:37-05:00",
          "tree_id": "ae43e0ca465d1963d205a79a6b56a986db9ca5db",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4563f529cf272d51769a9b5841a2e1618b088460"
        },
        "date": 1784818827315,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "db0583c555de98c55e68f364d6dbbf9c03d866b2",
          "message": "fix: handle static inside class-string diagnostics\n\nRecognize self-references nested inside class-string and interface-string\ntypes when deciding whether argument diagnostics need late-static-binding\ncontext. This prevents sibling subclass ::class constants from being\nflagged against class-string<static> parameters.\n\nCloses #271",
          "timestamp": "2026-07-23T11:31:08-05:00",
          "tree_id": "36c2823cb1030393ca80b916c1d8b10bb2997564",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/db0583c555de98c55e68f364d6dbbf9c03d866b2"
        },
        "date": 1784825191569,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "f7030d4661707f75d3b4547f0dca534210d373e8",
          "message": "fix: resolve facade static calls through concrete targets\n\nResolve missing static methods on Laravel-style facades through the\nconcrete class returned by getFacadeAccessor before falling back to the\nfacade magic __callStatic return type. Also try facade @mixin targets so\nannotated facades keep their concrete method return types.\n\nCloses #270",
          "timestamp": "2026-07-23T11:58:11-05:00",
          "tree_id": "2c1064bc20eea5fa9d5c188e37d2f6102ccc5ce4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f7030d4661707f75d3b4547f0dca534210d373e8"
        },
        "date": 1784826853839,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "01cbcab5e7cd577ed4656295bc73172a9f9d1a3a",
          "message": "feat: support phpstan-ignore identifiers\n\nHighlight @phpstan-ignore tags and each listed PHPStan identifier in\nordinary comments and docblocks. Offer identifier completion from cached\nPHPStan diagnostics for the current file while avoiding per-code reason\ntext.",
          "timestamp": "2026-07-23T12:43:40-05:00",
          "tree_id": "f368fcdc3776971a745cbf873681c2b2ae7a8dd0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/01cbcab5e7cd577ed4656295bc73172a9f9d1a3a"
        },
        "date": 1784829568692,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 97.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4e08b0b724a5e6e0915ccd07bf850ad7b33865b3",
          "message": "Split up classmap_scanner",
          "timestamp": "2026-07-23T23:30:52+02:00",
          "tree_id": "e334e4cabb3fc16ffe8395a3a1e2e2a9b05649ee",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4e08b0b724a5e6e0915ccd07bf850ad7b33865b3"
        },
        "date": 1784843266745,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bf31804d71de863fb7a97d4631afbcd5dea49ddc",
          "message": "Split completion logic in to smaller files",
          "timestamp": "2026-07-24T00:05:51+02:00",
          "tree_id": "1d9db8442daee0bba056fe806b5222cb9cbb60ba",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bf31804d71de863fb7a97d4631afbcd5dea49ddc"
        },
        "date": 1784845308396,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 45.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "93ce064fd5e6c8d3a96b2e7be433efa76fe4411b",
          "message": "Split phpstan return type action",
          "timestamp": "2026-07-24T00:26:31+02:00",
          "tree_id": "bfa32ac30418d69a5190e3848dbf5179b090d1b6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/93ce064fd5e6c8d3a96b2e7be433efa76fe4411b"
        },
        "date": 1784846490825,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9948d28185b297c325d53e781a8010ff17295d64",
          "message": "Split up diagnostics",
          "timestamp": "2026-07-24T00:42:22+02:00",
          "tree_id": "c8a4a1d9c7fac63949c35a61ba27063ad1f0da35",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9948d28185b297c325d53e781a8010ff17295d64"
        },
        "date": 1784847367942,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b598d3ae19d9c13acd1680b19a998a24ab7573f8",
          "message": "Split up classes parser",
          "timestamp": "2026-07-24T01:40:29+02:00",
          "tree_id": "18b97bd93b82984c5e15907faef32364662a9876",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b598d3ae19d9c13acd1680b19a998a24ab7573f8"
        },
        "date": 1784850984260,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 97,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "117185e0e50a397b7d000cceec605548833553b7",
          "message": "Split up extraction.rs",
          "timestamp": "2026-07-24T01:57:47+02:00",
          "tree_id": "56b2a06cd3a894a1fc1db9c80efb31dbe0649cd5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/117185e0e50a397b7d000cceec605548833553b7"
        },
        "date": 1784852024262,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1d3d6de0d2163b305fd129f01d1240d7ff56f46c",
          "message": "Another chunk of refactoring",
          "timestamp": "2026-07-24T02:13:22+02:00",
          "tree_id": "3acae142cc998de3a5b54694f2f2347e939378a8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1d3d6de0d2163b305fd129f01d1240d7ff56f46c"
        },
        "date": 1784852960973,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 91.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9e22d06f2eed8caffe8df32cf056fbf16e529b93",
          "message": "Unify to_subject_text methods",
          "timestamp": "2026-07-24T02:37:30+02:00",
          "tree_id": "408f2eebddf6895dafb8ee7b3b12e3a077d41af4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9e22d06f2eed8caffe8df32cf056fbf16e529b93"
        },
        "date": 1784854418128,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "15a051458df9e5fefdd97455cd1412ce64d55b5c",
          "message": "Fix flaky resolve on shadowed classes",
          "timestamp": "2026-07-24T03:21:08+02:00",
          "tree_id": "a3d83a2de101a8d4bd10de08ee1208ee345080cd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/15a051458df9e5fefdd97455cd1412ce64d55b5c"
        },
        "date": 1784857002483,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "596a795f681080d78115936931c8cd8dc1b4bed1",
          "message": "Fix self / static from within macros",
          "timestamp": "2026-07-24T03:40:23+02:00",
          "tree_id": "215428b57090870d21ab0bf450541f57c0b173b3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/596a795f681080d78115936931c8cd8dc1b4bed1"
        },
        "date": 1784860514265,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "edda6f81f63cda081e9b3bfc1731d8bdb3caf1b7",
          "message": "Fix namespace context for resolve_function_name",
          "timestamp": "2026-07-24T04:26:59+02:00",
          "tree_id": "cc2b4a1f1712330bf0d854efffc1a744511c2e7a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/edda6f81f63cda081e9b3bfc1731d8bdb3caf1b7"
        },
        "date": 1784861025722,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "19ba7366b39f5f2440eeea8a99db6c6bbf33cf75",
          "message": "More refactoring",
          "timestamp": "2026-07-24T11:51:43+02:00",
          "tree_id": "f3ea57dc9a278c3ebcb9a4731ebd90d50c1c5a3c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/19ba7366b39f5f2440eeea8a99db6c6bbf33cf75"
        },
        "date": 1784887673188,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e04cf74a46e98825c61d43aabb38d402017bae58",
          "message": "Extracted shared function",
          "timestamp": "2026-07-24T12:15:45+02:00",
          "tree_id": "1cd7581419be4f6f6e1953ac70053f7c3296b89f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e04cf74a46e98825c61d43aabb38d402017bae58"
        },
        "date": 1784889196302,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "832a23efb268d90b1c9185678091440e15523c8e",
          "message": "`@template` bindings resolve correctly when a call uses named arguments",
          "timestamp": "2026-07-24T14:10:48+02:00",
          "tree_id": "8fc348f4b6660697cf96f870b6e9b9d567796932",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/832a23efb268d90b1c9185678091440e15523c8e"
        },
        "date": 1784895997592,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0016d249f01092dcf5febba4edbbba95286cb713",
          "message": "Another bucket of refactoring to compleation and diagnostics",
          "timestamp": "2026-07-24T15:50:48+02:00",
          "tree_id": "39a98d4e9f99f19eaa9381af562d64f3cac2b3eb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0016d249f01092dcf5febba4edbbba95286cb713"
        },
        "date": 1784902013779,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "de648397cfb9a91d3d44c99ffc148e3b99f8c608",
          "message": "docs: update init command docs, config reference, and add PR template\n\nThe `init` command creates a minimal .phpantom.toml with just a JSON\nschema directive, but the documentation promised \"all options documented\nand commented out.\" This corrects that mismatch and brings the config\ndocumentation up to date.\n\n- Update DEFAULT_CONFIG_CONTENT to include helpful comments pointing\n  users to the schema and configuration reference, rather than leaving\n  a bare schema line.\n- Rewrite the CLI.md init section to accurately describe the minimal\n  file that gets created and explain the schema-driven discoverability\n  approach.\n- Rewrite the SETUP.md Project Configuration section with complete\n  reference tables for every config section: php, diagnostics (including\n  ignore rules), indexing, formatting, phpstan, phpcs, mago, and laravel\n  (schema and migrations). The previous version only covered php,\n  diagnostics (partially), indexing, and formatting.\n- Add formatting.pint to config-schema.json, which was present in the\n  Rust config struct but missing from the schema.\n- Add a PR template with a contributor checklist (inspired by jj-vcs)\n  to remind contributors to update docs, schema, changelog, and tests.\n- Update README.md documentation link to mention configuration.\n\nCloses #275",
          "timestamp": "2026-07-24T09:49:32-05:00",
          "tree_id": "eb738eb9951a6fd5df616c278662d47360ad23eb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/de648397cfb9a91d3d44c99ffc148e3b99f8c608"
        },
        "date": 1784905556569,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cc20ae22db6666e61175041f1471f8108b4cd825",
          "message": "Clean up code actions",
          "timestamp": "2026-07-24T17:28:44+02:00",
          "tree_id": "e8c472ab57c72f0257e760e4031c97f03b774e53",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cc20ae22db6666e61175041f1471f8108b4cd825"
        },
        "date": 1784907890820,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 46.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "37d6f78099cce70fd946e5d87d3af6b54ba4e642",
          "message": "More refactoring",
          "timestamp": "2026-07-24T17:57:15+02:00",
          "tree_id": "36b2f6e6037f544591de26ae7b6077013f69e22e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/37d6f78099cce70fd946e5d87d3af6b54ba4e642"
        },
        "date": 1784909574979,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 19.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "0eda82fe9af9f114b3481c7f7f936091beef67ba",
          "message": "docs: add Zensical documentation site with versioned deployment\n\nSet up a documentation website using Zensical (the actively maintained\ndrop-in replacement for MkDocs, by the mkdocs-material team) with\nversioned docs deployed to GitHub Pages via squidfunk's mike fork.\n\n- Add zensical.toml with collapsible nav, light/dark toggle, search,\n  and version switching via mike.\n- Add pyproject.toml for uv-managed Python dependencies (zensical,\n  squidfunk/mike fork). Contributors preview docs with\n  `uv run zensical serve`.\n- Add .github/workflows/docs.yml with two deployment paths: prerelease\n  docs on push to main, versioned docs on GitHub release publication.\n  Skipped on forks.\n- Split SETUP.md into installation.md (tabbed install options),\n  editor-setup.md, and agent-setup.md for cleaner navigation.\n- Add docs/configuration.md as a dedicated config reference with\n  complete tables for all config sections.\n- Add docs/benchmarks.md with comparison table and links to live\n  latency and memory charts.\n- Add docs/index.md landing page with centered logo, key features,\n  and version jump links.\n- Rename project-specific docs to lowercase (cli.md, configuration.md).\n- Fix all 25 broken anchor links in roadmap docs.\n- Fix missing 0.9.0 link reference in CHANGELOG.md.\n- Add roadmap sub-pages and benchmarks to navigation.\n- Add formatting.pint to config-schema.json (was missing from schema).\n- Add .github/PULL_REQUEST_TEMPLATE.md with contributor checklist.\n- Update DEFAULT_CONFIG_CONTENT with helpful comments and docs link.\n- Simplify README.md documentation section to link to docs site.\n- Update docs/CONTRIBUTING.md with docs build instructions.\n- Add site/ and .venv/ to .gitignore.",
          "timestamp": "2026-07-24T13:07:18-05:00",
          "tree_id": "60b030de458fafe9137f232d8056cd73e2e7ae67",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0eda82fe9af9f114b3481c7f7f936091beef67ba"
        },
        "date": 1784917320281,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "eb6090df6ffbbffad98fa25b4f225e6d634e4d4b",
          "message": "docs: improve editor setup and installation pages\n\n- Reorder nav: Editor Setup before Manual Installation, since most\n  editors auto-install PHPantom.\n- Split editor setup into Automatic Installation (Zed, VS Code) and\n  Manual Installation sections so users can quickly find their editor.\n- Convert editor and agent setup pages from HTML <details> dropdowns\n  to proper markdown headers for better navigation and rendering.\n- Clean up PHPStorm instructions (remove redundant download step).\n- Restructure installation page with Latest Release (tabbed), Latest\n  Main (cargo install --git for testing fixes), and Build from Source.\n- Add cargo-binstall as an installation option.\n- Remove redundant Zed binary note (covered by section intro).\n- Remove navigation.expand so sidebar sections start collapsed.",
          "timestamp": "2026-07-24T13:50:42-05:00",
          "tree_id": "9582f4b8aec0f39b0cd340df4ed5ba4ab8c7c547",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eb6090df6ffbbffad98fa25b4f225e6d634e4d4b"
        },
        "date": 1784920034738,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "517879c7efbd2c50e3717a4a74ba12f36b640152",
          "message": "docs: center README badges and promote documentation link\n\nMove badges into the centered icon block so they render beneath the\nlogo instead of left-aligned. Add a documentation link above the\nfeature table so new visitors find the docs site immediately rather\nthan scrolling past the comparison matrix first.",
          "timestamp": "2026-07-24T14:13:52-05:00",
          "tree_id": "a77a00ee2f72509b36dce212cfca43624782e591",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/517879c7efbd2c50e3717a4a74ba12f36b640152"
        },
        "date": 1784921351935,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "ad2ec8a5532c6c29df3bb9bcd9fec519337f16ea",
          "message": "fix: go-to-implementation with same-named interface and class\n\nWhen an interface and its implementing class shared the same short\nname (e.g. App\\Contracts\\HttpClient and App\\Foo\\HttpClient),\ngo-to-implementation returned no results or pointed to the interface\nitself instead of the concrete class.\n\nTwo places in the implementation resolver compared by short name\ninstead of fully-qualified name, causing false matches when classes\nin different namespaces shared a name:\n\n- class_implements_or_extends excluded any candidate whose short name\n  matched the target, even when the FQNs differed.  Now compares by\n  FQN only.\n- locate_class_declaration looked up the class file by short name, so\n  it returned the first file containing that name rather than the\n  correct namespace.  Now builds the FQN from the class's namespace\n  before lookup.\n\nCloses #279",
          "timestamp": "2026-07-24T14:36:52-05:00",
          "tree_id": "fc1d5533683405dbb5414dba07dd5f744c7d2a9c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ad2ec8a5532c6c29df3bb9bcd9fec519337f16ea"
        },
        "date": 1784922800909,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "e7cbe8cd4571f3b45b0ea3ef5b893400dbb5c61c",
          "message": "feat: suggest trait member overrides in completion\n\nTyping a method or property name in a class body that uses a trait\nnow suggests the trait's public and protected members as candidates,\nmatching the existing override completion for parent classes and\ninterfaces.\n\nTrait method replacements intentionally omit #[\\Override] because\nPHP treats trait methods as copied into the class — adding the\nattribute on a trait-only method is a compile error. The completion\ndetail shows 'trait' instead of 'override' to reflect this.\n\nPreviously, classes that only used traits (no parent class, no\ninterfaces) received no override suggestions at all because the\nhandler bailed out early. That guard now also checks used_traits.\n\nCloses #276",
          "timestamp": "2026-07-24T15:20:26-05:00",
          "tree_id": "8e503d5809d7abca6cb5e5173f79199218c64a2c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e7cbe8cd4571f3b45b0ea3ef5b893400dbb5c61c"
        },
        "date": 1784925379379,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c1a74ffb1a796ee87541b8445e75af77cab7ef1f",
          "message": "Even more refactoring",
          "timestamp": "2026-07-25T01:27:56+02:00",
          "tree_id": "6173bfb8d1b7ec62d9515e428de30533df9221bc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c1a74ffb1a796ee87541b8445e75af77cab7ef1f"
        },
        "date": 1784936655702,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d8549e06432afe506fd066e6a842e42956cf6146",
          "message": "Update AGENTS.md",
          "timestamp": "2026-07-25T02:08:25+02:00",
          "tree_id": "74e2f479662173ed80d6364461bd5d21fdb0e7fe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d8549e06432afe506fd066e6a842e42956cf6146"
        },
        "date": 1784939031421,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2e1402b2d97f1156701ad977bbc40af961b2c326",
          "message": "Re organize backend",
          "timestamp": "2026-07-25T02:57:47+02:00",
          "tree_id": "41ad893f8815a0799ca16e5543b1c865fe3e32d3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2e1402b2d97f1156701ad977bbc40af961b2c326"
        },
        "date": 1784942023157,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 97.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "eba0193372c3ac554605b440c922cbe1545377d2",
          "message": "Move the type engine out of the compleation module",
          "timestamp": "2026-07-25T03:16:52+02:00",
          "tree_id": "89a7ef001fadce4e2086aa923bd133b1cb7a8574",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eba0193372c3ac554605b440c922cbe1545377d2"
        },
        "date": 1784943194995,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 98.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d4eac1843cdd82150954a9b406a2dbaf5290f25e",
          "message": "Reuse external proxy pattern",
          "timestamp": "2026-07-25T03:43:14+02:00",
          "tree_id": "fc6cdab7eee81c471bfe9efdadb7723f3d087104",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d4eac1843cdd82150954a9b406a2dbaf5290f25e"
        },
        "date": 1784944764451,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "445c65ac0569542f27b34e4a0fe6748cd1b1fde9",
          "message": "Finish `resolved_class_cache` generic-arg specialisation",
          "timestamp": "2026-07-25T03:53:35+02:00",
          "tree_id": "e9e54babde28fa236b5312682c79a6a343ed70c1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/445c65ac0569542f27b34e4a0fe6748cd1b1fde9"
        },
        "date": 1784945412120,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "19288497833e17c312aa45664858c99a9e3bc8c1",
          "message": "feat: add reference count inlay hints\n\nShow reference counts for classes, enums, interfaces, traits, methods,\nproperties, and constants as inlay hints instead of non-actionable code\nlenses. Interface and abstract class declarations also show implementation\ncounts. Code lenses remain reserved for actionable prototype navigation.\n\nThe hints omit private members, magic methods, and overridden members to\navoid noisy or misleading counts, while still showing zero-reference hints\nso users can distinguish unused symbols from missing data.\n\nGo-to-definition on overridden declarations now follows methods,\nproperties, and constants to the nearest parent, trait, or interface\nprototype where applicable.\n\nUse a single all-targets Clippy invocation in CI and let agent docs prefer\nclippy --fix so automated changes apply safe lint suggestions before\nformatting.\n\nCloses #140",
          "timestamp": "2026-07-24T21:00:33-05:00",
          "tree_id": "9efe14eaaf7e3ce16040f91a13756e266e3935c2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/19288497833e17c312aa45664858c99a9e3bc8c1"
        },
        "date": 1784945807718,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "8ae35c63d07ea9cefacccd14d5016c010cbb821c",
          "message": "fix: highlight PHP attributes as decorators\n\nAttribute usages need a semantic token distinct from normal class references so editor themes can color #[...] separately from classes used in types or expressions. Reuse the existing attribute class-reference context to emit the standard decorator token.\\n\\nCloses #117",
          "timestamp": "2026-07-24T21:22:18-05:00",
          "tree_id": "b698560e5e3f7b226f8b3828f65015a8e9661dec",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ae35c63d07ea9cefacccd14d5016c010cbb821c"
        },
        "date": 1784947121341,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "dc4f1b15f0060a2a87b9cff8f9b4d17295ee1124",
          "message": "Fix cross resolve for facades, consistency for model-properties, a\ncouple of compleation issue",
          "timestamp": "2026-07-25T04:56:53+02:00",
          "tree_id": "1c82a0d57030d95019e6502736c3dde382eb5fe2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dc4f1b15f0060a2a87b9cff8f9b4d17295ee1124"
        },
        "date": 1784949165913,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 97,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6da452bd1d142f51a64d0e50f4a56da9a8b74bac",
          "message": "Fix inlay position calculation",
          "timestamp": "2026-07-25T05:03:58+02:00",
          "tree_id": "4c376dba0687f62b98936d12dfe51dfc632e4e6b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6da452bd1d142f51a64d0e50f4a56da9a8b74bac"
        },
        "date": 1784949612234,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "54c8bcb8dd29a2b52bd26ed39fffdb8d2de2c4ba",
          "message": "feat: add semantic token modes\n\nDefault semantic tokens to a contextual stream so editor syntax grammars remain in charge of ordinary PHP highlighting. Keep the previous broad stream available with full mode and provide an off switch for users who want no LSP semantic highlighting.\\n\\nDocument the new setting in the configuration reference and schema.",
          "timestamp": "2026-07-24T22:23:25-05:00",
          "tree_id": "6b07430ca4a7e1b019fd1434aec207500b5ccb67",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/54c8bcb8dd29a2b52bd26ed39fffdb8d2de2c4ba"
        },
        "date": 1784950767924,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5826d7f81ad0b3479b7fb90c9b3dd93e5f96c0d9",
          "message": "Add bug note",
          "timestamp": "2026-07-25T05:27:02+02:00",
          "tree_id": "f075a09dc5ec27e048f6296288a6ff753b94bf03",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5826d7f81ad0b3479b7fb90c9b3dd93e5f96c0d9"
        },
        "date": 1784950964269,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 96.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "adf3751709631030b71ec3d1a3377cdf31739bea",
          "message": "L33. Artisan command and signature strings (#274)",
          "timestamp": "2026-07-25T05:32:38+02:00",
          "tree_id": "c10503798c3e0f95b9d98b3ebfbeb68b08f756ac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/adf3751709631030b71ec3d1a3377cdf31739bea"
        },
        "date": 1784951336615,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "758d85e801589356ecda5fa0b207e8a7e6d79fa2",
          "message": "Cargo fmt",
          "timestamp": "2026-07-25T05:35:58+02:00",
          "tree_id": "1af2f6484fdb0d82535d429ca991e3caf6969d73",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/758d85e801589356ecda5fa0b207e8a7e6d79fa2"
        },
        "date": 1784951482573,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "86a35b9274ad6a1ad2036ca7939cd46baa5c5b3e",
          "message": "fix: complete chains after same-line closure close\n\nRecognize lines like `})->` as continuations when completing member\naccess after a multi-line closure argument. This lets the resolver rebuild\nthe full receiver instead of returning no suggestions until the chain\noperator is moved to a fresh line.\n\nCloses #282",
          "timestamp": "2026-07-24T23:07:17-05:00",
          "tree_id": "1c712d20e5a85c987f51b8811ad49a750f74aea1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/86a35b9274ad6a1ad2036ca7939cd46baa5c5b3e"
        },
        "date": 1784953416874,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "852aa6b28a2d7ee9553a98310cb67c358bd03902",
          "message": "fix: resolve conditional return string branch for interpolated strings\n\nThe AST-based conditional return resolver only matched\nExpression::Literal(Literal::String) when checking `$key is string`,\nso interpolated strings like `\"{$prefix}.host\"` (parsed as\nExpression::CompositeString) fell through to the else branch and\nresolved as null. Add CompositeString to the string match so the\ncorrect branch is taken.\n\nCloses #269",
          "timestamp": "2026-07-24T23:37:35-05:00",
          "tree_id": "2cf263003616b24a2f1901a511cd41945f5d33db",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/852aa6b28a2d7ee9553a98310cb67c358bd03902"
        },
        "date": 1784955242031,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "a7f931b89376767a3c8237cef056c9c5d4313b33",
          "message": "fix: resolve conditional return string branch for interpolated strings\n\nThe AST-based conditional return resolver only matched\nExpression::Literal(Literal::String) when checking `$key is string`,\nso interpolated strings like `\"{$prefix}.host\"` (parsed as\nExpression::CompositeString) fell through to the else branch and\nresolved as null. Add CompositeString to the string match so the\ncorrect branch is taken.\n\nCloses #269",
          "timestamp": "2026-07-24T23:38:17-05:00",
          "tree_id": "78bcf69d1e82320e43ca17209577bd5dd5ef7874",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a7f931b89376767a3c8237cef056c9c5d4313b33"
        },
        "date": 1784955292971,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 97.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d7c76c1a83840ae7c7c87f4a8e8c2c2d40c83221",
          "message": "Renaming a constructor-promoted property parameter now cascades to\n`$this->prop` usages",
          "timestamp": "2026-07-25T06:38:52+02:00",
          "tree_id": "1eb678b68f2c8f65d409a06bf6405a213eb80d08",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d7c76c1a83840ae7c7c87f4a8e8c2c2d40c83221"
        },
        "date": 1784955307318,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "b9c69c61cac84ddd0231aa70db8d77cdc5ffede8",
          "message": "fix: syntax error",
          "timestamp": "2026-07-24T23:40:24-05:00",
          "tree_id": "604524744783c374f658d907349036a5e3b35ade",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b9c69c61cac84ddd0231aa70db8d77cdc5ffede8"
        },
        "date": 1784955429098,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "840d33ec39a84ff15a48be4da87a375cad3e42d4",
          "message": "Fix resolution issue",
          "timestamp": "2026-07-25T06:51:31+02:00",
          "tree_id": "286764a0f27b4c4bd6df044500fe833724a1fb21",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/840d33ec39a84ff15a48be4da87a375cad3e42d4"
        },
        "date": 1784955975745,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 49.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 93.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2438e0e606a4c686264b4e6a71bf676a10d17806",
          "message": "Move voted items in to sprint",
          "timestamp": "2026-07-25T06:55:48+02:00",
          "tree_id": "a0c9d6062c2ed3d81d33027e97c2e6bac56bdeb7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2438e0e606a4c686264b4e6a71bf676a10d17806"
        },
        "date": 1784956339158,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 91.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "c7cd659042fdc0396d7b55137f479e3532738afe",
          "message": "feat: scan vendor framework config files as fallback\n\nScan vendor/laravel/framework/config/ for default config values\nwhen the project does not override them with its own config/ file.\nProject configs take precedence, then package service provider\nconfigs, then framework defaults.\n\nAlso adds the same vendor scanning to config key enumeration so\ncompletion and diagnostics know about framework default keys.",
          "timestamp": "2026-07-25T00:14:22-05:00",
          "tree_id": "de6837d1913800b9cdc833e7c42dfc70d37112b3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c7cd659042fdc0396d7b55137f479e3532738afe"
        },
        "date": 1784957314044,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 95.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "8ed83dcc30f21ec3a8f74aa75841cb82276bdc31",
          "message": "fix: wire config resolver into array shape key completion\n\nThe array shape completion path constructed its Loaders without the\nconfig_resolver closure, so variables assigned from config() calls\ndid not offer array key completions even though hover showed the\ncorrect array shape type. Pass the config resolver through so\n$mysql['...'] offers the expected keys.",
          "timestamp": "2026-07-25T00:17:26-05:00",
          "tree_id": "150b92ffccd0c816c300932a4205aaffc2800d28",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ed83dcc30f21ec3a8f74aa75841cb82276bdc31"
        },
        "date": 1784957671501,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 47.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 92.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4cf762815b7bf83d7c8e49624ea0c7e81464ae7f",
          "message": "Intern composite per-member cache keys",
          "timestamp": "2026-07-25T07:43:01+02:00",
          "tree_id": "b46cec142437129ff039f08a97ac4e8b5e084874",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4cf762815b7bf83d7c8e49624ea0c7e81464ae7f"
        },
        "date": 1784959102462,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 91.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "45554d67531057cd673d2e07218c711460f605f7",
          "message": "Improve startup performance",
          "timestamp": "2026-07-25T09:35:32+02:00",
          "tree_id": "cc5bbf60439c3f30a12e3a1bf37e7aefaf7a0955",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/45554d67531057cd673d2e07218c711460f605f7"
        },
        "date": 1784965908026,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 48.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 94,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "40c8c5049c1fe2e415adab8d7db001219a9344f3",
          "message": "Avoid stub duplication on ever thread",
          "timestamp": "2026-07-25T09:59:30+02:00",
          "tree_id": "5a50af61166aad628aa957b2ebcc4a0384d08e00",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/40c8c5049c1fe2e415adab8d7db001219a9344f3"
        },
        "date": 1784967358634,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e8ce13c9e37a928864f1c4fc47502a7e5696f4b4",
          "message": "Fix race condition in laravel config parsing",
          "timestamp": "2026-07-25T10:10:25+02:00",
          "tree_id": "67bddcec232ab46babc410acac5c1390db06bab0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e8ce13c9e37a928864f1c4fc47502a7e5696f4b4"
        },
        "date": 1784968015252,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "468a0e16abaa7a90b25f67a161f86b9eeb933de1",
          "message": "Lower memory use when flattening generic class hierarchies",
          "timestamp": "2026-07-25T11:21:44+02:00",
          "tree_id": "fc69942cb0003ea327952df16b10f551bdfed7d1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/468a0e16abaa7a90b25f67a161f86b9eeb933de1"
        },
        "date": 1784972318931,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 83.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d94d9892b95ab14d2bf3b72c28be86f04cff2f25",
          "message": "Deduplictae methods and properties",
          "timestamp": "2026-07-25T12:49:48+02:00",
          "tree_id": "3ccd542716ebfa5746ed291242289683faef281d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d94d9892b95ab14d2bf3b72c28be86f04cff2f25"
        },
        "date": 1784977550134,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "138bcf329dffd4517af68f95aae8a16cfa02a67c",
          "message": "fix: add context-aware diagnostics for class-string<static>\n\nResolve self/static/$this/parent in parameter types to concrete\nclass names before the type compatibility check, so that\nclass-string<static> is properly validated instead of blanket-\nsuppressed.\n\nThe call-site context class is extracted from the call expression:\nClassName::method uses the named class, static::/self::/$this->\nuses the enclosing class, and parent:: uses its parent.  This\nturns class-string<static> into class-string<DeclaringClass>,\nletting the existing class-string covariance check flag provably\ninvalid arguments (unrelated classes) while still accepting child\nclasses and siblings.\n\nCloses #273",
          "timestamp": "2026-07-25T18:41:03-05:00",
          "tree_id": "826e07521076e84775ef56ad731206ce904c4416",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/138bcf329dffd4517af68f95aae8a16cfa02a67c"
        },
        "date": 1785023813251,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "03ceb68e61b9ebc9cc60f614764ff24212deaf98",
          "message": "refactor: model static and $this as bounded types in the type system\n\nAdd PhpType::StaticType(Atom) and PhpType::ThisType(Atom) variants\nthat preserve late-static-binding semantics instead of flattening\nstatic/$this to a bare class name.  StaticType carries the bound\nclass (\"at least this class or a subclass\"), ThisType is more\nspecific (\"the exact runtime instance type\").\n\nThe subtype chain is ThisType(A) <: StaticType(A) <: Named(A).\nDisplay: static(Foo), $this(Foo) -- shows the bound class.\n\nProduction sites updated:\n- replace_self(fqn) now delegates to resolve_self_refs_bounded()\n  so static -> StaticType(fqn) and $this -> ThisType(fqn);\n  replace_self_with_type(&receiver) is unchanged (preserves full\n  generic receiver types for accurate chain resolution)\n- Subject resolution: $this -> ThisType, static -> StaticType\n- First-class callable partial application: preserves static/this\n- Template substitution: preserve_static path uses bounded types\n- new static() -> StaticType instead of Named\n- Diagnostic param checking: resolve_self_refs_bounded() produces\n  bounded types so class-string<static> is properly validated\n\nSubtype checking updated:\n- StaticType(A) <: Named(A) and ThisType(A) <: Named(A)\n- ThisType(A) <: StaticType(A)\n- base_name(), top_level_class_names(), collect_class_names() all\n  handle the new variants\n\nPeripheral sites updated:\n- return_type_is_mixin_self handles StaticType/ThisType\n- is_simple_php_type handles StaticType/ThisType\n- Union member deduplication handles StaticType/ThisType",
          "timestamp": "2026-07-25T21:09:17-05:00",
          "tree_id": "9d2c1c931241cf5609782998f965c4ea42b2c187",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/03ceb68e61b9ebc9cc60f614764ff24212deaf98"
        },
        "date": 1785032737557,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "5dee2ec75a6c1c75b15c108c21a949e2f4005b09",
          "message": "docs: remove stale eager resolution nav link",
          "timestamp": "2026-07-25T22:09:04-05:00",
          "tree_id": "894cb909a8cc2409c08aee7a5726f37f3d8847e6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5dee2ec75a6c1c75b15c108c21a949e2f4005b09"
        },
        "date": 1785036342544,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b10603ea22fa2b04172a49083b18740bf808e75a",
          "message": "Switch all linux release builds to musl + mimalloc, add memory profiling\nfeatures, plan memory optimizations",
          "timestamp": "2026-07-26T05:17:44+02:00",
          "tree_id": "91d1f3303b0c2295947eeb95439dcb8bf9ca66d6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b10603ea22fa2b04172a49083b18740bf808e75a"
        },
        "date": 1785036722193,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4f15fecad5cd2a9edeeaf9c0db0721197a86c86b",
          "message": "Lower memory use across every stored type",
          "timestamp": "2026-07-26T21:39:25+02:00",
          "tree_id": "41c9a4834f52a836caecdb1dd7ea6f59d07d49f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4f15fecad5cd2a9edeeaf9c0db0721197a86c86b"
        },
        "date": 1785095778795,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "535d3814f01afce87367a74f944a798b9f550c69",
          "message": "Lower memory use in the cross-file reference index",
          "timestamp": "2026-07-26T22:23:18+02:00",
          "tree_id": "217da33c844dd7f76a3d27da3d26db9f4d17f460",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/535d3814f01afce87367a74f944a798b9f550c69"
        },
        "date": 1785098371553,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1f102e3aa9bdecb5a9daaf8dd1bcbd4a33010254",
          "message": "Fix linked editing sometimes working of a stale cache",
          "timestamp": "2026-07-27T00:33:13+02:00",
          "tree_id": "6670495619258e6946a791897bff1020bfd889f5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1f102e3aa9bdecb5a9daaf8dd1bcbd4a33010254"
        },
        "date": 1785106186388,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b89d74d4bfefce69acaebea19d6935e39eb9cf38",
          "message": "Parameter name inlay hints no longer shift to the wrong parameter when\nonly part of a multi-line call is visible",
          "timestamp": "2026-07-27T03:52:23+02:00",
          "tree_id": "dc740cd72a1b3a4e0ca5fc59bac94767ace4dc11",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b89d74d4bfefce69acaebea19d6935e39eb9cf38"
        },
        "date": 1785118077237,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e91c13a2cfcb7d1b6cfe1234fc1d1dce8c5fe2fd",
          "message": "`SymbolKind` stores owned strings per span",
          "timestamp": "2026-07-27T05:06:09+02:00",
          "tree_id": "9772db3ac245d8ccda1fcdd8eec4130decf5688a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e91c13a2cfcb7d1b6cfe1234fc1d1dce8c5fe2fd"
        },
        "date": 1785122490325,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3195f1e539cd20c379e7785e8fde97e28f9386da",
          "message": "Lower memory use for method lookups",
          "timestamp": "2026-07-27T06:05:40+02:00",
          "tree_id": "1de2f558a2dabba944122117840a493add0ad6f5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3195f1e539cd20c379e7785e8fde97e28f9386da"
        },
        "date": 1785126102099,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ded4f96b7dfcff3afc6e6063e2f1e27d3c947f8e",
          "message": "Lower memory use for member access spans",
          "timestamp": "2026-07-27T07:09:12+02:00",
          "tree_id": "3f0567f426320e848a3c3b26db0ef4f822ce6024",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ded4f96b7dfcff3afc6e6063e2f1e27d3c947f8e"
        },
        "date": 1785129938877,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b7d6c21d4136b858f5b321f3df73c3b0a4a44104",
          "message": "Fix memory audit",
          "timestamp": "2026-07-27T07:32:04+02:00",
          "tree_id": "9c03a17713bcde46bb7a2219dbfe579edb3ab920",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b7d6c21d4136b858f5b321f3df73c3b0a4a44104"
        },
        "date": 1785131277380,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fd58e078cd3db647cae606368287d3d924fa4a24",
          "message": "Rename no longer rewrites unrelated code",
          "timestamp": "2026-07-27T09:27:38+02:00",
          "tree_id": "2287ef4fefc915df405c088db8e95441fdd70b17",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fd58e078cd3db647cae606368287d3d924fa4a24"
        },
        "date": 1785138215929,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c68b0c06877e72a8032449babebbd1616bd89f87",
          "message": "Fix namespace renaming",
          "timestamp": "2026-07-27T09:33:05+02:00",
          "tree_id": "b7be648cac7c89bbd87433e9aa8aaadd6986a254",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c68b0c06877e72a8032449babebbd1616bd89f87"
        },
        "date": 1785138657682,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "579bce936bc3490a19aba6487b7bc7b46a87cce9",
          "message": "Type narrowing against `@phpstan-assert`/`@psalm-assert` no longer leaks\nmemory",
          "timestamp": "2026-07-27T14:59:08+02:00",
          "tree_id": "c186c899c7211dd4d6d2eec0cd8753aaa3e86f63",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/579bce936bc3490a19aba6487b7bc7b46a87cce9"
        },
        "date": 1785158068963,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "d816150c8bdb8f0e08bebe1c6bc61ea32777abf4",
          "message": "fix(diagnostics): narrow literal types through compound expressions\n\nThe literal narrowing pass in the diagnostic path only handled direct\nExpression::Literal nodes, so compound expressions like ternaries\n($flag ? 'asc' : 'desc'), null-coalesce, match, and parenthesized\nwrappers resolved to the widened base type (e.g. string) instead of\nthe precise literal union ('asc'|'desc').\n\nExtract the inline narrowing logic into a recursive\nnarrow_literal_type() helper that walks the complete set of PHP\nexpression forms whose result is one of N sub-expression values:\nternary, null-coalesce, match, and parenthesized wrappers.  This is\na closed set — no other PHP expressions select among sub-values.\n\nWhen any leaf is a variable or non-literal expression, the function\nreturns None and the caller keeps the widened type from the general\nresolver, preserving the existing conservative behaviour.\n\nCloses #180",
          "timestamp": "2026-07-27T22:54:56-05:00",
          "tree_id": "c6c4f6f1153b6fc72022ec8f8eeffe4614399bb9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d816150c8bdb8f0e08bebe1c6bc61ea32777abf4"
        },
        "date": 1785211873871,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "273935a86c04deaf820df776367366b674e38021",
          "message": "Identical types are stored once",
          "timestamp": "2026-07-28T07:19:57+02:00",
          "tree_id": "85bc3aae67632b62f6b962e639d971d9f2be7706",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/273935a86c04deaf820df776367366b674e38021"
        },
        "date": 1785216973833,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fc5026d307c44476bab839d57d177c0e47112d3c",
          "message": "Faster diagnostics on large projects",
          "timestamp": "2026-07-28T07:34:37+02:00",
          "tree_id": "12b853f9803ff898e6d153851663f728f2be2851",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fc5026d307c44476bab839d57d177c0e47112d3c"
        },
        "date": 1785217849002,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3095978fbdc0d2f71e192e1aac6091636e0d115f",
          "message": "Find References, Rename, and Go to Implementation no longer look stalled\nduring startup",
          "timestamp": "2026-07-28T18:43:25+02:00",
          "tree_id": "7ea55168a67f1fbbd48f717fbeae723a20d4bd49",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3095978fbdc0d2f71e192e1aac6091636e0d115f"
        },
        "date": 1785257937976,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "55dde0b659106ce6b2df1a9fbdd12c39cd60f8f2",
          "message": "Request input key completion from validation rules (#292)",
          "timestamp": "2026-07-28T19:31:05+02:00",
          "tree_id": "296b388b147d42451e6d78d49116fe787f12582c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/55dde0b659106ce6b2df1a9fbdd12c39cd60f8f2"
        },
        "date": 1785260760079,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f99fb2b929b7fdf0228bb24e005159c1cd75264f",
          "message": "Laravel string keys are collected once instead of once per CPU core",
          "timestamp": "2026-07-29T02:11:19+02:00",
          "tree_id": "f68caec8d37bf5270dcca9f7b1e8f1266c4c82df",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f99fb2b929b7fdf0228bb24e005159c1cd75264f"
        },
        "date": 1785284859346,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "909223409012eacd9cc07c71b9b39979710d5aec",
          "message": "Improve diagnostics multi threading",
          "timestamp": "2026-07-29T13:16:55+02:00",
          "tree_id": "30c49f373608be2ccbf7478bc7ec0bc6f1433c32",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/909223409012eacd9cc07c71b9b39979710d5aec"
        },
        "date": 1785324791035,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "280c500a8d4b05dc9de0e8be38f770fb15f139bb",
          "message": "`@method` and `@property` tags are parsed once per class",
          "timestamp": "2026-07-29T14:27:21+02:00",
          "tree_id": "98b49aed76354520090310d00b6cce32e36d2545",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/280c500a8d4b05dc9de0e8be38f770fb15f139bb"
        },
        "date": 1785329004593,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "de55c370ba1f3a44651f1f713852b018461a2fcc",
          "message": "Upgrade Mago to 1.44.0",
          "timestamp": "2026-07-29T16:00:42+02:00",
          "tree_id": "cd73fa8f60c58e45aa48042aafed644ec27ad9c4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/de55c370ba1f3a44651f1f713852b018461a2fcc"
        },
        "date": 1785334596298,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d868970d1208b2ebc4f597e85acdca5a23f41a7b",
          "message": "Update dependencies",
          "timestamp": "2026-07-29T16:46:57+02:00",
          "tree_id": "c6bb3a02f4333bf931fa078034bde91bdb0b71c2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d868970d1208b2ebc4f597e85acdca5a23f41a7b"
        },
        "date": 1785337423399,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "be16bfa33d696a81982ad0e39722f5b6a3d96271",
          "message": "Migrate to the new Mago packages",
          "timestamp": "2026-07-29T18:27:54+02:00",
          "tree_id": "a7c02e94e710fad75f23dd9cf1f61afa5a4b0a5e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/be16bfa33d696a81982ad0e39722f5b6a3d96271"
        },
        "date": 1785343469923,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e7c65cb516a9dec2ed4f3b1a5d22828b474380db",
          "message": "More Mago migration",
          "timestamp": "2026-07-29T19:05:03+02:00",
          "tree_id": "e5f2c4c160049f882f3937b95b018799c52ae374",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e7c65cb516a9dec2ed4f3b1a5d22828b474380db"
        },
        "date": 1785345700632,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "06f8eb9f006363d5b557c071edcb1b746e375f5a",
          "message": "Migrate more code to new Mago packages",
          "timestamp": "2026-07-29T21:09:42+02:00",
          "tree_id": "26d5a329be5cb0e354d558e22fb1405010160254",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/06f8eb9f006363d5b557c071edcb1b746e375f5a"
        },
        "date": 1785353165312,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "600f8eb6e4c0a17729131b8c586cc1f325b7e8dd",
          "message": "Docblock navigation works in `@method` and `@property` tags written\nacross several lines",
          "timestamp": "2026-07-29T22:05:10+02:00",
          "tree_id": "134b0acc530375c17f5d3c04a3d282e19f03fed9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/600f8eb6e4c0a17729131b8c586cc1f325b7e8dd"
        },
        "date": 1785356498126,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "dalessandro.ariel@gmail.com",
            "name": "Ariel D'Alessandro",
            "username": "adalessa"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "74961c8fe8517703d95cd7c398666bef34ec0eca",
          "message": "update flake",
          "timestamp": "2026-07-30T08:37:15+02:00",
          "tree_id": "5b9cd391d4c218aac4748926536917384417936e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/74961c8fe8517703d95cd7c398666bef34ec0eca"
        },
        "date": 1785394406287,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7ff492ece1b59a72c84db55808fc09a3ec3d5058",
          "message": "Vendor package scanning no longer reads every file twice",
          "timestamp": "2026-07-30T11:12:18+02:00",
          "tree_id": "7a861245350490300c71fa88f061c60071f8d012",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7ff492ece1b59a72c84db55808fc09a3ec3d5058"
        },
        "date": 1785403732094,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "93c9b2680fa96d626e5d72492e062e93fb07f862",
          "message": "The `analyze` and `fix` CLI subcommands no longer build the cross-file\nreference index",
          "timestamp": "2026-07-30T15:58:16+02:00",
          "tree_id": "cf3484642eaa2ffc70d4afedfe20572691c75b6a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/93c9b2680fa96d626e5d72492e062e93fb07f862"
        },
        "date": 1785420888500,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fbb2f7cb89127ca1c5d36a684974763f408acd2f",
          "message": "`static`/`$this` return types are no longer flagged when passed where a\n`Stringable` object is accepted",
          "timestamp": "2026-07-30T16:13:05+02:00",
          "tree_id": "1d189fbdcd5f63feadd2bba3b47656a125855da8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fbb2f7cb89127ca1c5d36a684974763f408acd2f"
        },
        "date": 1785421767045,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "558e6116ffba45dd1afffbfb07ed00b5b1ebb590",
          "message": "Project startup is roughly three times faster",
          "timestamp": "2026-07-30T16:59:27+02:00",
          "tree_id": "1ac6c4fa97d11773732901144393870c02a9c161",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/558e6116ffba45dd1afffbfb07ed00b5b1ebb590"
        },
        "date": 1785424555608,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8e7d58cf2d4583c6a5a2adb5795d3dc0d05a846a",
          "message": "Clean up changelog",
          "timestamp": "2026-07-30T17:31:22+02:00",
          "tree_id": "c5349e316e80acb44de3a1edd0084f374f2611b4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8e7d58cf2d4583c6a5a2adb5795d3dc0d05a846a"
        },
        "date": 1785426467045,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ce58861932f66100e25ba020b3ea4979798262f4",
          "message": "Class origin classification no longer re-scans the whole classmap after\nthe fact",
          "timestamp": "2026-07-30T19:07:07+02:00",
          "tree_id": "104f16d55538a509724ee2bdf4ef68725f4e21f6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ce58861932f66100e25ba020b3ea4979798262f4"
        },
        "date": 1785432207996,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8c704fdd60cbd53eaef8c77aa50e030ecf5dac93",
          "message": "Faster project startup",
          "timestamp": "2026-07-30T21:51:46+02:00",
          "tree_id": "406b640b69ebfef774048fc3c817335076aaebac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8c704fdd60cbd53eaef8c77aa50e030ecf5dac93"
        },
        "date": 1785442074267,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ae8c211d52a256d9fabc8e9aaf61ffef05bdd7e8",
          "message": "Faster `assert()`/type-guard narrowing during the forward walk",
          "timestamp": "2026-07-30T22:12:24+02:00",
          "tree_id": "a5e51b898d1ce3cea81e6a262bdbd27a72187fb8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ae8c211d52a256d9fabc8e9aaf61ffef05bdd7e8"
        },
        "date": 1785443298215,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "38047658bd3ee84ce986b95afda16dbe58c7c94c",
          "message": "Faster diagnostics on method/function calls that resolve to no concrete\nclass",
          "timestamp": "2026-07-31T09:14:30+02:00",
          "tree_id": "c81af222d31cd2441954ff6bfc94ea4d5b7eac29",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/38047658bd3ee84ce986b95afda16dbe58c7c94c"
        },
        "date": 1785483038061,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "52f7d45a3aad12294b496ebb4994fd1ff7e7584b",
          "message": "Argument checking no longer slows down quadratically with file size",
          "timestamp": "2026-07-31T10:08:35+02:00",
          "tree_id": "377846a760d6b67295774edad8c00bce2b23d882",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/52f7d45a3aad12294b496ebb4994fd1ff7e7584b"
        },
        "date": 1785486213496,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c749daa482a32ebb23c26a1215a4cc556ba02611",
          "message": "Argument-count and argument-type diagnostics no longer mix up calls that\nshare the same text but resolve differently",
          "timestamp": "2026-07-31T17:32:10+02:00",
          "tree_id": "4e08bb2c210fb7d722771a3d1246984fd4cf6b89",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c749daa482a32ebb23c26a1215a4cc556ba02611"
        },
        "date": 1785512844526,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d209b97e04efd0496d1ce3d578984c730cddd4c0",
          "message": "Faster workspace symbol search",
          "timestamp": "2026-07-31T17:39:42+02:00",
          "tree_id": "22f220ec795d2c6062a4f0f60210766b9f4392d8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d209b97e04efd0496d1ce3d578984c730cddd4c0"
        },
        "date": 1785513252870,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e722043846d4062241fc67bdfc40b347531cde03",
          "message": "Faster Eloquent scope-method resolution",
          "timestamp": "2026-07-31T19:31:58+02:00",
          "tree_id": "5a656d5bde9f1c6b185ba1e9a9f962eb6977ffae",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e722043846d4062241fc67bdfc40b347531cde03"
        },
        "date": 1785520105244,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "93f701c535ddfdf26bd761b66af1de4f14313923",
          "message": "Saving a file no longer re-analyses every other open tab",
          "timestamp": "2026-07-31T20:23:39+02:00",
          "tree_id": "cf0cba30acfc0d32117ccef1f72da133e5fe5dac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/93f701c535ddfdf26bd761b66af1de4f14313923"
        },
        "date": 1785523261408,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "017b566ce0ef7ae10ab5ca4c2064835700e518e0",
          "message": "Eloquent morph map aliases",
          "timestamp": "2026-07-31T22:01:42+02:00",
          "tree_id": "0f72df9867b10bcaa4f12bae9e03ca460c4b7bdf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/017b566ce0ef7ae10ab5ca4c2064835700e518e0"
        },
        "date": 1785529065035,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "krist7599555@gmail.com",
            "name": "Krist Ponpairin",
            "username": "krist7599555"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "75bd24d79065a662b7423039850707c963cab4cf",
          "message": "fix: Blade comments no longer desync on a quote character\n\n`{{-- ... --}}` comments were preprocessed by temporarily switching\ninto Mode::Php so the closing `--}}` could be recognized and the\ncomment emitted as `/* ... */`. That mode also runs generic PHP\nstring-literal tracking, so an apostrophe or double quote inside the\ncomment text was mistaken for the start of a real string literal. The\nscanner then skipped past the comment's actual `--}}` terminator\nhunting for a matching closing quote, corrupting everything after it\nand producing bogus \"undefined variable\"/\"unexpected token\" errors far\nbelow the comment.\n\nTrack whether Mode::Php represents a comment (mirroring the existing\nin_php_directive_block flag) and skip string tracking while inside\none, since comment text is not PHP code and quotes in it carry no\nsyntactic meaning.\n\nCo-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014r1i5jrXLRGn2KPPBt1cy4",
          "timestamp": "2026-07-31T22:17:16+02:00",
          "tree_id": "5f458d4828accf8ba42e8bd2b5156eb1e3a982da",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/75bd24d79065a662b7423039850707c963cab4cf"
        },
        "date": 1785529962560,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0871fff343119b8c92b0576ca5f3741dd1f46e0e",
          "message": "Typed validated() array shapes from rules",
          "timestamp": "2026-07-31T22:52:53+02:00",
          "tree_id": "0eca96ffdb4dfad2f68cf0b0cc1330dbd0ad40a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0871fff343119b8c92b0576ca5f3741dd1f46e0e"
        },
        "date": 1785532146203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "88ba719a9a297ad6bf0e392696b76d3919453428",
          "message": "Route parameter name completion",
          "timestamp": "2026-07-31T23:15:34+02:00",
          "tree_id": "72bf174c87ffbc1b14a072303908269823f91e51",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/88ba719a9a297ad6bf0e392696b76d3919453428"
        },
        "date": 1785533508332,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "abecab26069b251d7659d0ea9a1a7e340436a645",
          "message": "A template parameter bound only by the argument it type-checks no longer\nflags a false positive",
          "timestamp": "2026-08-01T09:38:24+02:00",
          "tree_id": "a4ee59b26307dc1bcb62d8cf7baa5ffbda56be38",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/abecab26069b251d7659d0ea9a1a7e340436a645"
        },
        "date": 1785571205610,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "snowyukitty@outlook.com",
            "name": "snowyukitty",
            "username": "snowyukitty"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a2e7012ff0493aca79335540ad38a94a81230822",
          "message": "preserve scalar literal types in expression resolution",
          "timestamp": "2026-08-01T11:22:48+02:00",
          "tree_id": "6c4dfee6d199b08051131fb67e4fe427ded72795",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a2e7012ff0493aca79335540ad38a94a81230822"
        },
        "date": 1785577122572,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b7e42e03a14a034dc31389cf5dce2a203b582d6e",
          "message": "Remove type leniency",
          "timestamp": "2026-08-01T11:23:31+02:00",
          "tree_id": "904284413d099797e7ff46c8990cb3d50d85d3eb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b7e42e03a14a034dc31389cf5dce2a203b582d6e"
        },
        "date": 1785577659581,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "523a3669739926aae4dae85708575e367637568a",
          "message": "A ternary with a statically-known condition no longer unions in its dead\narm",
          "timestamp": "2026-08-01T12:12:57+02:00",
          "tree_id": "bd79fd57f96a3274d77baa9d334a88b793da8d5b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/523a3669739926aae4dae85708575e367637568a"
        },
        "date": 1785580200367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "422f1282ea66ac1aa27ef0ea8e1507214630a594",
          "message": "fix: class constant references keep their literal value\n\nA constant declared with a scalar initializer widened to its base type\nwhen referenced (`Foo::STRING_CONSTANT` → `string`), while the same\nliteral assigned to a variable kept its value. Constant-value inference\nnow produces the literal type (`'foo'`, `1`, `3.14`) so precision no\nlonger depends on whether a value reaches an expression through a\nvariable or a constant. This covers class constants (including typed\nconstants, `self::`, and inheritance) and global `const`/`define()`\nconstants, since all resolve through the same value classifier.\n\nNon-literal initializers keep the previous widened result: quoted text\nthat is actually a concatenation, arithmetic between floats, and int\nspellings that overflow i64 all stay at the base type. Hex, binary,\noctal, and underscored int spellings normalise to their decimal value.\n\nThe ported PHPStan fixtures now assert the literal expectations from\nupstream's nsrt corpus instead of the widened types.\n\nCloses #309",
          "timestamp": "2026-08-01T14:31:04+02:00",
          "tree_id": "c882c7732de70c4179032a4138453d86a3c72994",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/422f1282ea66ac1aa27ef0ea8e1507214630a594"
        },
        "date": 1785588657373,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3528d191487a5126c2c8d4f01cb5c974d6315003",
          "message": "parent::SOME_CONSTANT` resolves to a type",
          "timestamp": "2026-08-01T16:22:25+02:00",
          "tree_id": "2cec5a6f1f73a4d00169a6d1bb3e25ec41823f9d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3528d191487a5126c2c8d4f01cb5c974d6315003"
        },
        "date": 1785595126474,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "27fe59d40cb5165435416e2b658b91847df47806",
          "message": "Backing type for enum validation rules",
          "timestamp": "2026-08-01T17:00:43+02:00",
          "tree_id": "521767b4be19d3971c8aca006dd702f73cc2ef97",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/27fe59d40cb5165435416e2b658b91847df47806"
        },
        "date": 1785597309611,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "976bb69a9d4dca6686d3b4fc687764b595f3cdc0",
          "message": "Route parameter name completion",
          "timestamp": "2026-08-01T20:51:44+02:00",
          "tree_id": "1c07c72716da06536260f1f0c9f97f0c64195174",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/976bb69a9d4dca6686d3b4fc687764b595f3cdc0"
        },
        "date": 1785611285281,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ee86788428f1136b5887004acb750b68f0d66bdf",
          "message": "fix: @property tag overrides database-derived column type\n\nA model attribute documented with an explicit `@property` tag — on the\nmodel itself or on a trait it uses — resolved to the raw column type\nderived from the schema dump or migrations. For a timestamp column the\nschema yields `string|null`, so `@property Carbon $published_at` on a\ntrait was ignored and chaining date methods reported \"Cannot access\nmethod on type 'string'\".\n\nThe schema only knows the storage type; the tag declares what the\nattribute is at runtime, which Eloquent's casting makes true. Property\ntags collected by the PHPDoc provider from the class, its traits,\nparents, and interfaces are now marked with a `DocblockTag` source, and\nthe virtual-member merge lets such a tag replace a type that was merely\ninferred from the database (schema columns, `$attributes` defaults) even\nat equal type specificity. Types produced by real PHP code — `$casts`,\naccessors, relationship methods — still win the tie, and real declared\nproperties are never displaced. `@mixin`-borrowed tags keep their\nlowest-precedence standing.\n\nCloses #306",
          "timestamp": "2026-08-01T21:07:37+02:00",
          "tree_id": "78d8079c1d9ac9277d1022bffce5001916beff49",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ee86788428f1136b5887004acb750b68f0d66bdf"
        },
        "date": 1785612579010,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d12e7c90f7357d4ad5d4025ae806bfddc4cfdc52",
          "message": "fix: exclude relationship builder methods from virtual properties\n\nThe relationship property synthesis loop iterated over all inherited\nmethods, including the base relationship builder methods from\nHasRelationships (hasMany, belongsTo, morphOne, etc.).  Because these\nmethods have relationship return types, they were incorrectly turned\ninto virtual properties and count properties (has_many_count,\nbelongs_to_count, etc.) on every Eloquent model.\n\nSkip methods whose name matches one of the known relationship builder\nmethod names in both the property and count property synthesis loops.\n\nCloses #312\n\nCo-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>",
          "timestamp": "2026-08-01T21:31:15+02:00",
          "tree_id": "ff224df83c622b2d169d6bb8dc7b4550f823b7e9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d12e7c90f7357d4ad5d4025ae806bfddc4cfdc52"
        },
        "date": 1785613718957,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8a172e437457592ade5bf530d6ce3443082d3ade",
          "message": "fix: exclude relationship builder methods from virtual properties",
          "timestamp": "2026-08-01T21:33:37+02:00",
          "tree_id": "ff224df83c622b2d169d6bb8dc7b4550f823b7e9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8a172e437457592ade5bf530d6ce3443082d3ade"
        },
        "date": 1785613800329,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "ffd60406db4c9c44c101322415dea751da1c3df1",
          "message": "Laravel higher-order collection proxies",
          "timestamp": "2026-08-01T23:37:31+02:00",
          "tree_id": "5602ddbc6c1b8cbea39a4ddfb76566b926fb385d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ffd60406db4c9c44c101322415dea751da1c3df1"
        },
        "date": 1785621209059,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "17cdf973e32f3feaf85edc5db128e2839ad380ae",
          "message": "Framework internals no longer appear as properties on Eloquent models",
          "timestamp": "2026-08-01T23:47:13+02:00",
          "tree_id": "7f8399a3861cb47793b36eefdd9b636cd267c0ee",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/17cdf973e32f3feaf85edc5db128e2839ad380ae"
        },
        "date": 1785621926744,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "b9d0da90554862fe867d6e3b3ca00139f53856cf",
          "message": "docs: update nvim installation instructions\n\nCloses #310",
          "timestamp": "2026-08-01T19:01:44-05:00",
          "tree_id": "ef31ad51851e08623dbed919c501fe3669c0bd31",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b9d0da90554862fe867d6e3b3ca00139f53856cf"
        },
        "date": 1785629881004,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "38919ea76d29a5da070ee6667d8fd858b5a1d5d4",
          "message": "chore: remove already implemented feature item from todo list",
          "timestamp": "2026-08-02T10:35:07+02:00",
          "tree_id": "7841a90fc74a415ccb2bad4c4ac590099a7e2c8b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/38919ea76d29a5da070ee6667d8fd858b5a1d5d4"
        },
        "date": 1785660702435,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3c77654e051e80c58b25c887452b91e74d7e2f05",
          "message": "Clean up comments",
          "timestamp": "2026-08-02T14:47:07+02:00",
          "tree_id": "f921c2f45b1897341fea0a103ff992f492eef6bf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3c77654e051e80c58b25c887452b91e74d7e2f05"
        },
        "date": 1785675807812,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c297202e5522a0b9137c222422af1d09ca09e001",
          "message": "A static call on an unqualified class name resolves against the current namespace first",
          "timestamp": "2026-08-02T15:04:12+02:00",
          "tree_id": "a0d5acea6840dd5189c9d8749524a710c9dcd383",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c297202e5522a0b9137c222422af1d09ca09e001"
        },
        "date": 1785676865554,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b0ee72d45f8be4c1008ad547bab3997ee7d43e23",
          "message": "A qualified class name resolves against the current namespace first",
          "timestamp": "2026-08-02T15:25:58+02:00",
          "tree_id": "c1d9e60fd03573108bd06b5e08c241b71c5b41fd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b0ee72d45f8be4c1008ad547bab3997ee7d43e23"
        },
        "date": 1785678081705,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cb517af773561992c2a40c00611f1a645cddba2c",
          "message": "Clean up more code comments",
          "timestamp": "2026-08-02T19:10:59+02:00",
          "tree_id": "81e64814448dbad324e5de1bf8bfe83c805771a5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cb517af773561992c2a40c00611f1a645cddba2c"
        },
        "date": 1785691619121,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bdeaa14aae998759bc8b1934635cf1e9b1ee0dde",
          "message": "Member completion at the class-body root",
          "timestamp": "2026-08-02T21:54:52+02:00",
          "tree_id": "26a32e715b3fbbc68401fcb3a6fda176e20ed98b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bdeaa14aae998759bc8b1934635cf1e9b1ee0dde"
        },
        "date": 1785701534203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "226292433ec07ce3e758433cfa48f86b4181aae4",
          "message": "Clean up test comments",
          "timestamp": "2026-08-02T22:23:59+02:00",
          "tree_id": "be619998ee28542607ecff220ee1ac675e092ae3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/226292433ec07ce3e758433cfa48f86b4181aae4"
        },
        "date": 1785703211976,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "702695d80bb7d233e60ceb659fc6210ea166d89d",
          "message": "Resource route URIs",
          "timestamp": "2026-08-02T22:28:53+02:00",
          "tree_id": "864727fcdade6cb31c459ee5d137ff43ad4f90fc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/702695d80bb7d233e60ceb659fc6210ea166d89d"
        },
        "date": 1785703444478,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e2e3ae7a52345542c8268e7684a3ff001df48b03",
          "message": "Fix various issues found with recent changes",
          "timestamp": "2026-08-03T01:35:10+02:00",
          "tree_id": "65c50bab52610f3bfe2469bc07d4574090d4508c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e2e3ae7a52345542c8268e7684a3ff001df48b03"
        },
        "date": 1785714676240,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "701106a32101a8c7c955fe35977b46c3d360a29d",
          "message": "Fix a host of issues found during review",
          "timestamp": "2026-08-03T02:41:30+02:00",
          "tree_id": "f8e4eecaec9ec542cf5445719c22ad193362cc15",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/701106a32101a8c7c955fe35977b46c3d360a29d"
        },
        "date": 1785718667893,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f4ea6e63bd6dea629f61a14270b761309c22386c",
          "message": "Fix a few more issues",
          "timestamp": "2026-08-03T02:54:45+02:00",
          "tree_id": "d73e2483e8f078fbdb57501ff2e0562dcb5f1665",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f4ea6e63bd6dea629f61a14270b761309c22386c"
        },
        "date": 1785719464203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6dc9a7a1dc2b1afe7dfadb537b1c8a802f344884",
          "message": "Fix a batch of issues found during system review",
          "timestamp": "2026-08-03T03:23:33+02:00",
          "tree_id": "0ae3375cad537d8b901ba7b6f49b267f1d342bd6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6dc9a7a1dc2b1afe7dfadb537b1c8a802f344884"
        },
        "date": 1785721187070,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "815a400e18f39bd116c53da6a177b2fcc4bd5951",
          "message": "Fix a handful of bugs found when analyzing the system",
          "timestamp": "2026-08-03T04:18:49+02:00",
          "tree_id": "04126c162335dc18dca4d0f17aa638420258b798",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/815a400e18f39bd116c53da6a177b2fcc4bd5951"
        },
        "date": 1785724533295,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7934a9298c7a422b77b179b3b30a2a9bd139f679",
          "message": "A property assigned inside a guarded `if` keeps that type after the\nblock",
          "timestamp": "2026-08-03T04:43:44+02:00",
          "tree_id": "4e97c2bf7a1e19aa5a3cbab3da16f22e8145f305",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7934a9298c7a422b77b179b3b30a2a9bd139f679"
        },
        "date": 1785725986852,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "329f1112e7f91099cf912f1cf577983ce4c212fd",
          "message": "A long `??` chain or a deeply nested ternary still resolves",
          "timestamp": "2026-08-03T11:36:34+02:00",
          "tree_id": "ace84829305c783a24aa3ed8e586e84e4108e454",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/329f1112e7f91099cf912f1cf577983ce4c212fd"
        },
        "date": 1785750798021,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ca4fe8418e743873767ae5a4f240d627fd57df23",
          "message": "chore: remove L18 from todo sprint table",
          "timestamp": "2026-08-03T23:17:42+02:00",
          "tree_id": "c5f6c10a352b4c2afc238329d652c54ff7f2ec7c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ca4fe8418e743873767ae5a4f240d627fd57df23"
        },
        "date": 1785792862089,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "930756eaf42559cbab903559b2b58621f2355522",
          "message": "A write through `__set` no longer overrides what `__get` returns",
          "timestamp": "2026-08-03T23:19:47+02:00",
          "tree_id": "6b48a93c011846eea6edc8362dc151d35494a667",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/930756eaf42559cbab903559b2b58621f2355522"
        },
        "date": 1785792959835,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "78dc2febba8d948df6890aa50b34b21dfd4e5190",
          "message": "A very long fluent chain no longer crashes the language server",
          "timestamp": "2026-08-04T01:09:53+02:00",
          "tree_id": "183243570dd74c8e5ef6a8c14fa7e3861c1b7baf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/78dc2febba8d948df6890aa50b34b21dfd4e5190"
        },
        "date": 1785799504601,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0579fc689f67c210e606d642403cd9f90695f2f4",
          "message": "Functions loaded through a `__DIR__`-relative `require_once` chain are\nindexed",
          "timestamp": "2026-08-04T01:13:42+02:00",
          "tree_id": "02029305c620857f7aa8f376a58891b7eb355819",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0579fc689f67c210e606d642403cd9f90695f2f4"
        },
        "date": 1785799691197,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ff212e83e303dc9ed7a3e4e502e57bb2f0022fe9",
          "message": "Promote to constructor property\" keeps the property's attributes",
          "timestamp": "2026-08-04T01:36:54+02:00",
          "tree_id": "0befeaa44ff337bebc302ef6ba0fc7ebfe2a5379",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ff212e83e303dc9ed7a3e4e502e57bb2f0022fe9"
        },
        "date": 1785801212533,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "be0988c9e9834ea93f2a7c0eee4a8b4a1cf71b39",
          "message": "Every feature now resolves a type as well as hover does",
          "timestamp": "2026-08-04T02:11:42+02:00",
          "tree_id": "48cef0f653cb4625cabc49b19c91e2511c794910",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/be0988c9e9834ea93f2a7c0eee4a8b4a1cf71b39"
        },
        "date": 1785803289346,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b18dbf61dc48bdda97f8ebb0aca960532283d807",
          "message": "The type-engine resolvers are ambient thread-local state",
          "timestamp": "2026-08-04T03:20:10+02:00",
          "tree_id": "e1aa40cf29f543ca3828056e8af8ad2496d01fb3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b18dbf61dc48bdda97f8ebb0aca960532283d807"
        },
        "date": 1785807396350,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "be4383c82615fb4dbd633daa347649067d58ed29",
          "message": "Override completion writes `static`, not `$this`, as the return type",
          "timestamp": "2026-08-04T03:25:23+02:00",
          "tree_id": "c0cbb54d2cbef5d0816e98cb958320e91753bc4c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/be4383c82615fb4dbd633daa347649067d58ed29"
        },
        "date": 1785807718966,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b47e2783683612b8851b2909ae389866cc1a41a8",
          "message": "Fix a few more override generation issues",
          "timestamp": "2026-08-04T03:41:48+02:00",
          "tree_id": "214f69833b607d0671506e643e25757ab9b8a80b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b47e2783683612b8851b2909ae389866cc1a41a8"
        },
        "date": 1785808738653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "1a781704a47f9b2ee4ebcd73a1be2c264aa67aa3",
          "message": "feat: add config toggle for variable linked editing ranges\n\nAdd a [linked_editing] section to .phpantom.toml with a `variables`\ntoggle (default: true) that controls whether placing the cursor on a\nvariable activates linked editing mode. When set to false, the handler\nreturns None and the editor falls back to explicit rename.\n\nThe server capability stays registered so future linked editing kinds\ncan be added independently.\n\nChanges:\n- src/config.rs: LinkedEditingConfig struct with variables_enabled()\n  accessor and three unit tests\n- src/linked_editing.rs: early return when variables disabled\n- config-schema.json: linked_editing section for TOML schema support\n- docs/CHANGELOG.md: user-facing entry",
          "timestamp": "2026-08-04T15:16:02-05:00",
          "tree_id": "242f5c231351750ab2b0e1259f4cecb4343c2ed0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1a781704a47f9b2ee4ebcd73a1be2c264aa67aa3"
        },
        "date": 1785875575139,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e5f052380a93452c47eb18750195d94a9cbebf0b",
          "message": "Analyze verbosity flags",
          "timestamp": "2026-08-04T22:23:44+02:00",
          "tree_id": "c36be9008759888ce7c4131c349d008fec9a1ba7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e5f052380a93452c47eb18750195d94a9cbebf0b"
        },
        "date": 1785875985938,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "08592e1eb3fd1142716388209f5022d19d21b583",
          "message": "Remove Linked Editing support",
          "timestamp": "2026-08-05T13:00:35+02:00",
          "tree_id": "390ce924a9eb0b54d2dbeee62a86fa10b7d73732",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/08592e1eb3fd1142716388209f5022d19d21b583"
        },
        "date": 1785928592869,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "343c606eadee9a7d97c8f80ff1afb75f323d5058",
          "message": "A union of two classes with the same short name keeps both halves",
          "timestamp": "2026-08-05T23:20:11+02:00",
          "tree_id": "304971071a40431b82e39d025e7d697b1847e861",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/343c606eadee9a7d97c8f80ff1afb75f323d5058"
        },
        "date": 1785965779540,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "095ebff0d5e8597b087677cb14143f4c7b83b8cb",
          "message": "Calling a function or method that returns `never` is now recognized as\nan unconditional exit",
          "timestamp": "2026-08-06T00:17:02+02:00",
          "tree_id": "14f24a3d133cd8c0b608d7429351c27ee473705f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/095ebff0d5e8597b087677cb14143f4c7b83b8cb"
        },
        "date": 1785969193475,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8dc82ec5c988cb7572826d32360666ad27b77caf",
          "message": "Update Mago to 1.46.0",
          "timestamp": "2026-08-06T01:13:00+02:00",
          "tree_id": "8e1d15154c95af9cfb892c96c33d25296bced7b0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8dc82ec5c988cb7572826d32360666ad27b77caf"
        },
        "date": 1785972542603,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ef75866d4a0450d5e9d221892135c0cf367b7375",
          "message": "\"Extract function\" no longer breaks by-reference writes",
          "timestamp": "2026-08-06T01:16:02+02:00",
          "tree_id": "e4cd332918453a5c37fbda0df2aae0c7b464b751",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ef75866d4a0450d5e9d221892135c0cf367b7375"
        },
        "date": 1785972742619,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "413fe1586b1a01376fbf48b0dd5f32a79e82c27e",
          "message": "A guard clause written with the alternative `if: … endif;` syntax now\nnarrows",
          "timestamp": "2026-08-06T01:47:37+02:00",
          "tree_id": "5ace18ce4effd1c7dab870396ae5975c2883d836",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/413fe1586b1a01376fbf48b0dd5f32a79e82c27e"
        },
        "date": 1785974639868,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f070dc23512a8bc36bc02ef6dc88ba6fb6356f46",
          "message": "Hover no longer builds a scope snapshot nothing reads",
          "timestamp": "2026-08-06T01:48:03+02:00",
          "tree_id": "9870d3b4d57be5f04d11a0538e0967a84a6651f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f070dc23512a8bc36bc02ef6dc88ba6fb6356f46"
        },
        "date": 1785974655048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3ca363499ce2b96929e2568fc48391041030264f",
          "message": "Nested `@param-closure-this` closures resolve `$this` to the innermost\nbinding",
          "timestamp": "2026-08-06T02:29:56+02:00",
          "tree_id": "d888e740ad984ddbb7ac00c4ae10b223ff38c0b0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3ca363499ce2b96929e2568fc48391041030264f"
        },
        "date": 1785977206599,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "658dbb8230cbdcafbe813311b95968aa6797c582",
          "message": "A plain function body resolves class names against the file's namespace",
          "timestamp": "2026-08-06T02:50:01+02:00",
          "tree_id": "c4726fa7c48637f8ed8a3ea68ea07dfee92273f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/658dbb8230cbdcafbe813311b95968aa6797c582"
        },
        "date": 1785978406702,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "49c26d1d9af932f70891d76c09f5403c3bdd6308",
          "message": "Blade templates infer their variables from `view()` call site",
          "timestamp": "2026-08-06T14:43:39+02:00",
          "tree_id": "c6302e0a5d052e81696a6b7a9706a1f9e490752e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/49c26d1d9af932f70891d76c09f5403c3bdd6308"
        },
        "date": 1786021191481,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "028efbeaa946a1c3fadc8486f9911eb5f58be583",
          "message": "A conditional buried in a generic return type is decided at the call\nsite",
          "timestamp": "2026-08-06T20:10:35+02:00",
          "tree_id": "2897334af0da8b79364e8d33c909a3c70b4cb46e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/028efbeaa946a1c3fadc8486f9911eb5f58be583"
        },
        "date": 1786046972564,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1dc45eb635a2869bfa7fc301843a698622d17cbd",
          "message": "A generic class is no longer rejected by a parameter typed with that\nsame class",
          "timestamp": "2026-08-06T20:22:27+02:00",
          "tree_id": "cf577be0f0a8165d886e6d00c9575e4e01c2a3f7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1dc45eb635a2869bfa7fc301843a698622d17cbd"
        },
        "date": 1786047475282,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "70afd97de16c250f9b192f9591939a74ff3763a8",
          "message": "`auth()->user()` and `Auth::user()` resolve to the configured model\nagain",
          "timestamp": "2026-08-06T23:11:38+02:00",
          "tree_id": "68107324b7c96e7ac3db8728c62154a041780ece",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/70afd97de16c250f9b192f9591939a74ff3763a8"
        },
        "date": 1786062491198,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0e7fb3f5d506eb17b91425c269c040889b6ea79e",
          "message": "A comment no longer displaces the call a string argument belongs to",
          "timestamp": "2026-08-07T02:16:48+02:00",
          "tree_id": "179da6777da1c4f22fb23c8e6d42a00861e03b46",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0e7fb3f5d506eb17b91425c269c040889b6ea79e"
        },
        "date": 1786062809042,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c91a2549541d334576e83aea2d7a11cfb3365c79",
          "message": "A comment before the arrow or double colon no longer hides a\nstring-argument call's receiver",
          "timestamp": "2026-08-07T02:38:06+02:00",
          "tree_id": "3e5ef1159f15b97851d1142e30f1e4d2bf2308ec",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c91a2549541d334576e83aea2d7a11cfb3365c79"
        },
        "date": 1786064063439,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f3203582de8ea2f91e2d1ea688a3a8769527c4e9",
          "message": "A comment no longer hides a request field's receiver, including through\nthe `safe()` hop",
          "timestamp": "2026-08-07T03:02:23+02:00",
          "tree_id": "7a73b2a9d87dd64656a351c7265a94483ada2f90",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f3203582de8ea2f91e2d1ea688a3a8769527c4e9"
        },
        "date": 1786065514167,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cb721bf2126c36bbdfb1e1980344f18b3f2dc79c",
          "message": "Improve route discovery",
          "timestamp": "2026-08-07T03:03:59+02:00",
          "tree_id": "117d02532cf0b48f85744c040e5587e10b950030",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cb721bf2126c36bbdfb1e1980344f18b3f2dc79c"
        },
        "date": 1786065575605,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "979f0988f66a01368a433cadcc9f56ecb35a5958",
          "message": "A package with no route files of its own no longer flags every `route()`\ncall as unknown",
          "timestamp": "2026-08-07T03:11:26+02:00",
          "tree_id": "ab379bbbee72f07a289545c4106e891cc8da1b9a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/979f0988f66a01368a433cadcc9f56ecb35a5958"
        },
        "date": 1786066100483,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6ba6dcd17e3c38b2d1d1261e94ee3fad16801430",
          "message": "Routes registered by a router macro are recognized",
          "timestamp": "2026-08-07T03:38:38+02:00",
          "tree_id": "b787a9bed62e84c537d6a1a24afa84f5a7d04027",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6ba6dcd17e3c38b2d1d1261e94ee3fad16801430"
        },
        "date": 1786067684864,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0c63c6f0c97512abc268f66cd0cb86048ee53c92",
          "message": "Route names built in a loop are no longer reported as unknown",
          "timestamp": "2026-08-07T03:52:36+02:00",
          "tree_id": "7b1c5e8f8c5e7191fb8ee01d774b8a076966d4b6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0c63c6f0c97512abc268f66cd0cb86048ee53c92"
        },
        "date": 1786068557872,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0dd3910fb18981780d1dacf8a69948eaf77da396",
          "message": "`$this` inside a macro closure resolves to the target again in\ndiagnostics, hover, and go-to-definition",
          "timestamp": "2026-08-07T04:00:47+02:00",
          "tree_id": "ff8f6a61580dba4b18fe8997a8fd8997f95ca231",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0dd3910fb18981780d1dacf8a69948eaf77da396"
        },
        "date": 1786069374542,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2013e45e6a48b5c2575aefbbc4a92f6300f5fad6",
          "message": "Route names built with a string function are no longer reported as\nunknown",
          "timestamp": "2026-08-07T04:23:28+02:00",
          "tree_id": "16ce13a23e5674097eac15f187ffb95699b7fd0f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2013e45e6a48b5c2575aefbbc4a92f6300f5fad6"
        },
        "date": 1786070445092,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c7f2c7e330b9337ab296b1812f65f04a20b7d0ac",
          "message": "Provider resource paths behind a local variable are now resolved",
          "timestamp": "2026-08-07T04:47:00+02:00",
          "tree_id": "8cd2bf18f097bb7266b9b854eca56d7afa89c32d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c7f2c7e330b9337ab296b1812f65f04a20b7d0ac"
        },
        "date": 1786071811836,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a50597cfa02f65d288a676d92737c2eae91aab49",
          "message": "Completion is fast again in large files",
          "timestamp": "2026-08-07T05:41:26+02:00",
          "tree_id": "46d15a7155069cb77de9375cddb47f14bb0b2e41",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a50597cfa02f65d288a676d92737c2eae91aab49"
        },
        "date": 1786075001848,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c1fcf2b0ea50e451cf820daacdf55707f02cd10b",
          "message": "Completion is fast again in large files",
          "timestamp": "2026-08-07T07:27:50+02:00",
          "tree_id": "5a1a98383d9346e4e2db9218a8d92a5a9d8d9664",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c1fcf2b0ea50e451cf820daacdf55707f02cd10b"
        },
        "date": 1786081470211,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5298f43f06b6af02d5af73fe08f8092b404b62d9",
          "message": "Imports written inside a Blade template are honoured",
          "timestamp": "2026-08-07T19:07:31+02:00",
          "tree_id": "345d7f708780e5f235036de6c92e37b00794809d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5298f43f06b6af02d5af73fe08f8092b404b62d9"
        },
        "date": 1786123401276,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cc3c75712174a3c2ab205634abfa4f446d71ffbf",
          "message": "Imports written inside a Blade template are honoured",
          "timestamp": "2026-08-07T19:13:20+02:00",
          "tree_id": "de947f993f10638bacd501c62c51026048e255a8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cc3c75712174a3c2ab205634abfa4f446d71ffbf"
        },
        "date": 1786123706129,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bd37b7ebb923bf14b8bf14b5fd36d3a43efa240c",
          "message": "A standalone `@var` block keeps its variable in scope for the rest of\nthe body",
          "timestamp": "2026-08-07T19:46:32+02:00",
          "tree_id": "e3a1a97bcd28a38613e2a6871de3bba96d0a89cf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bd37b7ebb923bf14b8bf14b5fd36d3a43efa240c"
        },
        "date": 1786125750886,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "19a485120e946efd3ba46efbe8d3946b0977327e",
          "message": "Included route file paths behind a local variable are now resolved",
          "timestamp": "2026-08-07T19:47:58+02:00",
          "tree_id": "a6cca38c598e90ddf2c6f7ce33f907a829119dbf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/19a485120e946efd3ba46efbe8d3946b0977327e"
        },
        "date": 1786125908590,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "97fd70bffd2988fe326d99a78b8f1cfb8cad3932",
          "message": "`@props` declares its keys as local variables",
          "timestamp": "2026-08-07T20:52:39+02:00",
          "tree_id": "9358778d992434318c999c4ef66030c6f1d76fdd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/97fd70bffd2988fe326d99a78b8f1cfb8cad3932"
        },
        "date": 1786129710604,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7dfeb1b56d3457c65960af20c9535cdc91e31c01",
          "message": "A `@method` tag no longer overrides a method that really exists",
          "timestamp": "2026-08-07T20:52:11+02:00",
          "tree_id": "44c58ddbdd55368cfcf028de8fe980739090c29a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7dfeb1b56d3457c65960af20c9535cdc91e31c01"
        },
        "date": 1786129740679,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6120382a8d970486386ad73f1be3433c35f1890d",
          "message": "A Blade component attribute may wrap over several lines",
          "timestamp": "2026-08-07T22:04:58+02:00",
          "tree_id": "883355e906af17dec7f2c81fa04c16b908601625",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6120382a8d970486386ad73f1be3433c35f1890d"
        },
        "date": 1786134042784,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a87dfece6578cc720fdace3d2f58e947c1682f9e",
          "message": "`isset()` in a short-circuit condition now marks the variable defined\nfor the rest of the chain",
          "timestamp": "2026-08-07T22:05:32+02:00",
          "tree_id": "65f71ee649a36ec08576c15e58023c3c225cafe6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a87dfece6578cc720fdace3d2f58e947c1682f9e"
        },
        "date": 1786134138154,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ac87b9fce79b7e9759fc21cd42e2c98cb0c3000c",
          "message": "A check stored in a variable still narrows",
          "timestamp": "2026-08-07T23:14:32+02:00",
          "tree_id": "d430db21243936c185069f27d9694a917590672e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ac87b9fce79b7e9759fc21cd42e2c98cb0c3000c"
        },
        "date": 1786138176993,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f0ba73ab7896e3acddb52fc0d73c3b6e4758b9a3",
          "message": "Translation keys are no longer judged when the application loads them\nfrom elsewhere",
          "timestamp": "2026-08-07T23:15:42+02:00",
          "tree_id": "0dea1a882aacb654fc3611944dc948377022936e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f0ba73ab7896e3acddb52fc0d73c3b6e4758b9a3"
        },
        "date": 1786138285833,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "340f8297f952a42eff88fc5a7d207d31751a5920",
          "message": "A closure parameter declared as plain `array` narrows to what the call\nsite passes",
          "timestamp": "2026-08-07T23:46:57+02:00",
          "tree_id": "a313fe6d89abe544a56017bf20f3d84470c7a252",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/340f8297f952a42eff88fc5a7d207d31751a5920"
        },
        "date": 1786140227018,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "46cc0868a6768afd4f45be187a629ffce90e3279",
          "message": "An array function keeps its element type when the call is used inline",
          "timestamp": "2026-08-08T00:26:51+02:00",
          "tree_id": "9fa32d4c3162b9f99addf9473bead663f872a46e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/46cc0868a6768afd4f45be187a629ffce90e3279"
        },
        "date": 1786142609784,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3ac002f2b0842efd7cf0ddcc7667509900a3686b",
          "message": "`App::make()`, `App::makeWith()`, and `App::resolve()` resolve a\nclass-string argument to that class",
          "timestamp": "2026-08-08T00:28:28+02:00",
          "tree_id": "7cd3a9fe6e6fbe6e28e53d0bc16d565bb26a6fdf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3ac002f2b0842efd7cf0ddcc7667509900a3686b"
        },
        "date": 1786142685555,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "49a212a75da1417bd017f3392b6340820fc1a530",
          "message": "String container bindings resolve to their bound class",
          "timestamp": "2026-08-08T01:01:11+02:00",
          "tree_id": "bc204736c5d47c88fa60c2e2216c340917aa1009",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/49a212a75da1417bd017f3392b6340820fc1a530"
        },
        "date": 1786144658230,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bff58d08eaae11398a24e7cd96051d2903e09025",
          "message": "A static call through a `string`-typed subject is no longer reported as\nscalar access",
          "timestamp": "2026-08-08T01:06:50+02:00",
          "tree_id": "fc064daed23814e4bc548929898c4a7e135c6e8b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bff58d08eaae11398a24e7cd96051d2903e09025"
        },
        "date": 1786145848091,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cf15caa88aa07575f1d31920da042a1c4d6586ca",
          "message": "A class in a file's global `namespace { }` block keeps its global name",
          "timestamp": "2026-08-08T01:48:54+02:00",
          "tree_id": "022eb5b4e7d7bda432fec1f25af135768d0f3faa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cf15caa88aa07575f1d31920da042a1c4d6586ca"
        },
        "date": 1786147493881,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "238413fb1f16d1ab8082e5243baac84ed5ba0bdc",
          "message": "An assignment through a by-reference closure capture is no longer lost",
          "timestamp": "2026-08-08T02:22:32+02:00",
          "tree_id": "31a67529c634f7a5c6a4bc7fae27785861e12afc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/238413fb1f16d1ab8082e5243baac84ed5ba0bdc"
        },
        "date": 1786149541320,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "61a14c406e9ecb21b5abdaf222820d2be41aedac",
          "message": "A re-keying callback rebinds the key type of the collection it returns",
          "timestamp": "2026-08-08T02:25:48+02:00",
          "tree_id": "a0271bb5e01ce642416eef825254d529f30e460c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/61a14c406e9ecb21b5abdaf222820d2be41aedac"
        },
        "date": 1786149751005,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ead9aca89a3840f33ec41b3f77f7a07d01b05579",
          "message": "A standalone `@var` docblock narrows a call inside the same `echo`,\n`if`, or other non-expression statement",
          "timestamp": "2026-08-08T03:04:44+02:00",
          "tree_id": "3ca8b24401c00010c7f7853f0810633a63be976b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ead9aca89a3840f33ec41b3f77f7a07d01b05579"
        },
        "date": 1786152066908,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "770ec875e897c81f9c82fb655b0af5f73fa9257c",
          "message": "A callback body that is a call binds the template it returns",
          "timestamp": "2026-08-08T03:10:58+02:00",
          "tree_id": "294c140e14a7cb2eb760e697176e1fbaed7449e9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/770ec875e897c81f9c82fb655b0af5f73fa9257c"
        },
        "date": 1786152510694,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6a7a6b19b17c05c95e45543587550b56fb81359d",
          "message": "A static factory's method-level template survives into a directly\nchained call",
          "timestamp": "2026-08-08T03:26:47+02:00",
          "tree_id": "719d61580e3c0a4bc3391718cc2549d842c93e6b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6a7a6b19b17c05c95e45543587550b56fb81359d"
        },
        "date": 1786153373020,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1b6326664a61dc84fc3a4958e95051416a1a4e59",
          "message": "An array literal argument binds a union parameter hint's element type\ntoo",
          "timestamp": "2026-08-08T04:07:49+02:00",
          "tree_id": "19de2dd3bf99356637d0fb0eb508d472bb3cfe01",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1b6326664a61dc84fc3a4958e95051416a1a4e59"
        },
        "date": 1786155770860,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "27fc2c19c7e28319b404defb9b7d217229434c7a",
          "message": "`analyze` reports the same diagnostics on every run",
          "timestamp": "2026-08-08T04:17:56+02:00",
          "tree_id": "4d1c83baedcdc73839d98bd4474cd8cb299a81e8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/27fc2c19c7e28319b404defb9b7d217229434c7a"
        },
        "date": 1786156562574,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5461d72c682831afd6f5477919266a21cbe10f97",
          "message": "Add issues",
          "timestamp": "2026-08-08T05:23:49+02:00",
          "tree_id": "a9c42063a5288fdb1514ffe57e84e8c63fb1709c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5461d72c682831afd6f5477919266a21cbe10f97"
        },
        "date": 1786160353785,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "99b5e44b37003a5e1be5ed0e75976e53e2750a2f",
          "message": "Update priority tasks",
          "timestamp": "2026-08-08T05:36:39+02:00",
          "tree_id": "2da42c4415ae172b5b447f3dcb4157d31acfede3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/99b5e44b37003a5e1be5ed0e75976e53e2750a2f"
        },
        "date": 1786161189981,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e6ce00540fc603f6242ccd1fef5f83ce49b6606b",
          "message": "A container call through the `App` facade resolves when it is chained\ndirectly",
          "timestamp": "2026-08-08T05:40:25+02:00",
          "tree_id": "25ba27d4d5392e6e4d5092d19e39a342272b7468",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6ce00540fc603f6242ccd1fef5f83ce49b6606b"
        },
        "date": 1786161428873,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2c6e156f4b947dd47ca90012ce9dcd4d90cdd73a",
          "message": "A callback parameter is typed when the array argument is an inline call\nto an array function",
          "timestamp": "2026-08-08T06:04:06+02:00",
          "tree_id": "bf099476047cc1c155640b974c4c09bf09ab410e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2c6e156f4b947dd47ca90012ce9dcd4d90cdd73a"
        },
        "date": 1786162829811,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1c9a28f360fa9ded888570b22ff613c4ab808e77",
          "message": "Implement support for HashMap aliases",
          "timestamp": "2026-08-08T14:20:25+02:00",
          "tree_id": "e73aa4d2dd39e9c939999d879e569dbb3e0b9ca2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1c9a28f360fa9ded888570b22ff613c4ab808e77"
        },
        "date": 1786192601797,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0e93c97f3f39e66a099cf9dd38d03a16636f884a",
          "message": "A Blade template's variables come from one declared priority chain",
          "timestamp": "2026-08-08T14:24:03+02:00",
          "tree_id": "7e720707d11f70724ba31cfd3b3e3d6f6828a82f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0e93c97f3f39e66a099cf9dd38d03a16636f884a"
        },
        "date": 1786193158960,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "39ced521f7bb94ddf06e53d0b07bc8adc288fc63",
          "message": "Prioritize application's container bindings",
          "timestamp": "2026-08-08T15:19:02+02:00",
          "tree_id": "0e3759225437ac5cd82cbce13d310b9dc254dfad",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/39ced521f7bb94ddf06e53d0b07bc8adc288fc63"
        },
        "date": 1786196103570,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "12577cc07043a4f685221604ba89a814b67eae06",
          "message": "A `@var` whose type is a closure signature binds the right variable",
          "timestamp": "2026-08-08T15:53:28+02:00",
          "tree_id": "12356b22ea97268eaa2cf695f0d75fb4bd1eb894",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/12577cc07043a4f685221604ba89a814b67eae06"
        },
        "date": 1786198286796,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "721c00ad5c05550a419a7490c9b15e13fd0dbc95",
          "message": "A dotted container key no longer resolves to a class named after its\nfirst segment",
          "timestamp": "2026-08-08T16:10:55+02:00",
          "tree_id": "961d7eb9120b77adc0e1522c13ceab14e943d5b6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/721c00ad5c05550a419a7490c9b15e13fd0dbc95"
        },
        "date": 1786199344030,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 32.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "388fdcc47160bf140bfec6479f921968ed641a85",
          "message": "Attributes passed to an anonymous component no longer need `@props` just\nto exist",
          "timestamp": "2026-08-08T16:47:20+02:00",
          "tree_id": "0191624f4876e5608b94bdb30e821863980daf8e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/388fdcc47160bf140bfec6479f921968ed641a85"
        },
        "date": 1786201628055,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e5475eb6a52e26c013697fb0831726dccaf90625",
          "message": "A command declared with the `#[Signature]` attribute is a known command\n\nCloses #331",
          "timestamp": "2026-08-08T16:52:14+02:00",
          "tree_id": "9b815b4c2da46f4f961f9bae2bc8767edb096ff4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e5475eb6a52e26c013697fb0831726dccaf90625"
        },
        "date": 1786201650246,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7b5936905137ba8eb9438a31b48c30fed476fa12",
          "message": "A command that declares both a signature and `#[AsCommand]` is indexed\nunder the name Artisan answers to",
          "timestamp": "2026-08-08T17:50:56+02:00",
          "tree_id": "5fdb9eb802b47b80d0caa13bdbf093c5c6cb0b8a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7b5936905137ba8eb9438a31b48c30fed476fa12"
        },
        "date": 1786205232614,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "98f7c93959e42dffcbc19cb06fa22edf4b6288c1",
          "message": "Clean up bugs.md",
          "timestamp": "2026-08-08T17:57:56+02:00",
          "tree_id": "99094fb4c23f235150c0b6b0a7fee986738314f2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/98f7c93959e42dffcbc19cb06fa22edf4b6288c1"
        },
        "date": 1786205616680,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c8ecad28f9ca5115d6324e7f51f7a95448cbb29f",
          "message": "A component's own class supplies the variables its view reads",
          "timestamp": "2026-08-08T19:16:38+02:00",
          "tree_id": "5e13fc6ec581ee7826dc8bd856a3240d6f3ea86a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c8ecad28f9ca5115d6324e7f51f7a95448cbb29f"
        },
        "date": 1786210316062,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4798eb0655ae86b2276244ba26339951ed0fbf81",
          "message": "`Storage::disk()` and friends resolve to the concrete adapter instead of\nthe bare contract",
          "timestamp": "2026-08-08T19:18:53+02:00",
          "tree_id": "03ddbcba81b0fa3ec73f0f247dabe1483fddd576",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4798eb0655ae86b2276244ba26339951ed0fbf81"
        },
        "date": 1786210446203,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a06ad96b7fe1c6044d1182772ec931a0d7e43c9d",
          "message": "A custom `Storage::extend()` driver no longer costs the rest of the\nproject its disk type",
          "timestamp": "2026-08-08T19:42:39+02:00",
          "tree_id": "79cfb58f4e8ae6f4a3256b8dc89e6802e6523d52",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a06ad96b7fe1c6044d1182772ec931a0d7e43c9d"
        },
        "date": 1786211915725,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4798eb0655ae86b2276244ba26339951ed0fbf81",
          "message": "`Storage::disk()` and friends resolve to the concrete adapter instead of\nthe bare contract",
          "timestamp": "2026-08-08T19:18:53+02:00",
          "tree_id": "03ddbcba81b0fa3ec73f0f247dabe1483fddd576",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4798eb0655ae86b2276244ba26339951ed0fbf81"
        },
        "date": 1786212041657,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "846ea94bef6cfb7cb6d8970b1ce005a53cdf8f35",
          "message": "A custom `Storage::extend()` driver no longer costs the rest of the\nproject its disk type",
          "timestamp": "2026-08-08T19:59:28+02:00",
          "tree_id": "073ed053e24d09c9ba2b578d12f804ce097dd782",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/846ea94bef6cfb7cb6d8970b1ce005a53cdf8f35"
        },
        "date": 1786212958683,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "92f742e808919fa4bce6d4d7aa5134d457e70986",
          "message": "Variable resolution no longer recurses when building the top-level scope\nfor `global`",
          "timestamp": "2026-08-08T20:51:52+02:00",
          "tree_id": "fdb9991b8c6505ee173e5d5d62c567185823cc13",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/92f742e808919fa4bce6d4d7aa5134d457e70986"
        },
        "date": 1786216046653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b86a74609b66a6354b4e5c18ebd08df57aaf2857",
          "message": "View names and component tags resolve through one project-wide index",
          "timestamp": "2026-08-08T21:08:27+02:00",
          "tree_id": "153eda49bcb70edb46c3a6bbb5c97b478f524874",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b86a74609b66a6354b4e5c18ebd08df57aaf2857"
        },
        "date": 1786217096550,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2e43a487d3c4e62560627377d4420aa50a4eac20",
          "message": "Update plan regarding facades",
          "timestamp": "2026-08-08T21:25:26+02:00",
          "tree_id": "180d2be1eeae479b177121da227b80add2ad29f3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2e43a487d3c4e62560627377d4420aa50a4eac20"
        },
        "date": 1786218075473,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "01e8df6d80f56d2105bb99c2070cafef62e10bbc",
          "message": "Shared and composed template variables come from the provider that\nregisters them",
          "timestamp": "2026-08-08T21:49:38+02:00",
          "tree_id": "3a27315ac813ad69c49f566cb51d80df6a35c593",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/01e8df6d80f56d2105bb99c2070cafef62e10bbc"
        },
        "date": 1786219591223,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "436a1d8f0b5a3bd715f33db6df48cebea0b08ffb",
          "message": "`$this` in a Livewire view is the component",
          "timestamp": "2026-08-08T22:20:16+02:00",
          "tree_id": "ba34d3ab46800011b3796b761056616614bb067a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/436a1d8f0b5a3bd715f33db6df48cebea0b08ffb"
        },
        "date": 1786221405460,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b56702680bcd2d4e9456aad36fb906a3b4211059",
          "message": "A layout's declared variables reach the templates that extend it",
          "timestamp": "2026-08-08T22:51:02+02:00",
          "tree_id": "3aa4f64a96abf9ece8a32ad33f38660d94ea9b4c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b56702680bcd2d4e9456aad36fb906a3b4211059"
        },
        "date": 1786223198008,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "38a84f32c6cefb004d72f6ee36cc5451e15fe476",
          "message": "`keyBy()`, `groupBy()`, and `mapWithKeys()` rebind a collection\nsubclass's key type correctly",
          "timestamp": "2026-08-08T22:50:41+02:00",
          "tree_id": "432e291ebd315a1bd7b79dfcdcc6568acad63513",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/38a84f32c6cefb004d72f6ee36cc5451e15fe476"
        },
        "date": 1786223244399,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "19a6c840afc807c6e249949d60d878a3264d7799",
          "message": "An app-defined facade lists the members it actually forwards",
          "timestamp": "2026-08-08T23:30:11+02:00",
          "tree_id": "d036b02f1ef3e480edbc24fb22589e41e379d66c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/19a6c840afc807c6e249949d60d878a3264d7799"
        },
        "date": 1786225609243,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e85863c146f18296d14fe8f0b27b257ae462030c",
          "message": "`view()` calls are checked against the template's declared contract",
          "timestamp": "2026-08-09T00:10:16+02:00",
          "tree_id": "4d377b466bcfdd619d6f687c8b8a334ec7602232",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e85863c146f18296d14fe8f0b27b257ae462030c"
        },
        "date": 1786227957527,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e1af0d0511f59cb3a89814a0088359037820b850",
          "message": "A facade that names a container binding lists its members too",
          "timestamp": "2026-08-09T00:13:01+02:00",
          "tree_id": "63fe06a24cb304258d105f44b5f5a7a0398d6506",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e1af0d0511f59cb3a89814a0088359037820b850"
        },
        "date": 1786228195921,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "729ff2e4abeb30c4731db77f3ae1db2bb4ff5a8e",
          "message": "Renaming a class no longer writes into Blade templates that merely\nreceive it",
          "timestamp": "2026-08-09T00:41:33+02:00",
          "tree_id": "28db8fc47b8dc86849db15017a3c9915004945d9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/729ff2e4abeb30c4731db77f3ae1db2bb4ff5a8e"
        },
        "date": 1786229879149,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0dbdeab138a9116a54511f5864eedcb3d3b3cebe",
          "message": "String concatenation can be rewritten as an interpolated string",
          "timestamp": "2026-08-09T00:58:44+02:00",
          "tree_id": "caa21fafc9e0511f6e8e8ba10c97c44c43248363",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0dbdeab138a9116a54511f5864eedcb3d3b3cebe"
        },
        "date": 1786230919612,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "729ff2e4abeb30c4731db77f3ae1db2bb4ff5a8e",
          "message": "Renaming a class no longer writes into Blade templates that merely\nreceive it",
          "timestamp": "2026-08-09T00:41:33+02:00",
          "tree_id": "28db8fc47b8dc86849db15017a3c9915004945d9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/729ff2e4abeb30c4731db77f3ae1db2bb4ff5a8e"
        },
        "date": 1786230963636,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e6329b3248108d3fc5822c697f69186f2a2b186d",
          "message": "A layout chosen with `@extendsFirst` is no longer invisible",
          "timestamp": "2026-08-09T01:04:47+02:00",
          "tree_id": "0330f94239a55d028fa4b5ff7af5e0b7ecaa7145",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6329b3248108d3fc5822c697f69186f2a2b186d"
        },
        "date": 1786231295595,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "185585ab3387204b123808eb38be54f140c44266",
          "message": "String concatenation can be rewritten as an interpolated string",
          "timestamp": "2026-08-09T01:09:22+02:00",
          "tree_id": "4b2c47d9aba1d76b8ea9ba8bf40b9652a9d9a218",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/185585ab3387204b123808eb38be54f140c44266"
        },
        "date": 1786231468214,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d67b7c54339bed277a93d0b8ea952adb70d94184",
          "message": "A callback's return type binds every template it names, not the first\none only",
          "timestamp": "2026-08-09T02:20:50+02:00",
          "tree_id": "dccd016e1c097c67b4a8bd32500e08ea6bc52dd5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d67b7c54339bed277a93d0b8ea952adb70d94184"
        },
        "date": 1786235808498,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2028223f43734d7284cde78f2350f644b13158c5",
          "message": "`@each` binds its item and key like a `foreach` does",
          "timestamp": "2026-08-09T02:23:35+02:00",
          "tree_id": "2f89dd59e3e1a8d49de5506bf30a4f16b3fe5efe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2028223f43734d7284cde78f2350f644b13158c5"
        },
        "date": 1786235984323,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "13e5a73fe92103b2b20cf604f97c2c40f90a851c",
          "message": "A component no longer excuses its callers from passing what its\nframework base class happens to declare",
          "timestamp": "2026-08-09T02:52:10+02:00",
          "tree_id": "a4712fb4f3406dcb7227771bf2ee9e1409175fa2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/13e5a73fe92103b2b20cf604f97c2c40f90a851c"
        },
        "date": 1786237729257,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5579d94d999819eebd7aa12ae1c33ca12f9693f0",
          "message": "A test navigates to the code it covers",
          "timestamp": "2026-08-09T03:09:02+02:00",
          "tree_id": "5422ad13a7430906d312774085a7ea79481cc3b2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5579d94d999819eebd7aa12ae1c33ca12f9693f0"
        },
        "date": 1786238645280,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1d90173a440df6eefab419b470343268fa4c110a",
          "message": "Update roadmap",
          "timestamp": "2026-08-09T03:51:32+02:00",
          "tree_id": "f67993985c339e876da55bdd19a8c88532084d95",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1d90173a440df6eefab419b470343268fa4c110a"
        },
        "date": 1786241259386,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 84.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1a00142015708c7b0bf71945521ffbb448b2abe4",
          "message": "A mailable's template is a render site like any other",
          "timestamp": "2026-08-09T04:15:51+02:00",
          "tree_id": "57db4f38e537aa654f77da9b3488da0c1710d4d9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1a00142015708c7b0bf71945521ffbb448b2abe4"
        },
        "date": 1786242766579,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7702d511cebb5f78213ced9f4231edc884246fdb",
          "message": "An `@include` is checked against the type the surrounding template holds",
          "timestamp": "2026-08-09T04:45:49+02:00",
          "tree_id": "dabbeb0594781bb598ead1dc8bef2fba9448ae9e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7702d511cebb5f78213ced9f4231edc884246fdb"
        },
        "date": 1786244525882,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "86201b0c9b71c75ec85abc22c692421fef375b38",
          "message": "A template's signature is held to the layouts it renders through",
          "timestamp": "2026-08-09T04:49:21+02:00",
          "tree_id": "48bc2e5d2089636a24fc94667c205c90dd54bf2e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/86201b0c9b71c75ec85abc22c692421fef375b38"
        },
        "date": 1786244790481,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e212c99d970d582fdd63f13afcd30169f6bc7492",
          "message": "Add blade tests",
          "timestamp": "2026-08-09T05:02:39+02:00",
          "tree_id": "bafee611d056922f7409331ef96dd8a521672a80",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e212c99d970d582fdd63f13afcd30169f6bc7492"
        },
        "date": 1786245535256,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4259ce2be0848c05771dfced585e1427049dc9e1",
          "message": "A render site is recognised by what its receiver is, not only by how it\nis spelled",
          "timestamp": "2026-08-09T05:28:51+02:00",
          "tree_id": "6d08ded64e9ee6c5484eef227e330031583109aa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4259ce2be0848c05771dfced585e1427049dc9e1"
        },
        "date": 1786247264457,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "de42964f0931cc7a26e6c0ab7af78a72b68d7238",
          "message": "Add blade tests",
          "timestamp": "2026-08-09T05:40:54+02:00",
          "tree_id": "3a9aa71392429225af606b70b1dc74b16b9210f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/de42964f0931cc7a26e6c0ab7af78a72b68d7238"
        },
        "date": 1786247805536,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a212cb70d2364454dffa6853b3d92e4cb0ca5439",
          "message": "Clean up blade roadmap",
          "timestamp": "2026-08-09T05:42:45+02:00",
          "tree_id": "9df2d2904c01750a9bca7212489e4c95f470bd1b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a212cb70d2364454dffa6853b3d92e4cb0ca5439"
        },
        "date": 1786247954407,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "40b372fb14d1d9678e0d3a364576bdbca624de6b",
          "message": "Update roadmap",
          "timestamp": "2026-08-09T06:11:44+02:00",
          "tree_id": "fbd1dd71c9cc564985ab3c6fa32afbbda8321a62",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/40b372fb14d1d9678e0d3a364576bdbca624de6b"
        },
        "date": 1786249670464,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c19b9c5dbd5b47b96b82ee94339108f0089610de",
          "message": "Fix a stall and 3 minor bugs",
          "timestamp": "2026-08-09T15:54:42+02:00",
          "tree_id": "547a3b4530ad7e278a61144b4aed236465045132",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c19b9c5dbd5b47b96b82ee94339108f0089610de"
        },
        "date": 1786284638898,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "99ed2eed3741ad8d089518e4dc4fb9e6cbefdedf",
          "message": "Blade directive-name completion",
          "timestamp": "2026-08-09T15:57:58+02:00",
          "tree_id": "85fbe2c03b1aa41cb1853310c331512c04f0491f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/99ed2eed3741ad8d089518e4dc4fb9e6cbefdedf"
        },
        "date": 1786284810285,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2dc6bba36b43c974d6b1550d5dc731ec1c54b704",
          "message": "A component addressed through a registered anonymous prefix matches its\ncall sites",
          "timestamp": "2026-08-09T16:48:38+02:00",
          "tree_id": "95800655c12455383414943f1180d30d38d83577",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2dc6bba36b43c974d6b1550d5dc731ec1c54b704"
        },
        "date": 1786287851462,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "80fcae8b9ec5c34a9dc6a9fffd887ba67243f025",
          "message": "A layout's variable is not the caller's to pass when the layout itself\nis where it comes from",
          "timestamp": "2026-08-09T16:57:00+02:00",
          "tree_id": "e8caa8f8c957f71e97e85c31ce427a865b056750",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/80fcae8b9ec5c34a9dc6a9fffd887ba67243f025"
        },
        "date": 1786288418863,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 84.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4b26466c2289b62c15c94f56da938365f856c5a3",
          "message": "Update Blade support ratings",
          "timestamp": "2026-08-09T18:01:17+02:00",
          "tree_id": "d47a2349e62e0b4c3dee77b6717f172b9ec64a98",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4b26466c2289b62c15c94f56da938365f856c5a3"
        },
        "date": 1786292218673,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "01db96655b5075d48922cff47167219e6bd65569",
          "message": "An inline `@php(…)` no longer hides the rest of the template",
          "timestamp": "2026-08-09T18:01:41+02:00",
          "tree_id": "7bfb9fd001b52a4abe9a0954a51a2d61ef8f4aad",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/01db96655b5075d48922cff47167219e6bd65569"
        },
        "date": 1786292281140,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e183625bad43046be1db839e8d23659fccfc6abc",
          "message": "Fix docs",
          "timestamp": "2026-08-09T18:14:47+02:00",
          "tree_id": "ac3648cef1a9b3200f564b7e084a4045fe20fcb4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e183625bad43046be1db839e8d23659fccfc6abc"
        },
        "date": 1786293054261,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fb1988ed5ef287942395aec6dc9d80453ce0619e",
          "message": "Every Blade directive Laravel ships is now recognised, and one where its\narguments were going untyped is type-checked",
          "timestamp": "2026-08-09T18:21:39+02:00",
          "tree_id": "4cf3346f409a72102dd95fe6e001d870f4bc6f65",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fb1988ed5ef287942395aec6dc9d80453ce0619e"
        },
        "date": 1786293495842,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2c5f5870bd6b7d57ba09a0744867fcd9e4bd1620",
          "message": "Fix code lenses not opening the file in some editors",
          "timestamp": "2026-08-09T18:30:49+02:00",
          "tree_id": "acae3ba02e339617f8616dc38fcd65d0fffc82fe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2c5f5870bd6b7d57ba09a0744867fcd9e4bd1620"
        },
        "date": 1786294042116,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "46faa3f7fa0f286859560514976864706baee098",
          "message": "Fix non-php livewire attribute handeling",
          "timestamp": "2026-08-09T18:49:50+02:00",
          "tree_id": "8d6818e8d59a99d64bec842393673c06cc8af1fb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/46faa3f7fa0f286859560514976864706baee098"
        },
        "date": 1786295156323,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b214c5347ff5d20c5e9fe39b6519233b63249c43",
          "message": "A declaration's reference count is now its own",
          "timestamp": "2026-08-09T19:34:54+02:00",
          "tree_id": "1d0da76a1eea58160e45f2b2f0080842162f1eb6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b214c5347ff5d20c5e9fe39b6519233b63249c43"
        },
        "date": 1786297823542,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8ff7937638a1dc712fbfcebe0ecc822643c7e187",
          "message": "Improve blade analasis performance",
          "timestamp": "2026-08-09T19:36:52+02:00",
          "tree_id": "80f3af749ffdd6a1c88eccbb71c87cb158e3d8a7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ff7937638a1dc712fbfcebe0ecc822643c7e187"
        },
        "date": 1786297981321,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8af690941a0993bc09a3b9190639f155bdc10368",
          "message": "A Blade template's variable type no longer changes shape between runs\nwhen more than one call site passes it",
          "timestamp": "2026-08-09T19:55:46+02:00",
          "tree_id": "1817f7001117283940e5eb1295cfe3c5f4ca65e1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8af690941a0993bc09a3b9190639f155bdc10368"
        },
        "date": 1786299141282,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f16c6f9d6c23514319d6882046a0e30cbb88f02a",
          "message": "Fix edge cases",
          "timestamp": "2026-08-09T21:10:04+02:00",
          "tree_id": "69706c4c69396b80cd55095e2672d9a923fab789",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f16c6f9d6c23514319d6882046a0e30cbb88f02a"
        },
        "date": 1786303454495,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0668137c44eb176edd012a3c5bc81b2197d73e13",
          "message": "Factory count-conditional return types",
          "timestamp": "2026-08-09T21:48:56+02:00",
          "tree_id": "0792f620e3c079e17757d4cf939373a1519a8967",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0668137c44eb176edd012a3c5bc81b2197d73e13"
        },
        "date": 1786305908882,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "1242225+sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "af8bda98ab29494194167b702dfd67fb7a97947a",
          "message": "fix(laravel): prevent migration discovery from hanging the server",
          "timestamp": "2026-08-09T22:05:17+02:00",
          "tree_id": "1f8d1a052dca45e341e449cf34715eb0520ac02b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/af8bda98ab29494194167b702dfd67fb7a97947a"
        },
        "date": 1786306895048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a6094c816218350d9ae200ccf5b503f47d247079",
          "message": "A branch PHPantom cannot type widens the answer instead of disappearing\nfrom it",
          "timestamp": "2026-08-09T22:34:14+02:00",
          "tree_id": "b47805ddee5d7ef8a0deae592a6bc1dba481db05",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a6094c816218350d9ae200ccf5b503f47d247079"
        },
        "date": 1786308904667,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "krist7599555@gmail.com",
            "name": "Krist Ponpairin",
            "username": "krist7599555"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "9ee47697de8747ef5b9b082ba97bbc485fcbcca4",
          "message": "fix(laravel): index commands registered via withCommands() and their aliases",
          "timestamp": "2026-08-09T22:41:04+02:00",
          "tree_id": "d8758683b9d5ccb515c0648daa9bf24ddd43aa84",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9ee47697de8747ef5b9b082ba97bbc485fcbcca4"
        },
        "date": 1786309052922,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "eedf8638528bda331a1d78647609b383bb9d281d",
          "message": "A `Blueprint` macro's columns are no longer missing until you edit a\nmigration",
          "timestamp": "2026-08-09T23:16:25+02:00",
          "tree_id": "9d3e83d7dac15d26b1019b54508ac2fd0a573f33",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eedf8638528bda331a1d78647609b383bb9d281d"
        },
        "date": 1786311185147,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "626a783e310562dd10658bc6845c9a85299caf8a",
          "message": "Gate ability and policy strings",
          "timestamp": "2026-08-09T23:22:01+02:00",
          "tree_id": "5c9948a8ac875ca78b8c0bf06e97c5076cc9302d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/626a783e310562dd10658bc6845c9a85299caf8a"
        },
        "date": 1786311510522,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "74a8d97dbcd31d53acc2fd13827dca889660753a",
          "message": "Editing a migration in an ignored directory no longer adds tables a\nrestart drops again",
          "timestamp": "2026-08-09T23:42:42+02:00",
          "tree_id": "0f743741c1ea1ae9511613eee57b16276565f117",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/74a8d97dbcd31d53acc2fd13827dca889660753a"
        },
        "date": 1786312750386,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2e609ded0ba9c2130d16e253ec1b736807436207",
          "message": "A project that authorizes through a permission package is no longer told\nevery ability is unknown",
          "timestamp": "2026-08-10T00:18:47+02:00",
          "tree_id": "916461ffc8531a03d640120582eef8971b026902",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2e609ded0ba9c2130d16e253ec1b736807436207"
        },
        "date": 1786314916670,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e6756216148169477c75b8be7e5cda5973fafdb3",
          "message": "A template's block directives are checked against each other",
          "timestamp": "2026-08-10T00:55:48+02:00",
          "tree_id": "9b701d019a1a623ddcad6691f13bba4f003da3b7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6756216148169477c75b8be7e5cda5973fafdb3"
        },
        "date": 1786317138770,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "da0cb4fce4bdd1440d63e7146b644bca2f1463e4",
          "message": "A component tag is a call, and its attributes are checked as arguments",
          "timestamp": "2026-08-10T02:26:33+02:00",
          "tree_id": "495cebf8e933f68f097d0e45b26cf6995834ef25",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/da0cb4fce4bdd1440d63e7146b644bca2f1463e4"
        },
        "date": 1786322570482,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b84d06e019e47f680641e9c0187e023cd240443f",
          "message": "A Blade section knows where its other half is",
          "timestamp": "2026-08-10T02:37:17+02:00",
          "tree_id": "669591ccddb9eb6fd61b78fc4d8d971ff7ad4185",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b84d06e019e47f680641e9c0187e023cd240443f"
        },
        "date": 1786323230908,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "20c4197781bf107b7b858d5558c5bb6a3dcb93cd",
          "message": "A component tag leads to the component behind it",
          "timestamp": "2026-08-10T04:26:13+02:00",
          "tree_id": "fdc8cd7a0e380e6029eb08540dda5b2f680b0b4a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/20c4197781bf107b7b858d5558c5bb6a3dcb93cd"
        },
        "date": 1786329784009,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "eef07f9022bb8f0fa06f6702cda6b68223a03d7a",
          "message": "A class shows the tests that cover it",
          "timestamp": "2026-08-10T05:30:02+02:00",
          "tree_id": "9c674ea517fe77fb1e6d3a829a41e1e7a8dbaa17",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eef07f9022bb8f0fa06f6702cda6b68223a03d7a"
        },
        "date": 1786333613766,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a4f75f34ecaed5a14b9093ae6f9537a90087c385",
          "message": "A Blade partial learns its variables from the templates that render it",
          "timestamp": "2026-08-10T05:32:36+02:00",
          "tree_id": "b8d08c492e7cc4141685a4d31998add79c5f6cff",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a4f75f34ecaed5a14b9093ae6f9537a90087c385"
        },
        "date": 1786333725593,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "671b5a02da4485e11b369186ffd76565f993e71a",
          "message": "A check on a method call narrows the call, not just a variable",
          "timestamp": "2026-08-10T07:33:45+02:00",
          "tree_id": "9702408e50e8f38fd997bc22b3558b15aadc209d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/671b5a02da4485e11b369186ffd76565f993e71a"
        },
        "date": 1786341021324,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "e90ef1bcf6579bbb97a11105a14195e5a3cbcf36",
          "message": "fix: store @phpstan-type aliases from trait and enum docblocks\n\nThe trait and enum branches of the class parser were setting\ntype_aliases to AtomMap::default(), discarding the aliases that\nextract_class_docblock() had already parsed. This caused any\n@phpstan-type alias defined on a trait or enum to be reported as\n\"Class not found\" by the unknown_class diagnostic.\n\nClasses and interfaces already stored doc_info.type_aliases correctly;\nthis fix brings traits and enums in line.",
          "timestamp": "2026-08-10T12:28:18-05:00",
          "tree_id": "a0ea75117fe55cfce21c371b69c70f7d72f7539b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e90ef1bcf6579bbb97a11105a14195e5a3cbcf36"
        },
        "date": 1786383892217,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "d65e982a8b23d0fcb21398430e9d26e7fc13e22b",
          "message": "fix: send inlayHint/refresh after didChange background parse\n\nThe didChange handler sent workspace/semanticTokens/refresh after\ncommitting a new symbol map but did not send\nworkspace/inlayHint/refresh. This left inlay hints (parameter names,\nclosure types, reference counts) showing pre-edit data until the\neditor re-requested them on its own, typically requiring a manual\nfile reload in NeoVim.\n\nNow both refresh notifications are sent together when the background\nparse produces a new symbol map.",
          "timestamp": "2026-08-10T12:46:13-05:00",
          "tree_id": "0b36a0a4be7d2b4d16b635f3e0c28917d71bdb8f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d65e982a8b23d0fcb21398430e9d26e7fc13e22b"
        },
        "date": 1786384965433,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8bb4a3fa6579cdd30d577b5be034d361d6211fe0",
          "message": "Update backlog",
          "timestamp": "2026-08-11T01:00:15+02:00",
          "tree_id": "0c512aadce5c6251d2316074d0601157608c795a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8bb4a3fa6579cdd30d577b5be034d361d6211fe0"
        },
        "date": 1786403842253,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "744bae44ec40ebc81b03616cbe9869ab927815c3",
          "message": "A PHPDoc pseudo-type with a no model found is no longer enforced as\nthough it were a class",
          "timestamp": "2026-08-11T02:31:41+02:00",
          "tree_id": "4dd2001baa89d53d772736bb7db3522ce1dc9db5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/744bae44ec40ebc81b03616cbe9869ab927815c3"
        },
        "date": 1786409295280,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "df3e3f491b344ed97a010bc1a219ea4c833a7534",
          "message": "`key-of<T>` no longer widens to a template's declared bound when the\nargument is an array literal",
          "timestamp": "2026-08-11T02:43:49+02:00",
          "tree_id": "230c45e1c5e8e1c79337411c2ad7af1317ea3ed5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/df3e3f491b344ed97a010bc1a219ea4c833a7534"
        },
        "date": 1786410046029,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ea3201be4ab7934f6b452c5066e4d8faeaf1f40d",
          "message": "A class named `Integer`, `Boolean`, `Double`, or `Resource` is no longer\nread as PHP's scalar alias of the same name",
          "timestamp": "2026-08-11T03:06:31+02:00",
          "tree_id": "64e4f398a21a34671bcac1f145657e7311e318bd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ea3201be4ab7934f6b452c5066e4d8faeaf1f40d"
        },
        "date": 1786411367897,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ab5db42099aaaf52ecef737d5c62e3c7a777dbbc",
          "message": "An `object{prop: Type}` shape now matches a class or object literal that\nactually has that property, instead of rejecting every argument",
          "timestamp": "2026-08-11T03:31:59+02:00",
          "tree_id": "cf15b2abdfe3c0207d0ea3e7a389c5805c9cb8e7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ab5db42099aaaf52ecef737d5c62e3c7a777dbbc"
        },
        "date": 1786412918213,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2dd4eb7b9bb1fe74a69aa2e58e55d3093a1ff49c",
          "message": "`analyze` and `fix` no longer silently drop a `PATH` argument typed\nrelative to the working directory",
          "timestamp": "2026-08-11T03:53:13+02:00",
          "tree_id": "4dc02449e6b1d3e1e78a3f9d319f3211439e65c1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2dd4eb7b9bb1fe74a69aa2e58e55d3093a1ff49c"
        },
        "date": 1786414205859,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8376e3f93a6538ee0693c77c381acec92d1b71c8",
          "message": "A completion item in a Blade template no longer edits a position several\nlines below where the cursor sits",
          "timestamp": "2026-08-11T04:01:57+02:00",
          "tree_id": "706718c3c744f2ddabb66f1b022699b4abe085b3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8376e3f93a6538ee0693c77c381acec92d1b71c8"
        },
        "date": 1786414660636,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "88b066e3efb6ccc758be94b9ccace4fc8e7432f1",
          "message": "Renaming a method no longer rewrites a call whose receiver just happens\nto be named after the class",
          "timestamp": "2026-08-11T04:50:11+02:00",
          "tree_id": "baeb119b4fd254b924d3e9cd6330e8334d320a38",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/88b066e3efb6ccc758be94b9ccace4fc8e7432f1"
        },
        "date": 1786417555012,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3f5c3f087cafd4a32445e5f375cca6c4a786d3a8",
          "message": "A generic class named without its type arguments no longer hands back\nits own template parameter",
          "timestamp": "2026-08-11T04:56:11+02:00",
          "tree_id": "577cd82ace9ff73a99dabe7ef5d15512fdb476bb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3f5c3f087cafd4a32445e5f375cca6c4a786d3a8"
        },
        "date": 1786417989600,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "58940964852fcdfb480ae2cc822fcc8d7d9f43f5",
          "message": "A generic class named without its type arguments no longer hands back\nits own template parameter",
          "timestamp": "2026-08-11T05:10:47+02:00",
          "tree_id": "38747fa3b949fb88802986774ec40d04bd8cb575",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/58940964852fcdfb480ae2cc822fcc8d7d9f43f5"
        },
        "date": 1786418908090,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5a7c7e8aed07f2b4ee17a0c838a865ed54752f80",
          "message": "Better formatting around the table footnotes",
          "timestamp": "2026-08-11T05:23:38+02:00",
          "tree_id": "ad3bf363ced5ac1bbd40452a7f98d0c0a2978f2d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5a7c7e8aed07f2b4ee17a0c838a865ed54752f80"
        },
        "date": 1786419504953,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "caa56651660660b55c0d309a8bedd2e0756815a9",
          "message": "A check on `$a->value` no longer keeps narrowing it after `$a` itself is\nreplaced",
          "timestamp": "2026-08-11T05:31:51+02:00",
          "tree_id": "10bf24e720b2d1552406a9cb3f6d04c4239246c2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/caa56651660660b55c0d309a8bedd2e0756815a9"
        },
        "date": 1786420087581,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f10aedbab1622ed55014ba5c6c517ba0f4edf280",
          "message": "Find References matches a member's receiver by its type, never by its\nname",
          "timestamp": "2026-08-11T16:27:40+02:00",
          "tree_id": "0bbab7c345cf9b1925fb2a00fc931a5a67afe2b0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f10aedbab1622ed55014ba5c6c517ba0f4edf280"
        },
        "date": 1786459456529,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a693996f05c7d0101a1ee57cc27bafc2aaa365c0",
          "message": "`{@see method()}` written inside its own class no longer reads as a\nmissing global function",
          "timestamp": "2026-08-11T16:30:23+02:00",
          "tree_id": "dd9a39a47c80cbc00e54a54ab34a4e74036e87f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a693996f05c7d0101a1ee57cc27bafc2aaa365c0"
        },
        "date": 1786460270002,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c5bfa58e38763ed1cf1072e87ef3961e9804a642",
          "message": "Writes to a `readonly` property",
          "timestamp": "2026-08-11T17:38:12+02:00",
          "tree_id": "f8093ebe45780480eb52079a96ed734d20e3511e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c5bfa58e38763ed1cf1072e87ef3961e9804a642"
        },
        "date": 1786463739420,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d2f73d1635c409f364e6970fa362bac4dc0e9133",
          "message": "Passing a generic class with the wrong type argument is reported",
          "timestamp": "2026-08-11T17:40:25+02:00",
          "tree_id": "f2c624145c93adc385375cfdb3ed5bc68ab54ddb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d2f73d1635c409f364e6970fa362bac4dc0e9133"
        },
        "date": 1786463798170,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "435fe0de336ac5782f69bc6de1e3168e0f575d1d",
          "message": "A `@template` bound from several parameters is what all of them have in\ncommon",
          "timestamp": "2026-08-11T21:46:21+02:00",
          "tree_id": "afd9b32472b200f2988410cff22a0090ed367d39",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/435fe0de336ac5782f69bc6de1e3168e0f575d1d"
        },
        "date": 1786478586431,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6d12e06056496e61e135e4f9c852d28037473cb7",
          "message": "A function declared in two files resolves to the same one on every run",
          "timestamp": "2026-08-11T22:26:40+02:00",
          "tree_id": "0cbd3ddb6e81779513810ceaea4bbcf09198aec7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6d12e06056496e61e135e4f9c852d28037473cb7"
        },
        "date": 1786480924264,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "713f089c7f55f1fda36a53581646af8937d080c3",
          "message": "A class declared in two files survives the file that won it dropping the\nname",
          "timestamp": "2026-08-11T23:31:00+02:00",
          "tree_id": "d5248ca0b1220fb4359401568aa31f052aaca83e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/713f089c7f55f1fda36a53581646af8937d080c3"
        },
        "date": 1786484834839,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "11af7424ee4d7570b1fd9c9d2f323f74588e6db2",
          "message": "A value read out of an array literal keeps the value it was written with",
          "timestamp": "2026-08-12T02:50:51+02:00",
          "tree_id": "6412b388845aa1abd614f2eb82ad5ad2f00caa4d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/11af7424ee4d7570b1fd9c9d2f323f74588e6db2"
        },
        "date": 1786496825594,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8729748ff9c4d3d8895652701076f845dfddc3bc",
          "message": "`interface-string` is held to naming an interface",
          "timestamp": "2026-08-12T03:10:02+02:00",
          "tree_id": "e5625cb79200f05cfb4a79bcafc0a287126df4d4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8729748ff9c4d3d8895652701076f845dfddc3bc"
        },
        "date": 1786498157937,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9b13a77a912c2e66c6a88593a9cab8502303a225",
          "message": "A class name written with an escaped backslash is recognized",
          "timestamp": "2026-08-12T03:47:05+02:00",
          "tree_id": "747b681049e783bd94c3761014c1e3dfca2baa6d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9b13a77a912c2e66c6a88593a9cab8502303a225"
        },
        "date": 1786500213987,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8903dc190bb83ddae9ab3f3f7113dd108837a212",
          "message": "A partially-compatible union argument is now reported",
          "timestamp": "2026-08-12T04:37:14+02:00",
          "tree_id": "5ce88e719da410d530a6184111e1523f989c1c06",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8903dc190bb83ddae9ab3f3f7113dd108837a212"
        },
        "date": 1786503236141,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4b71f79e36d768e45fa28388564f62cc9818ddc1",
          "message": "A closure that returns the wrong thing for a `callable(...)` parameter\nis reported",
          "timestamp": "2026-08-12T05:23:00+02:00",
          "tree_id": "503feadbc175ad4fe3489bc35891260327e1da1f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4b71f79e36d768e45fa28388564f62cc9818ddc1"
        },
        "date": 1786505904979,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "354b45d0a3344085cfd234d794281fee2e16411e",
          "message": "A value proven to be two types at once is one value, not a choice\nbetween them",
          "timestamp": "2026-08-12T05:47:24+02:00",
          "tree_id": "4a25db70c738939eca7e9c938a9f6faa7e8fc63f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/354b45d0a3344085cfd234d794281fee2e16411e"
        },
        "date": 1786507415471,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "737414ec0cb594f341f503a0dfedbbaf84420e25",
          "message": "A literal that mixes positional and keyed entries keeps its positional\nones",
          "timestamp": "2026-08-12T06:27:41+02:00",
          "tree_id": "7dcec2372c6e4ec7082439d1635307f3b4d7ea2e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/737414ec0cb594f341f503a0dfedbbaf84420e25"
        },
        "date": 1786509833527,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0d0d7f6d368574502d7f0ca045a5fa56256b9191",
          "message": "Improve readonly diagnostics",
          "timestamp": "2026-08-12T06:32:34+02:00",
          "tree_id": "0d2761dd6251556bb85f6f722546f7c2a61f84e4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0d0d7f6d368574502d7f0ca045a5fa56256b9191"
        },
        "date": 1786510136979,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "83827fc47a514fe9c4efcf4a13c46e5fbb36317f",
          "message": "An `instanceof` on a nested property keeps the property's declared class",
          "timestamp": "2026-08-12T06:36:03+02:00",
          "tree_id": "c8c8bc40c5ad30aba861985b482888b6a3c01d5b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/83827fc47a514fe9c4efcf4a13c46e5fbb36317f"
        },
        "date": 1786510376927,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f2e670b15923e550d27f77f75d3a313910995090",
          "message": "A check written on a method call reaches the argument that repeats it",
          "timestamp": "2026-08-12T06:50:27+02:00",
          "tree_id": "1dfcd639febf28ddbb7d1cabd0bcaeeaa6fc3ff5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f2e670b15923e550d27f77f75d3a313910995090"
        },
        "date": 1786511245265,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "85db4fe35ffd9501d02f4b2b5da9281fda260a69",
          "message": "Update bug tracker",
          "timestamp": "2026-08-12T07:28:10+02:00",
          "tree_id": "85cc5c2f2336f3a4ee40b07a04560a564ace5462",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/85db4fe35ffd9501d02f4b2b5da9281fda260a69"
        },
        "date": 1786513412660,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "863f3edccf702b00bfc7a33d13a83e53c0fb1197",
          "message": "A native type hint naming a PHPDoc-only pseudo-type is reported",
          "timestamp": "2026-08-12T13:22:24+02:00",
          "tree_id": "f34b91d84e581fc6120105f2fd855fc0f7d1a1bf",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/863f3edccf702b00bfc7a33d13a83e53c0fb1197"
        },
        "date": 1786534737046,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8ed4be9e4d170e106bb631a802bdbdc94ce19cd5",
          "message": "Builtins whose failure branch nobody checks stop being reported",
          "timestamp": "2026-08-12T13:32:56+02:00",
          "tree_id": "ed7ea8a318bdf01d5a6f1ef7d0559d552be95774",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8ed4be9e4d170e106bb631a802bdbdc94ce19cd5"
        },
        "date": 1786535377926,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b59f561d1b794d9345da05c9a19f898ae40348a0",
          "message": "A read loop keeps the narrowing its condition established",
          "timestamp": "2026-08-12T15:14:57+02:00",
          "tree_id": "fbb54ce865c1c15a9e599808dfcf132be02137dd",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b59f561d1b794d9345da05c9a19f898ae40348a0"
        },
        "date": 1786541514858,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "29dd91dd3f6c2fda9bf873b6450ee80ad48f232d",
          "message": "An array filled in over several branches reads as one array",
          "timestamp": "2026-08-12T15:15:39+02:00",
          "tree_id": "60ce674ce71c970d66f4ecd2f02b3c334b8f7240",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/29dd91dd3f6c2fda9bf873b6450ee80ad48f232d"
        },
        "date": 1786541568676,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b58aadeb30ff3b4ccf49182b9a3799c6ea687774",
          "message": "A `foreach` key is typed from what is being iterated",
          "timestamp": "2026-08-12T15:56:09+02:00",
          "tree_id": "a502db0acc8396cdbb01f13f411e9e40eeccd559",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b58aadeb30ff3b4ccf49182b9a3799c6ea687774"
        },
        "date": 1786543978900,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "01682c5160a8d226a3316228998dba82953bc581",
          "message": "A check written beside the assignment it guards narrows the variable",
          "timestamp": "2026-08-12T16:01:37+02:00",
          "tree_id": "73354b1c2931b76304dcb9468e50c38d51e709c7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/01682c5160a8d226a3316228998dba82953bc581"
        },
        "date": 1786544332797,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8f297a42f67bc40b96ec4dba1d9978babc26d14b",
          "message": "A full ternary's own condition now narrows its then branch",
          "timestamp": "2026-08-12T16:14:47+02:00",
          "tree_id": "69aea5adfaaf951fc3c67dacd42ae23af7170e33",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8f297a42f67bc40b96ec4dba1d9978babc26d14b"
        },
        "date": 1786545101370,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7edd5b65b8597cd0964be531f05e3feefaca8485",
          "message": "A value proven to be two types at once is one value, not a choice\nbetween them",
          "timestamp": "2026-08-12T16:40:57+02:00",
          "tree_id": "47f6fd7c175927c46eb6d12a60f7987f2663898d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7edd5b65b8597cd0964be531f05e3feefaca8485"
        },
        "date": 1786546669299,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fa73955050c13c7c896e4ac854d02dacda19314a",
          "message": "Fix Laravel demo",
          "timestamp": "2026-08-12T16:53:23+02:00",
          "tree_id": "a5bb9c82c0271843f16ab473f599da60fe07910a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fa73955050c13c7c896e4ac854d02dacda19314a"
        },
        "date": 1786547419035,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 83.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2b871a127b8550f39429c2b010a4b9a9a5d04de5",
          "message": "A call can retype the variable it was called on",
          "timestamp": "2026-08-12T17:05:53+02:00",
          "tree_id": "aa3cec0b3e76fa2598167f8efa17b3fd1f3b90f6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2b871a127b8550f39429c2b010a4b9a9a5d04de5"
        },
        "date": 1786548116342,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "762c7111305ec76ea46eaae6366adcb3aac2f949",
          "message": "A generic type argument is no longer coerced away",
          "timestamp": "2026-08-12T19:21:41+02:00",
          "tree_id": "c89640e35d8c7aa714dd86de3a723c3a3d1113d7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/762c7111305ec76ea46eaae6366adcb3aac2f949"
        },
        "date": 1786556256417,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "99895c93057a447824595d31ab905fa4cbcb792d",
          "message": "A `@method` tag's own inline template parameter is no longer read as a\nclass name",
          "timestamp": "2026-08-12T19:34:31+02:00",
          "tree_id": "55a57f8de779b8e77cbbaaa533df8e72d1a178b1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/99895c93057a447824595d31ab905fa4cbcb792d"
        },
        "date": 1786557024277,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b4750bc471e0bf59c09e2925ddbc86f8be843082",
          "message": "A lookup into a constant table reads as the entry its key names",
          "timestamp": "2026-08-12T20:03:19+02:00",
          "tree_id": "5ddb11ab344d6402002aa3c6ae6f764fa1acfdac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b4750bc471e0bf59c09e2925ddbc86f8be843082"
        },
        "date": 1786558772863,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "67f468afca4ed948c814c1281531228045bf8928",
          "message": "A `for` loop's update clause carries its type into the next iteration",
          "timestamp": "2026-08-12T21:16:18+02:00",
          "tree_id": "70de391ab67c3c1455210b3559c792dc9fad01d2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/67f468afca4ed948c814c1281531228045bf8928"
        },
        "date": 1786565617295,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fecdec1de731d4b7614907bcaaf92f64c87309bd",
          "message": "An array literal keeps the values it was written with",
          "timestamp": "2026-08-12T22:01:12+02:00",
          "tree_id": "6cf9e517d9cc27ed1d313669bd5b826f67427754",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fecdec1de731d4b7614907bcaaf92f64c87309bd"
        },
        "date": 1786565832668,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "59d43d25beca7f9e126a0318fc2d2daeb9c593ee",
          "message": "A constant table constrains a plain signature too",
          "timestamp": "2026-08-12T22:16:57+02:00",
          "tree_id": "54d35d116d45e7b7389fc565d48e982065b29624",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/59d43d25beca7f9e126a0318fc2d2daeb9c593ee"
        },
        "date": 1786566848368,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b2824dafdfe6396672cabbabd7cf82f7845e8b81",
          "message": "A union of objects is narrowed by a check on the property that tells\nthem apart",
          "timestamp": "2026-08-12T23:04:20+02:00",
          "tree_id": "4f037aabdc83bc7ec2ceb6f1603fc5380f46635c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b2824dafdfe6396672cabbabd7cf82f7845e8b81"
        },
        "date": 1786569620094,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "799d523df0a4ce9ea3c32230e3da8998bd179bba",
          "message": "A guard clause on a property proves the same thing it proves about a\nlocal",
          "timestamp": "2026-08-12T23:05:54+02:00",
          "tree_id": "2f6d38c95d672b8115290f29ad7cb1c394349202",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/799d523df0a4ce9ea3c32230e3da8998bd179bba"
        },
        "date": 1786569747956,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5f9707e4b04360c6b8aa1b97390b1c5db1421248",
          "message": "`assert()` proves whatever the same condition proves inside an `if`",
          "timestamp": "2026-08-13T00:12:13+02:00",
          "tree_id": "a6f040a884da2e1b3bb5f4bba5fc02b5e2da8b1d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5f9707e4b04360c6b8aa1b97390b1c5db1421248"
        },
        "date": 1786573692028,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7cec79a45300520973374e4577147b5c0bf397cc",
          "message": "A scalar check on an argument-less method call narrows the call the same\nway it narrows a property",
          "timestamp": "2026-08-13T00:13:30+02:00",
          "tree_id": "9b620fbe6ccebd917df2ca8c038649d29a650fef",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7cec79a45300520973374e4577147b5c0bf397cc"
        },
        "date": 1786573861571,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e6e7c1b38f121413b8cf9bc17732055f65148a27",
          "message": "A `false` check narrows its `else` branch too, not just the guard clause\nthat returns",
          "timestamp": "2026-08-13T01:16:27+02:00",
          "tree_id": "ec0826c468aa6f528efcde7ee93f8e11b548ea29",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6e7c1b38f121413b8cf9bc17732055f65148a27"
        },
        "date": 1786577591544,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6b52b2ab87c52b29ffdb4d2f5d0b5a9edcfa3ce7",
          "message": "Consistent decleration handeling",
          "timestamp": "2026-08-13T01:22:12+02:00",
          "tree_id": "df76ec4a83eea04c93ca5f5cf926afa223f480c8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6b52b2ab87c52b29ffdb4d2f5d0b5a9edcfa3ce7"
        },
        "date": 1786577936765,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a8db6b7d8380b4c7c9325ae4bdf852c927a60836",
          "message": "A return type that depends on an argument's value is read from the value\npassed, or from the argument's default when it is left out",
          "timestamp": "2026-08-13T01:44:12+02:00",
          "tree_id": "41d966df7b04a8919cc2ae547289b5a373f0e9fb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a8db6b7d8380b4c7c9325ae4bdf852c927a60836"
        },
        "date": 1786579219557,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d672a3aa747f173d78c90a1be86c3f8f030404fe",
          "message": "A tag written on a docblock's opening line is read",
          "timestamp": "2026-08-13T02:02:27+02:00",
          "tree_id": "1093ef325b1a6d2d87ed8f874df9a16398d1ded3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d672a3aa747f173d78c90a1be86c3f8f030404fe"
        },
        "date": 1786580317535,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 19.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 66.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fb3b33233b0353b3a38386fc4d34ca2126a95943",
          "message": "`?->` on a `null` subject is no longer reported as a crash",
          "timestamp": "2026-08-13T03:03:04+02:00",
          "tree_id": "cb15b20e33a4e293a925dcbcbd712ab80e160abb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fb3b33233b0353b3a38386fc4d34ca2126a95943"
        },
        "date": 1786583999562,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3896704b98fdc6e7f9fa2866d1abe17dfaa14407",
          "message": "An array literal whose keys are out of order no longer passes for a list",
          "timestamp": "2026-08-13T03:44:20+02:00",
          "tree_id": "809ffcd8bbf72453248ba562736bf89c274a5071",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3896704b98fdc6e7f9fa2866d1abe17dfaa14407"
        },
        "date": 1786586471719,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a9eb1664ca10b4b1d7fc352881c13f65f89e6dee",
          "message": "An indexed collection keeps its element type after a check on it",
          "timestamp": "2026-08-13T03:52:37+02:00",
          "tree_id": "c128fcb7a065fc917b0db1ddcbb7250d52158efc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a9eb1664ca10b4b1d7fc352881c13f65f89e6dee"
        },
        "date": 1786586917301,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "dea901e1c653526643cf730e686994a89529a501",
          "message": "An omitted argument reads a constant table under the key its own default\nnames",
          "timestamp": "2026-08-13T04:03:56+02:00",
          "tree_id": "2e00962e3f4df513a724578cf16b6750e9bc0ecc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dea901e1c653526643cf730e686994a89529a501"
        },
        "date": 1786587624292,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9f2441424f475743635d4a9a360fb995de799b46",
          "message": "A docblock that contradicts its own signature",
          "timestamp": "2026-08-13T04:17:51+02:00",
          "tree_id": "b68e336db3b7dd2b52ad52e86810396c3f1061c1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9f2441424f475743635d4a9a360fb995de799b46"
        },
        "date": 1786588524297,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cbd271bafbe3e8d1194d9ab0358ada4269d9464a",
          "message": "Implement order imports, and clean up roadmap",
          "timestamp": "2026-08-13T04:53:38+02:00",
          "tree_id": "9c3fd9a01c3ef134fe1f30d4b9c8fe6ae13c172d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cbd271bafbe3e8d1194d9ab0358ada4269d9464a"
        },
        "date": 1786590580480,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a0de679aea5ed9bf9edfe4b852da57806f3104a3",
          "message": "Improve str_replace and json_encode return types",
          "timestamp": "2026-08-13T05:11:19+02:00",
          "tree_id": "a63c16d39b096a82d933725696bc9d1fe1807aa1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a0de679aea5ed9bf9edfe4b852da57806f3104a3"
        },
        "date": 1786591782147,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f38a38f811c50f9cde5429b08c6a2170e35ee262",
          "message": "Don't format blade files as PHP",
          "timestamp": "2026-08-13T05:40:06+02:00",
          "tree_id": "297fbb6ba0b37cc7f35b5d2dfea35558710ae137",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f38a38f811c50f9cde5429b08c6a2170e35ee262"
        },
        "date": 1786593465400,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 19.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 64,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0c4c13d5a1fbf1033e7119b68465f623f09a9219",
          "message": "A fully-qualified type-guard call narrows like its unqualified spelling",
          "timestamp": "2026-08-13T05:49:54+02:00",
          "tree_id": "15634757a5a9d4883343999aaa5800846aa06b21",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0c4c13d5a1fbf1033e7119b68465f623f09a9219"
        },
        "date": 1786593971601,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bff2a822774e8833c4fa80fd05ca1186703faf89",
          "message": "An argument written as an array element, a global constant, or a simple\noperator expression is no longer read as nothing",
          "timestamp": "2026-08-13T06:09:17+02:00",
          "tree_id": "61fbb4269e1c725da2784ecae8bd2b7e9ff4bfc8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bff2a822774e8833c4fa80fd05ca1186703faf89"
        },
        "date": 1786595171016,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "eeafc088f8da82d18b0f6c8084aad6027ec0888d",
          "message": "An `instanceof` check rules out the array half of a union, not just the\nother class",
          "timestamp": "2026-08-13T06:37:22+02:00",
          "tree_id": "008c6e12a8c6040bfd6d4ba856c864dfbcc48829",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eeafc088f8da82d18b0f6c8084aad6027ec0888d"
        },
        "date": 1786596816259,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "aea3557f8b7443ce8a4e9a5359b213b5807f6912",
          "message": "A guard that exits by calling a `never` method ends the branch whatever\nthe call is written on",
          "timestamp": "2026-08-13T07:13:23+02:00",
          "tree_id": "f76854c36f360914f39708521518d714a66f916d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/aea3557f8b7443ce8a4e9a5359b213b5807f6912"
        },
        "date": 1786599009031,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "26dc34e0cfc15f0b5d8b2ef20818719fddfe97ef",
          "message": "Fix a handful of issues",
          "timestamp": "2026-08-13T20:10:32+02:00",
          "tree_id": "aeb75d501aef4626343ea71a401227e708ebfa46",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/26dc34e0cfc15f0b5d8b2ef20818719fddfe97ef"
        },
        "date": 1786645644965,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0dc8de0b0b4968d13854a633e993c8c72d3e7eac",
          "message": "Fix a few issues",
          "timestamp": "2026-08-13T21:19:53+02:00",
          "tree_id": "e71d4cbd896f95c63e13c3732038e3ab98010104",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0dc8de0b0b4968d13854a633e993c8c72d3e7eac"
        },
        "date": 1786649799339,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 83.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "02f1a0dead62b47810171c02c31e99f1334a900f",
          "message": "Fix a few issues",
          "timestamp": "2026-08-13T22:23:46+02:00",
          "tree_id": "255df871b5d617f1d524040aa2bb4e5f6d7f7325",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/02f1a0dead62b47810171c02c31e99f1334a900f"
        },
        "date": 1786653623496,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7fdbc6e89e78e6939573c2553bc98c57655c7f5d",
          "message": "Fiew a few issues",
          "timestamp": "2026-08-13T23:39:55+02:00",
          "tree_id": "7b8cb69b0470582fdf21d6b4ffcb2260da1782e8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7fdbc6e89e78e6939573c2553bc98c57655c7f5d"
        },
        "date": 1786658185653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d3b58bb6b49fe54f6294799f61a8a204cc4723ad",
          "message": "Fix a couple of bugs",
          "timestamp": "2026-08-14T01:02:01+02:00",
          "tree_id": "b1143997a7565fed1a173744bbeef3c2a91dcc5f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d3b58bb6b49fe54f6294799f61a8a204cc4723ad"
        },
        "date": 1786663132729,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "df4766c3afc5eac54b17a3412bccf9d1c1f5c4e4",
          "message": "A parameter default written `self::SOME_CONST` resolves against the\nclass that declares it, not the caller's",
          "timestamp": "2026-08-14T01:31:48+02:00",
          "tree_id": "7a2676371991612b0c57b7635c7bc6e67bc3ad09",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/df4766c3afc5eac54b17a3412bccf9d1c1f5c4e4"
        },
        "date": 1786664971300,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "228faa0d6c90fe6d4a8f216c030bb5e5e40d3803",
          "message": "Refactor demos",
          "timestamp": "2026-08-14T03:44:43+02:00",
          "tree_id": "a1ddbf9799ce1d2f12b2737f47dc36bed0e5ba5b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/228faa0d6c90fe6d4a8f216c030bb5e5e40d3803"
        },
        "date": 1786672874008,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "158b7be561ad6d9d9341b4ab68b24ee546bd858c",
          "message": "Fix a few issues",
          "timestamp": "2026-08-14T03:46:36+02:00",
          "tree_id": "4352bcd31481f74412936762611ff42eab0092c9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/158b7be561ad6d9d9341b4ab68b24ee546bd858c"
        },
        "date": 1786672956232,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "72f4a10927cc9a967bd48d765c02f98c492f86c9",
          "message": "Fix a few issues",
          "timestamp": "2026-08-14T04:24:12+02:00",
          "tree_id": "5315a901356769fe3b44ee1c43309f40bf6b7223",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/72f4a10927cc9a967bd48d765c02f98c492f86c9"
        },
        "date": 1786675260308,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "40a8e074fea8032475a77577e401933b83c951a5",
          "message": "Fix argument types at call sites",
          "timestamp": "2026-08-14T05:30:38+02:00",
          "tree_id": "660c5a2d74152b0385984a867d05bf3d2978d3ac",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/40a8e074fea8032475a77577e401933b83c951a5"
        },
        "date": 1786679223123,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e3136d1b8e520d1ada2b3c82c7f3565a2b0bfe3d",
          "message": "Correct scope merge semantics issues",
          "timestamp": "2026-08-14T05:38:25+02:00",
          "tree_id": "14bbaf93fcd826549c88147f3eae2a1a91cb21d1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e3136d1b8e520d1ada2b3c82c7f3565a2b0bfe3d"
        },
        "date": 1786679704276,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "92e0d51b7e3d8f1d785a8675210b027eea8b9278",
          "message": "by-ref out-parameters are non-null after the call",
          "timestamp": "2026-08-14T05:57:58+02:00",
          "tree_id": "251c6d3b90473c3c2493010d3e2f260aa896d549",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/92e0d51b7e3d8f1d785a8675210b027eea8b9278"
        },
        "date": 1786680868093,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c0eea957e05442138c2703bda7dc17af0ed412ba",
          "message": "Subtract what a never branch of a conditional return rules out\n\n`throw_unless()`, `throw_if()`, `abort_unless()` and the rest declare\ntheir effect in the return type rather than with an assertion tag:\n\n    @return ($condition is false ? never : ($condition is non-empty-mixed ? TValue : never))\n\nA `never` branch is one the call cannot return through, so an argument\nvalue that selects it cannot have got past the call.  The argument was\nleft untouched, which made every guard written this way invisible: the\nvalue it had just proved present was still reported as possibly null on\nthe next line.\n\nA statement-level call whose callee has a conditional return type now\nsplits each argument into the alternatives it can hold at runtime\n(`?Foo` into `Foo` and `null`, `bool` into `true` and `false`), walks the\nconditional spine with each of them bound to the parameter, and drops the\nones that land on `never`.  The argument is keyed the way any other\nnarrowing subject is, so a property path or an array element narrows too.\n\nThe member-level condition test is separate from the shared conditional\nevaluator on purpose: that one answers `is false` only for an argument\nalready narrowed to a boolean, because a plain `bool` really may be\neither.  Here the alternatives have already been split apart, so a\nmember that cannot be a bool at all settles the condition, which is what\nsends a `null` argument down the else branch instead of leaving it\nundecided.  `non-empty-mixed` is read as \"truthy\", the condition this\nfamily is written against.",
          "timestamp": "2026-08-14T07:24:28+02:00",
          "tree_id": "66b867e7dd3b4383679e991cacc6ad91711ad24d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c0eea957e05442138c2703bda7dc17af0ed412ba"
        },
        "date": 1786715020793,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1ac8bf7b83a32ec52efca3fed9e7a959a8545050",
          "message": "Fix a couple of issues",
          "timestamp": "2026-08-14T15:36:48+02:00",
          "tree_id": "91caf4805bb5a3d94f622a97846f238b27d2d380",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1ac8bf7b83a32ec52efca3fed9e7a959a8545050"
        },
        "date": 1786715905227,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c5531466982684cdffc6be230173468297d45cff",
          "message": "A phar whose stub ends without a line break is read rather than skipped",
          "timestamp": "2026-08-14T15:59:09+02:00",
          "tree_id": "b396cce928eeba5bf008dddb3ac5af0d6297df78",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c5531466982684cdffc6be230173468297d45cff"
        },
        "date": 1786716979007,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a07fb5824d7ca648123fa15a9dcad5ab770b3ade",
          "message": "Fold a doubly negated guard before reading it",
          "timestamp": "2026-08-14T16:57:35+02:00",
          "tree_id": "595fd453512070e218b9500d188d88f9615ccc51",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a07fb5824d7ca648123fa15a9dcad5ab770b3ade"
        },
        "date": 1786720452747,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fab739b2448877e3e62642856255c08cd406e9ef",
          "message": "Fix a couple of issues with constants",
          "timestamp": "2026-08-14T16:59:16+02:00",
          "tree_id": "06e8f9f63f86bce66ec966c65bb2334662780581",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fab739b2448877e3e62642856255c08cd406e9ef"
        },
        "date": 1786720545851,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d8b2de5d926fde09c0a258adf612c1064eec2b82",
          "message": "A builder method chained on an Eloquent relation stays on the relation",
          "timestamp": "2026-08-14T18:24:56+02:00",
          "tree_id": "f900f65a9e7886269b4f9172efd51595b2fea733",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d8b2de5d926fde09c0a258adf612c1064eec2b82"
        },
        "date": 1786725689374,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e0a5b297c596f532fcfbeb0583ee0730b06bf21c",
          "message": "`preg_match()` fills `$matches` with the keys the pattern actually has",
          "timestamp": "2026-08-14T18:31:50+02:00",
          "tree_id": "f184b9da808f2a1eebb11d3e27e988f3f5b7eadc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e0a5b297c596f532fcfbeb0583ee0730b06bf21c"
        },
        "date": 1786726137542,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e4446d1da1839f3cb8a8d2a52e97f147cf03c4a9",
          "message": "[Fix][Laravel] Model Factories make/create return generic Model (#356)",
          "timestamp": "2026-08-14T19:02:20+02:00",
          "tree_id": "a1f489584f9c8e9868e66b1deb0ae4c41755bf15",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e4446d1da1839f3cb8a8d2a52e97f147cf03c4a9"
        },
        "date": 1786727904260,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "819ebfa735bd9e44898990b801b77b1b66b7ef8e",
          "message": "Unquoted array keys in string interpolation are no longer resolved as\nclass names",
          "timestamp": "2026-08-14T19:31:53+02:00",
          "tree_id": "fd5f9f6a90b3e8b128a4520c806d042ed8d8eabe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/819ebfa735bd9e44898990b801b77b1b66b7ef8e"
        },
        "date": 1786729730794,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b8b51f72c5f5444d093c80e6577a6ffc2cb90ebd",
          "message": "Fix issues with preg_match",
          "timestamp": "2026-08-14T19:39:49+02:00",
          "tree_id": "f75f243f894e830e41c5b37c85dae7ea2c459b28",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b8b51f72c5f5444d093c80e6577a6ffc2cb90ebd"
        },
        "date": 1786730225515,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5d251c04b198cb64a8bdb08a40453bbb8d7c4540",
          "message": "A `\"\\x8b\"` escape no longer takes the server down with it",
          "timestamp": "2026-08-14T20:05:17+02:00",
          "tree_id": "b8f410dfd1bc3ac733e1e36952e02464ae4870be",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5d251c04b198cb64a8bdb08a40453bbb8d7c4540"
        },
        "date": 1786731664894,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 85.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "889519c0f165463792fb4e2db784d6dfd5bb3547",
          "message": "Fix array shape issues",
          "timestamp": "2026-08-14T20:35:27+02:00",
          "tree_id": "53123c4724cd48ce8422a779508155573ea746f9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/889519c0f165463792fb4e2db784d6dfd5bb3547"
        },
        "date": 1786733623663,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5d003617f6b43c897e0d9a20efcc21357bfd387f",
          "message": "`@see` with free-text prose no longer produces bogus diagnostics",
          "timestamp": "2026-08-14T21:14:37+02:00",
          "tree_id": "3fb581891c9984fdc0ae03135bca9419fd19507e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5d003617f6b43c897e0d9a20efcc21357bfd387f"
        },
        "date": 1786735832030,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "08a43d85f51d236d8aae5ab31d693e9406c96563",
          "message": "An element write refines what the array already holds",
          "timestamp": "2026-08-14T21:21:13+02:00",
          "tree_id": "8f8c18cf14f4fe1969be5918f0dab533f87f9ab0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/08a43d85f51d236d8aae5ab31d693e9406c96563"
        },
        "date": 1786736331646,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "76cf8639ad9758a229e162634f784a759d793703",
          "message": "`array_filter` keeps what its callback proves about the keys",
          "timestamp": "2026-08-14T21:58:45+02:00",
          "tree_id": "4dc474d160ccc9c4194489a3f7a50a6dc02062c6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/76cf8639ad9758a229e162634f784a759d793703"
        },
        "date": 1786738434036,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8494d14443865205891d6ed2f98645fd0c20c2df",
          "message": "Fix a couple of type comparison issues",
          "timestamp": "2026-08-14T22:24:57+02:00",
          "tree_id": "7c1a67f0bd236303c073024caaf5c1e63c733e99",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8494d14443865205891d6ed2f98645fd0c20c2df"
        },
        "date": 1786740078083,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "69157811dbc585e8695547f0ece5855391c30f2d",
          "message": "Fix five narrowing issues",
          "timestamp": "2026-08-14T23:06:07+02:00",
          "tree_id": "3d5da9967aeb6811eae6afc64d5ef1e338de8e43",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/69157811dbc585e8695547f0ece5855391c30f2d"
        },
        "date": 1786742587666,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c511ef614b5d9c9b403fb89b0942efdcb9321c43",
          "message": "A namespaced constant is found however it is written",
          "timestamp": "2026-08-14T23:14:55+02:00",
          "tree_id": "fea92f06060040c5077467ed69c9a17956eccad3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c511ef614b5d9c9b403fb89b0942efdcb9321c43"
        },
        "date": 1786743561937,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "53eef924d5db9a2ad6512d759c303dd3b328f1cb",
          "message": "A file with box-drawing characters keeps its diagnostics",
          "timestamp": "2026-08-14T23:34:29+02:00",
          "tree_id": "5ba7b2e4902a44f65f4fefaf2c1bd2631d02c533",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/53eef924d5db9a2ad6512d759c303dd3b328f1cb"
        },
        "date": 1786744287637,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c3da9e9083bf43b51d3b6b6c82fd159f1b056f8d",
          "message": "A second `namespace` block in a file is checked like the first one",
          "timestamp": "2026-08-14T23:54:32+02:00",
          "tree_id": "69c5340ade4eb288c8f0595d769b88f8ad77e095",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c3da9e9083bf43b51d3b6b6c82fd159f1b056f8d"
        },
        "date": 1786745494905,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d60f7eda44d03c76c88cb0043c791b7758099b2a",
          "message": "Fix a couple of bugs and update the roadmap",
          "timestamp": "2026-08-15T00:54:25+02:00",
          "tree_id": "e7a14a9a90b2df29362577c7945f18e609968302",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d60f7eda44d03c76c88cb0043c791b7758099b2a"
        },
        "date": 1786749084448,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 84,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ff5946866c75ff30e01b0e6c6d196b550b66c8b4",
          "message": "Update embedded phpstorm-stubs to latest master\n\nPin moves from f6dd2dd (2026-04-29) to 5f68c1021b (2026-08-14),\npicking up PHP 8.6 stub coverage and various signature/return-type\nfixes (Redis, FFI, enchant, xmlreader, openssl_x509_parse,\nhtmlspecialchars default flags). Full test suite passes unchanged;\nnone of the upstream fixes overlap with our stub_patches.rs\nworkarounds or the open bugs in docs/todo/bugs.md.",
          "timestamp": "2026-08-15T01:21:17+02:00",
          "tree_id": "45b7227d5a973e53441b7986a4b041f3f799d50c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ff5946866c75ff30e01b0e6c6d196b550b66c8b4"
        },
        "date": 1786751758170,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "60f60132422f6fe63030e760de005d225ed6c4fa",
          "message": "Fix a couple of bugs and update the roadmap",
          "timestamp": "2026-08-15T01:41:06+02:00",
          "tree_id": "a52d62bd141d50322406105a123243ad369393a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/60f60132422f6fe63030e760de005d225ed6c4fa"
        },
        "date": 1786751977429,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fc5c6fa5528c232d6faa4342102ca9342da57592",
          "message": "Improve ternary narrowing",
          "timestamp": "2026-08-15T02:04:50+02:00",
          "tree_id": "3023e14d6c77c37ae0756eca24b07e83b0b2660c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fc5c6fa5528c232d6faa4342102ca9342da57592"
        },
        "date": 1786753254536,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bad25d1fbcd1bc8d42cad1935f9da54a9e62664b",
          "message": "Improve symbol resolution",
          "timestamp": "2026-08-15T02:06:10+02:00",
          "tree_id": "ffefcca064657f121888aa52ee406f92a48dc733",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bad25d1fbcd1bc8d42cad1935f9da54a9e62664b"
        },
        "date": 1786753398793,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9307706ed5febcd88f77a067f13b0f56e3c74c8a",
          "message": "A typed class constant keeps the value it was given",
          "timestamp": "2026-08-15T02:52:16+02:00",
          "tree_id": "641635d44abf5670a4b70e69d0a80355e218cf2b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9307706ed5febcd88f77a067f13b0f56e3c74c8a"
        },
        "date": 1786756128711,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 33.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6551e6c613bdca82249067f606c42a274eaff566",
          "message": "Fix more array shape logic array shape",
          "timestamp": "2026-08-15T03:07:52+02:00",
          "tree_id": "4e501e45a079eb6191f3268bda5d82f9907fc202",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6551e6c613bdca82249067f606c42a274eaff566"
        },
        "date": 1786757017161,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "44b4ea305b206319ff0bdc792af4dcfd1243001d",
          "message": "Improve Standard-library return types",
          "timestamp": "2026-08-15T03:59:05+02:00",
          "tree_id": "aaf4c726647dbfcb38beb81912e583fcc80cb1c9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/44b4ea305b206319ff0bdc792af4dcfd1243001d"
        },
        "date": 1786760173558,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6f54a0531d1c8083bdee486ddd73da77e5181c3f",
          "message": "Improve Standard-library return types",
          "timestamp": "2026-08-15T04:22:36+02:00",
          "tree_id": "dc31418be407e2988638740b899a32160f5db06a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6f54a0531d1c8083bdee486ddd73da77e5181c3f"
        },
        "date": 1786761583377,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d35c673f5717805a2b3a68405a4baa9ebfe893c3",
          "message": "A member read off a class constant resolves against the value the\nconstant holds, not the class that declares it",
          "timestamp": "2026-08-15T04:43:01+02:00",
          "tree_id": "11a0a0c8607a1c33c4d01f0365bdc903ac82731f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d35c673f5717805a2b3a68405a4baa9ebfe893c3"
        },
        "date": 1786762754957,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "eff4b0777e7b358cc4babcc8e52dd0cf44920329",
          "message": "Improve narrowing",
          "timestamp": "2026-08-15T04:50:31+02:00",
          "tree_id": "59c795701ba48181b0de2014fb2c055f8dd1ed73",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/eff4b0777e7b358cc4babcc8e52dd0cf44920329"
        },
        "date": 1786763257483,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ad4ea8be00fa16fbed20c89b052f7d01aa7b33a1",
          "message": "A one-line function no longer inherits the previous function's `@param`",
          "timestamp": "2026-08-15T05:09:52+02:00",
          "tree_id": "1e5ae06b38fed2daa86370a452b762f842a11f74",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ad4ea8be00fa16fbed20c89b052f7d01aa7b33a1"
        },
        "date": 1786764425029,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bfe0ce3b192550b46cc345b24a0c5b5cd969fe7d",
          "message": "Improve type narrowing",
          "timestamp": "2026-08-15T05:59:43+02:00",
          "tree_id": "5b816ff9791624153737a3d2ec5396d763b98ada",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bfe0ce3b192550b46cc345b24a0c5b5cd969fe7d"
        },
        "date": 1786767386687,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c618c8aad6f59020b83c93275b98723f648f8d55",
          "message": "Fix generic call return type, and update roadmap",
          "timestamp": "2026-08-15T07:06:13+02:00",
          "tree_id": "51b3aea7d4e0121e1e66fc01a3491b0cb6c3fd36",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c618c8aad6f59020b83c93275b98723f648f8d55"
        },
        "date": 1786771390758,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d2ff21a3621fb7586396e7468f5cc380d9581845",
          "message": "Stricter diagnostics",
          "timestamp": "2026-08-15T07:26:29+02:00",
          "tree_id": "3f72ec1d3dbf65cb4521af41d623524556666b17",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d2ff21a3621fb7586396e7468f5cc380d9581845"
        },
        "date": 1786772523426,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9bc886c33ca35f9be28a96178664642e4855b1bc",
          "message": "Update tasks",
          "timestamp": "2026-08-15T07:34:07+02:00",
          "tree_id": "23b45583c298a80c400f69b72cf22d79dc832556",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9bc886c33ca35f9be28a96178664642e4855b1bc"
        },
        "date": 1786773080512,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "758e90f2031afce8425c5f989dfa3653fcd6d09f",
          "message": "The type engine itself now knows that a class is an `object` and that a\n`Traversable` is `iterable`",
          "timestamp": "2026-08-15T07:55:26+02:00",
          "tree_id": "46bf744fa147a3857c52784a06b0cf7310c75bb8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/758e90f2031afce8425c5f989dfa3653fcd6d09f"
        },
        "date": 1786774285891,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c919f8d0d8dfa1bc6d02dcef2a66aa0829fccf25",
          "message": "A plain function's `@return` docblock is checked against its body",
          "timestamp": "2026-08-15T07:58:43+02:00",
          "tree_id": "e3eeff6af8e786e271bb2fc101230660f3d5021f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c919f8d0d8dfa1bc6d02dcef2a66aa0829fccf25"
        },
        "date": 1786774563445,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bf399a89d4a443535bd977a6756115a604be6a53",
          "message": "A `Stringable` object passed to a `string` parameter is checked against\nthe file's `strict_types` setting",
          "timestamp": "2026-08-15T08:06:08+02:00",
          "tree_id": "ac5ea05d29dce3fe3b8e89d468547bd9d9ff77f0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bf399a89d4a443535bd977a6756115a604be6a53"
        },
        "date": 1786774980247,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "97a422af93ea2e41bfd55a89210f10487640227d",
          "message": "A standalone `@var` cast above `return` is honoured",
          "timestamp": "2026-08-15T08:28:00+02:00",
          "tree_id": "621aa5b24f89d452d1aeafe65b76cbb2380c3496",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/97a422af93ea2e41bfd55a89210f10487640227d"
        },
        "date": 1786776269389,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a4340e6befc592800ab9f3ea2f3ff4c92a374568",
          "message": "An array built up with `[]` keeps the `false` half of what it stores",
          "timestamp": "2026-08-15T17:12:58+02:00",
          "tree_id": "973b7de8632e267397616b60e442465986cd20e4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a4340e6befc592800ab9f3ea2f3ff4c92a374568"
        },
        "date": 1786807788121,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e5ce33832e1521fc2a5c062f6bbdf214986faeb1",
          "message": "Improve type handeling of standard lib",
          "timestamp": "2026-08-15T17:14:04+02:00",
          "tree_id": "ec4d1ce5548ffb4ed206965abfd65eb8e63f88cc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e5ce33832e1521fc2a5c062f6bbdf214986faeb1"
        },
        "date": 1786807871254,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5fe9334bc42ddb6dd2bef00b680ddaa95e947722",
          "message": "Improve type narrowing",
          "timestamp": "2026-08-15T17:46:53+02:00",
          "tree_id": "22d1fbc4066cfd0ec1f4a4869e4eb7c8c9369952",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5fe9334bc42ddb6dd2bef00b680ddaa95e947722"
        },
        "date": 1786809739130,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "61fd16a15cf23e493adf54028934d335c8584495",
          "message": "Improve type narrowing",
          "timestamp": "2026-08-15T17:47:32+02:00",
          "tree_id": "81a9502c786a43c1764b50987bd84923c002c69f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/61fd16a15cf23e493adf54028934d335c8584495"
        },
        "date": 1786809849962,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1f79db3b90108e356edb02cca7de6410a985deb2",
          "message": "A helper that bails out on its condition narrows the code after it",
          "timestamp": "2026-08-15T18:00:38+02:00",
          "tree_id": "d225acdd1ca9b4bb9cb4ccc285f89fff28601695",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1f79db3b90108e356edb02cca7de6410a985deb2"
        },
        "date": 1786810606196,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a58de827769ca0a4e8469687cc7d244ec3ff2e75",
          "message": "A loop over an array that cannot be empty leaves the value it built\nbehind",
          "timestamp": "2026-08-15T18:21:53+02:00",
          "tree_id": "f395c184ca406c2d049519790410086393fc4d59",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a58de827769ca0a4e8469687cc7d244ec3ff2e75"
        },
        "date": 1786811891636,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "56b596ab0dabc8456155b3da1837a9163ba1a1e3",
          "message": "A magic constant carries its own type",
          "timestamp": "2026-08-15T18:43:50+02:00",
          "tree_id": "7d40b32e5deeec3430597a3a9c3f6397e243d5fa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/56b596ab0dabc8456155b3da1837a9163ba1a1e3"
        },
        "date": 1786813212627,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "dda69f209f7f0344aa7cbe58f89b296b618838a0",
          "message": "Faster diagnostics on long method chains",
          "timestamp": "2026-08-15T18:52:58+02:00",
          "tree_id": "4bddc4e4c266ff7ad19e993ba4e094ade4a5595d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dda69f209f7f0344aa7cbe58f89b296b618838a0"
        },
        "date": 1786813785924,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "97e4eae9cf9c20c31ad5fea06e686ae7bcd85eeb",
          "message": "A `float` reaching an `int` position outside `declare(strict_types=1)`\nis no longer reported",
          "timestamp": "2026-08-15T19:00:31+02:00",
          "tree_id": "f6d2875fc3cb27284ed739876778a8789dea771c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/97e4eae9cf9c20c31ad5fea06e686ae7bcd85eeb"
        },
        "date": 1786814177044,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1f506de48b3ae805fc60f53c4ac0c0d892a4aeb1",
          "message": "Fix integer math",
          "timestamp": "2026-08-15T19:35:33+02:00",
          "tree_id": "043a52975f8599590d90172b9d976ee7a7f99f55",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1f506de48b3ae805fc60f53c4ac0c0d892a4aeb1"
        },
        "date": 1786816304234,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "1c31857576c7229dd3ba8d4f0df3d4d3bd678628",
          "message": "A factory's `$model` property is ignored",
          "timestamp": "2026-08-15T20:43:41+02:00",
          "tree_id": "cb30c64e7c176c7d887840b336cc35c5bce33abb",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1c31857576c7229dd3ba8d4f0df3d4d3bd678628"
        },
        "date": 1786820404091,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "85673933+petrovo-as@users.noreply.github.com",
            "name": "Petr Aš",
            "username": "petrovo-as"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "5a2ad796689884d22c589f4119eee064410fcb79",
          "message": "Resolve, index, and rename functions and constants declared outside a class (#357)\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-08-15T22:27:00+02:00",
          "tree_id": "28148d11baeee22e69cdc894918867ae7ba058ea",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5a2ad796689884d22c589f4119eee064410fcb79"
        },
        "date": 1786826584432,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "russiameb@gmail.com",
            "name": "real420og",
            "username": "real420og"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f2d61ba6a51eeb7bd68be63196bc1988d0b0208d",
          "message": "Find implementations declared in Composer packages\n\nGo-to-implementation on a method declared by a vendor interface, such as\nSymfony's HttpKernelInterface::handle(), returned nothing. Three filters\neach removed a valid answer:\n\n- once the workspace index was ready, results were restricted to project\n  classes and the scans that could reach vendor code were skipped, so a\n  vendor-owned target lost the implementations shipping beside it;\n- implementors were collected with abstract classes excluded, dropping an\n  abstract base whose method has a body;\n- an implementor had to declare the member itself, so a concrete subclass\n  that inherits the method unchanged was skipped.\n\nA target that itself lives under /vendor/ now drops the project-only\nrestriction and keeps the class-index scans, which are the only way to\nreach classes the workspace index never parses. Embedded stubs stay on\nthe fast path, since PHP's own interfaces are implemented across the\nwhole dependency tree.\n\nBoth member-level paths share locate_member_implementation, which returns\nthe declaration that supplies the body — the implementor's own or the\nnearest ancestor's — and skips a method only ever re-declared abstract.",
          "timestamp": "2026-08-15T22:43:24+02:00",
          "tree_id": "5f1a462a4a21ff2173b8d025aa511b99824c3939",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f2d61ba6a51eeb7bd68be63196bc1988d0b0208d"
        },
        "date": 1786827633252,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ab7b56aa6c1f19664073321481a4022145f06a6b",
          "message": "Improve workspace diagnostics behaviour",
          "timestamp": "2026-08-16T00:26:27+02:00",
          "tree_id": "3dc82b9baad0bbb0d8c774a580d6b144a30310f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ab7b56aa6c1f19664073321481a4022145f06a6b"
        },
        "date": 1786833802310,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 80,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9dcaa706e4b04a92c5702abfa2f1e04a4314bfc1",
          "message": "A reopened file no longer shows diagnostics from before it was closed",
          "timestamp": "2026-08-16T00:32:14+02:00",
          "tree_id": "3c2af3aac82abb5aa321f5ea0fa6356c9e081c9c",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9dcaa706e4b04a92c5702abfa2f1e04a4314bfc1"
        },
        "date": 1786834146407,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2f018fa15a63c0871c8e637f3cdf40439f0149bc",
          "message": "Renaming a global constant no longer renames a class constant of the\nsame name",
          "timestamp": "2026-08-16T00:34:38+02:00",
          "tree_id": "7b73b221908a3720a78e9290dbe1cda7c632e094",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2f018fa15a63c0871c8e637f3cdf40439f0149bc"
        },
        "date": 1786834363666,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b0bde13b8756757c92e90c88f19009bcf1a51a2f",
          "message": "Reference counts above a declaration update once the initial index\nfinishes",
          "timestamp": "2026-08-16T00:41:20+02:00",
          "tree_id": "79fb457b15b3c404216b9ca487c01a5d2dea094f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b0bde13b8756757c92e90c88f19009bcf1a51a2f"
        },
        "date": 1786834643905,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "92b4cad1317649e0c7912a1cf64ff7888dbf3448",
          "message": "Find References on a global constant can exclude its declaration",
          "timestamp": "2026-08-16T01:21:52+02:00",
          "tree_id": "a9c2f2195a0a778b07d5d78350e2c8191fd3f823",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/92b4cad1317649e0c7912a1cf64ff7888dbf3448"
        },
        "date": 1786837128130,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4e813513eb3667ec02c91bf7a50b4e1b7fadd729",
          "message": "A workspace diagnostics pull no longer re-sends files the editor already\nhas",
          "timestamp": "2026-08-16T01:32:25+02:00",
          "tree_id": "e048741b0c4212b0b1e3da9db704ac685017db79",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4e813513eb3667ec02c91bf7a50b4e1b7fadd729"
        },
        "date": 1786837670767,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5cc652f7f0dcebc510320cf41a5d91ff4c6e7888",
          "message": "Update roadmap",
          "timestamp": "2026-08-16T01:43:19+02:00",
          "tree_id": "5a26240203df46662b1096bfd22efcf49bb7eaa1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5cc652f7f0dcebc510320cf41a5d91ff4c6e7888"
        },
        "date": 1786838336749,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "17831c987a3b8e936cb3ac1d61da549353449386",
          "message": "A project-wide external tool re-run no longer invalidates every file it\nreported",
          "timestamp": "2026-08-16T01:42:31+02:00",
          "tree_id": "4ea00b24855dff795133724820f1658e6fca9e60",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/17831c987a3b8e936cb3ac1d61da549353449386"
        },
        "date": 1786838372912,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "DarkGhostHunter@Gmail.com",
            "name": "Italo",
            "username": "DarkGhostHunter"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e6ab2e3c6080550e34eafba08c7f197cc910d5c9",
          "message": "Switch to thinLTO optimization (2% faster execution)",
          "timestamp": "2026-08-16T02:39:52+02:00",
          "tree_id": "080e544e9f128389b68de76a189a6ad716aa00af",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6ab2e3c6080550e34eafba08c7f197cc910d5c9"
        },
        "date": 1786841793334,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "d63bb5b2523fd981ccd83ec28a385e6287ba7bb8",
          "message": "Path helper links and completion",
          "timestamp": "2026-08-16T03:45:28+02:00",
          "tree_id": "99003784b424581386cb5111898fe445b52120e4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d63bb5b2523fd981ccd83ec28a385e6287ba7bb8"
        },
        "date": 1786845676518,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "0e06c05955aa5883994d1aa6da6ff174db60986b",
          "message": "A container key leads to the provider that binds it",
          "timestamp": "2026-08-16T05:48:35+02:00",
          "tree_id": "bee27c02fbd92fc8be74ae443b26703254c1251b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0e06c05955aa5883994d1aa6da6ff174db60986b"
        },
        "date": 1786853148889,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e6a15a720d0d2525e652760efc79a7387a8e5756",
          "message": "Conditional return types on `url()` and its sibling helpers (#337)",
          "timestamp": "2026-08-16T06:36:03+02:00",
          "tree_id": "912ec24f66c346caf46653505549a5e9c6f390a9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e6a15a720d0d2525e652760efc79a7387a8e5756"
        },
        "date": 1786855958602,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1d056bc51ade82b5fba52d7e7c5e75a63c600be7",
          "message": "Editing a service provider takes effect immediately",
          "timestamp": "2026-08-16T07:18:42+02:00",
          "tree_id": "23e6190922111ffc3f1540826149289bc73cb6ce",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1d056bc51ade82b5fba52d7e7c5e75a63c600be7"
        },
        "date": 1786858505415,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1b2705869265c17c098eeb0ec73d9ed4c7ef1146",
          "message": "A non-Laravel project's own `config()`, `route()`, `view()`, `__()`, or\n`trans()` function no longer hovers, navigates, or renames as a Laravel\nstring key",
          "timestamp": "2026-08-16T19:21:34+02:00",
          "tree_id": "b159658533d1fc800f451a78ef98e643a4652c9b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1b2705869265c17c098eeb0ec73d9ed4c7ef1146"
        },
        "date": 1786901818005,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6803a9cb3e83919ddfdcad9e2cfca8d0cc9bb7fe",
          "message": "Hover no longer depends on indexing timing",
          "timestamp": "2026-08-17T00:50:28+02:00",
          "tree_id": "6c4aa61fdc87ad44a5efae2e3c13e28224b1d2d0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6803a9cb3e83919ddfdcad9e2cfca8d0cc9bb7fe"
        },
        "date": 1786921645726,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": false,
          "id": "2d3f049d6c768c7b89293e969c8532cb5c514337",
          "message": "A raw Blade echo is read from its own opening brace",
          "timestamp": "2026-08-17T02:53:00+02:00",
          "tree_id": "0466f4de06764b1ce3216f315cc1c3ff29f7efc3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2d3f049d6c768c7b89293e969c8532cb5c514337"
        },
        "date": 1786929185049,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ec47f8015ad50ecf5b98f8a9e37280a7cbdaa0b6",
          "message": "fix: normalize filesystem aliases during discovery",
          "timestamp": "2026-08-17T03:39:25+02:00",
          "tree_id": "029f12344fb549f9c30073230a038b45a4c41921",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ec47f8015ad50ecf5b98f8a9e37280a7cbdaa0b6"
        },
        "date": 1786931759788,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ea941a807a828dd7a286a7ff1ff641a4bb323ef7",
          "message": "Hover stands down at every declaration site, not just most of them",
          "timestamp": "2026-08-17T03:40:20+02:00",
          "tree_id": "a36558e3e5df5fb30ca42e9cbc1609aae115a691",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ea941a807a828dd7a286a7ff1ff641a4bb323ef7"
        },
        "date": 1786931762103,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 83.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": false,
          "id": "a00e13ab4e1056b8025e24a5d4af3c08fd02d8f1",
          "message": "Go-to-definition works inside property hook bodies",
          "timestamp": "2026-08-17T05:03:31+02:00",
          "tree_id": "c62307ead97892023a3f66573f63081be6c13fee",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a00e13ab4e1056b8025e24a5d4af3c08fd02d8f1"
        },
        "date": 1786936947151,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7731c8acf32d2a549e9c373f4698f06d58ec3ee6",
          "message": "Clean up changelog",
          "timestamp": "2026-08-17T20:06:23+02:00",
          "tree_id": "9c74488e7d2240dd85163ad4dbf1cd4f4992effe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7731c8acf32d2a549e9c373f4698f06d58ec3ee6"
        },
        "date": 1786991034737,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "77535991ba4b725619e26843ecf183745bccce0f",
          "message": "A route under a dynamic group prefix is no longer flagged as unknown",
          "timestamp": "2026-08-17T21:20:28+02:00",
          "tree_id": "842337d650db77b939c675c999eefc36ae55a9e7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77535991ba4b725619e26843ecf183745bccce0f"
        },
        "date": 1786995458548,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7731c8acf32d2a549e9c373f4698f06d58ec3ee6",
          "message": "Clean up changelog",
          "timestamp": "2026-08-17T20:06:23+02:00",
          "tree_id": "9c74488e7d2240dd85163ad4dbf1cd4f4992effe",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7731c8acf32d2a549e9c373f4698f06d58ec3ee6"
        },
        "date": 1786995521597,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 81.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": false,
          "id": "77878bb055c3d298ea7c7c35e35a473c269c9f15",
          "message": "A route under a dynamic group prefix is no longer flagged as unknown",
          "timestamp": "2026-08-17T21:23:49+02:00",
          "tree_id": "fa1a2f982db460331a9ae768059863a7d91332f8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77878bb055c3d298ea7c7c35e35a473c269c9f15"
        },
        "date": 1786995710112,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "118f2c6cc4ddc7a83c979509cfbc56a24af011cb",
          "message": "Only use PHPStan if the project depends on it directly",
          "timestamp": "2026-08-18T22:34:01+02:00",
          "tree_id": "654962653eaf433c775368158b1cab0ca2497c47",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/118f2c6cc4ddc7a83c979509cfbc56a24af011cb"
        },
        "date": 1787086374406,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "775837431dd0c84a65c284c6386ac384dc588980",
          "message": "A generic argument left out takes the default its `@template` declares",
          "timestamp": "2026-08-19T01:31:11+02:00",
          "tree_id": "30c890f9b76a31cf9fb56d8ad670e18c05166fbc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/775837431dd0c84a65c284c6386ac384dc588980"
        },
        "date": 1787096886650,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bb55323252aae20649514d8c916abe5cbb36bedb",
          "message": "A `@phpstan-assert` tag whose asserted type is a union now narrows each\nmember",
          "timestamp": "2026-08-19T03:02:26+02:00",
          "tree_id": "6de22caf4ddf5fea31769ffab197c2b8c65a53d1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bb55323252aae20649514d8c916abe5cbb36bedb"
        },
        "date": 1787102382634,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 83.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0de10f47edc3656a40522ad96ef5da2f893a8d08",
          "message": "A project that uses Mago only for formatting no longer gets Mago's lint\nand analyze reports",
          "timestamp": "2026-08-19T04:08:55+02:00",
          "tree_id": "20676af32f369b555f4f13b5b9a3bed0c1a70b62",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0de10f47edc3656a40522ad96ef5da2f893a8d08"
        },
        "date": 1787106516173,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "36e8f3519ddf5cbc3b7be88db15d97ab219b6989",
          "message": "A Laravel project that requires Larastan gets PHPStan diagnostics too",
          "timestamp": "2026-08-19T04:21:53+02:00",
          "tree_id": "b75e953733571d524c9a062667b30aecc894004b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/36e8f3519ddf5cbc3b7be88db15d97ab219b6989"
        },
        "date": 1787107214538,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "812aa4ffbeda30b152cec3fa3e294cf86579c566",
          "message": "A custom Eloquent builder keeps the model it was built for",
          "timestamp": "2026-08-19T05:27:30+02:00",
          "tree_id": "3599a756b46402b56ce1e047b2c213b0255f91b0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/812aa4ffbeda30b152cec3fa3e294cf86579c566"
        },
        "date": 1787111261553,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "3b87e0484594e6b7b763a90b5a952aeb4579ea3b",
          "message": "A custom Eloquent builder keeps the model it was built for",
          "timestamp": "2026-08-19T05:31:18+02:00",
          "tree_id": "e94447dbf6eaaa02f54879eadad03c0eadac6873",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/3b87e0484594e6b7b763a90b5a952aeb4579ea3b"
        },
        "date": 1787111362091,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 79,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4352d560806ca8dcb8fd277c0456c4d6ef31a0ab",
          "message": "One unanalysable file no longer stops workspace diagnostics",
          "timestamp": "2026-08-19T05:31:44+02:00",
          "tree_id": "587d5ad825779a37df2a93ebbfe5b6a823255b82",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4352d560806ca8dcb8fd277c0456c4d6ef31a0ab"
        },
        "date": 1787111436918,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 82,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bc6c58bccf544e458c11ac7e32a59a9eaaa3e374",
          "message": "PHPStan: Check for /larastan to allow for forks, and also allow if a\nphpstan.neon exists",
          "timestamp": "2026-08-19T05:40:32+02:00",
          "tree_id": "32a9be44f68fb3cfbb2d0c691ff7a1512790c1a9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bc6c58bccf544e458c11ac7e32a59a9eaaa3e374"
        },
        "date": 1787112105858,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "424edc09bc81adca0b668e0d39e9b5e405b520e6",
          "message": "A project with its own `phpcs.xml` runs phpcs, even when\n`squizlabs/php_codesniffer` is only a transitive dependency",
          "timestamp": "2026-08-19T05:57:20+02:00",
          "tree_id": "a6a1c1c0c85563115841744dd227962c0eb2d965",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/424edc09bc81adca0b668e0d39e9b5e405b520e6"
        },
        "date": 1787112817915,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "832ac2b0f5475500faaea18f092a52952f7efa3b",
          "message": "An `instanceof` check on a value declared `object|string` narrows it to\nthe class",
          "timestamp": "2026-08-19T06:46:32+02:00",
          "tree_id": "7dd7061d80d8b93dc3e3421dfd245a675445bcb4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/832ac2b0f5475500faaea18f092a52952f7efa3b"
        },
        "date": 1787116013853,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "710232b52c176137cce9618b5acf6145ef9dd56a",
          "message": "A filtered `list` is no longer reported as a `list`",
          "timestamp": "2026-08-19T07:24:18+02:00",
          "tree_id": "45c42d9cbb3b43b4d6c13088b0b8387970ac9726",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/710232b52c176137cce9618b5acf6145ef9dd56a"
        },
        "date": 1787118197306,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 66.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7d8648c4099c62fbe8f4bcfc798c7c185fb1a72e",
          "message": "Constructor references include `new self()` and `new static()`",
          "timestamp": "2026-08-19T07:52:52+02:00",
          "tree_id": "bcfbc498ceaf72e82e382746f4eff8e55b32040a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7d8648c4099c62fbe8f4bcfc798c7c185fb1a72e"
        },
        "date": 1787119788011,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "17dd5c0bb2b3f54c8a88909278cbc1f2e304fe2a",
          "message": "`is_a($x, Foo::class, true)` keeps the string half of an `object|string`\nparameter",
          "timestamp": "2026-08-19T17:50:54+02:00",
          "tree_id": "d3544bfab53b26fd0f0d6f333ec4bb7628f9629d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/17dd5c0bb2b3f54c8a88909278cbc1f2e304fe2a"
        },
        "date": 1787155950743,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4c0eebb3265e1c271bd00d3368789e0f8e31b537",
          "message": "Renaming a constant now rewrites `defined()` and `constant()` calls too",
          "timestamp": "2026-08-19T19:10:21+02:00",
          "tree_id": "fc4c8927dc74e7fc34081076fa70f7d5473dbfd4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4c0eebb3265e1c271bd00d3368789e0f8e31b537"
        },
        "date": 1787160492412,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 66.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e12c300555e5e933deeb6998d2aabc9f0c018e3e",
          "message": "A project-wide PHPStan/PHPCS/Mago scan no longer undoes a result it was\novertaken by",
          "timestamp": "2026-08-19T19:28:09+02:00",
          "tree_id": "84ab4319da3d243b2abd539ed9ebc959c8b51249",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e12c300555e5e933deeb6998d2aabc9f0c018e3e"
        },
        "date": 1787161525311,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 65.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d8cbe374cb62288e3ed9db2637b45726380428e4",
          "message": "Fix issues with renaming",
          "timestamp": "2026-08-19T19:45:08+02:00",
          "tree_id": "76967a57467eb5fc0a90087900e26b10c41ce6c0",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d8cbe374cb62288e3ed9db2637b45726380428e4"
        },
        "date": 1787162570022,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f03738a00ea9fb4abfb4d529a0e8f4c69fc92162",
          "message": "Fix narrowing edge cases",
          "timestamp": "2026-08-19T22:23:17+02:00",
          "tree_id": "b654742cf6076d18efad3b3f7ebe3e20b7d67148",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f03738a00ea9fb4abfb4d529a0e8f4c69fc92162"
        },
        "date": 1787172549478,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "dc13ca3e81ad441d4ed54bf2208c9a48e488e0ee",
          "message": "A project that formats with Mago keeps formatting with Mago",
          "timestamp": "2026-08-19T23:23:39+02:00",
          "tree_id": "35ebc84eef87fdcb19cde2ab02976a7b9b1736d4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/dc13ca3e81ad441d4ed54bf2208c9a48e488e0ee"
        },
        "date": 1787176758810,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c9f39021bd3ea76b845ec377452da1fbaf0fff13",
          "message": "Closing a file while its own native diagnostics are still computing no\nlonger resurrects them",
          "timestamp": "2026-08-19T23:49:38+02:00",
          "tree_id": "1b0cfda65dfa2109db44e48b0f66922553c1c6ff",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c9f39021bd3ea76b845ec377452da1fbaf0fff13"
        },
        "date": 1787177361379,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b742f58d8ec07322c7e18a49dfccf36c857355f6",
          "message": "A class's own `offsetGet()` is trusted over `ArrayAccess`'s own docblock",
          "timestamp": "2026-08-20T00:04:35+02:00",
          "tree_id": "75f043bd1ebfeb0a06dfdab6ab00ed3beb7b4a46",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b742f58d8ec07322c7e18a49dfccf36c857355f6"
        },
        "date": 1787178213825,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1057f6dd48e0a4b1013390937b4f7311f8152ade",
          "message": "Unsetting an array element is now understood as narrowing the array, not\njust the variable",
          "timestamp": "2026-08-20T00:23:05+02:00",
          "tree_id": "cafc797605a8807b8dfeeacc12c49a06f6efb86f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1057f6dd48e0a4b1013390937b4f7311f8152ade"
        },
        "date": 1787179214119,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 66.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "af8b2a30c6951c7392fc78b73a3be00f75f6553d",
          "message": "Switching workspace diagnostics off now takes effect immediately too",
          "timestamp": "2026-08-20T00:40:16+02:00",
          "tree_id": "f6516a98c1a71b7ce90df921ad07949a75d27540",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/af8b2a30c6951c7392fc78b73a3be00f75f6553d"
        },
        "date": 1787180138094,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "735e64d1cb7653e52592f74e615c1bef0bd49abc",
          "message": "Improve symbol resolution",
          "timestamp": "2026-08-20T00:46:17+02:00",
          "tree_id": "a58aacb68c2214ceb1b7a7dee15ba3ab5783abd1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/735e64d1cb7653e52592f74e615c1bef0bd49abc"
        },
        "date": 1787180781685,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "77525f55d4d5907b5e699b03a5aecdba0cd6e5f4",
          "message": "A route group whose name is entirely a variable no longer flags its own\nroutes as unknown",
          "timestamp": "2026-08-20T01:48:37+02:00",
          "tree_id": "301fb9b569edf89895385620285a6fdd08dc1e11",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/77525f55d4d5907b5e699b03a5aecdba0cd6e5f4"
        },
        "date": 1787184317017,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "60ae628657c1ede8b89ddfbf82e5054247e186cb",
          "message": "A Laravel Folio page's route name is recognised",
          "timestamp": "2026-08-20T01:57:29+02:00",
          "tree_id": "3b84fb1789fd56778d94428ef52a34139125a185",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/60ae628657c1ede8b89ddfbf82e5054247e186cb"
        },
        "date": 1787184877261,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 66.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "af491bb428e3211a018adb25b8335a0372cfd64f",
          "message": "Update bug list",
          "timestamp": "2026-08-20T02:02:35+02:00",
          "tree_id": "4b5da14064d6ccfd2b611c23b6f2a763af2c2753",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/af491bb428e3211a018adb25b8335a0372cfd64f"
        },
        "date": 1787185171690,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f8c550f2726ceba7ba514c5a88209dd9ba133189",
          "message": "A method chain no longer resolves against another file's `use` import",
          "timestamp": "2026-08-20T02:15:12+02:00",
          "tree_id": "5a8c5262d8083d146009cd7eb6975bea2ce60d5d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f8c550f2726ceba7ba514c5a88209dd9ba133189"
        },
        "date": 1787185870415,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1b528a6d8cc2f3343442fed24e98b2fd1b336963",
          "message": "Go-to-definition on a Blade echo delimiter agrees with its hover",
          "timestamp": "2026-08-20T02:56:09+02:00",
          "tree_id": "2164ead5e1cbc5f2e767ae639ecc462b4ff02610",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1b528a6d8cc2f3343442fed24e98b2fd1b336963"
        },
        "date": 1787188401459,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1b528a6d8cc2f3343442fed24e98b2fd1b336963",
          "message": "Go-to-definition on a Blade echo delimiter agrees with its hover",
          "timestamp": "2026-08-20T02:56:09+02:00",
          "tree_id": "2164ead5e1cbc5f2e767ae639ecc462b4ff02610",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1b528a6d8cc2f3343442fed24e98b2fd1b336963"
        },
        "date": 1787188411185,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "533ef884630924b774a2d74ac3d6050f7b3fbce0",
          "message": "Bump version to 0.10.0",
          "timestamp": "2026-08-20T02:59:57+02:00",
          "tree_id": "b90ca259a1428baf1cf2ed8b0075c2568b0c65f4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/533ef884630924b774a2d74ac3d6050f7b3fbce0"
        },
        "date": 1787188619743,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 34.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4543e5f4825356d0b31a172a3a9da8490fad6cb6",
          "message": "The editor stays responsive while a request is being answered",
          "timestamp": "2026-08-22T16:44:14+02:00",
          "tree_id": "a29730f7ec76256994b953724967ae933ed458b9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4543e5f4825356d0b31a172a3a9da8490fad6cb6"
        },
        "date": 1787410773208,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7d7bbf7da174fa4c793142494cd36eeab3bef74c",
          "message": "Fix benchmarks",
          "timestamp": "2026-08-24T19:32:55+02:00",
          "tree_id": "f30c9254a30580eee26ce7569856e46772397235",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7d7bbf7da174fa4c793142494cd36eeab3bef74c"
        },
        "date": 1787593539626,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "578fb46f7a60044c1b2a8367d5c7425c885eb9cb",
          "message": "Fix closure issues",
          "timestamp": "2026-08-24T20:46:21+02:00",
          "tree_id": "407ed797cda338cf94c149205985cc46f8c96486",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/578fb46f7a60044c1b2a8367d5c7425c885eb9cb"
        },
        "date": 1787597935646,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7fe5801a65ae36c46f758bd584668391f66e4354",
          "message": "A conditional return type's winning intersection arm no longer comes\nback as a union",
          "timestamp": "2026-08-24T22:04:42+02:00",
          "tree_id": "433270d92eb121892f862aa1e0c08cdd5cbc9383",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7fe5801a65ae36c46f758bd584668391f66e4354"
        },
        "date": 1787602668367,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "942e153df2b2fbbf3c0a9748a9e30dbe0a855ef9",
          "message": "`new ReflectionProperty(Foo::class, 'bar')` remembers what it reflects",
          "timestamp": "2026-08-25T01:52:33+02:00",
          "tree_id": "0e5c92287be4610c527bbd27a438039ae25a1f87",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/942e153df2b2fbbf3c0a9748a9e30dbe0a855ef9"
        },
        "date": 1787616342706,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1e8eaab3029241d594401bf6e96c0cde8bd07f62",
          "message": "Fix a couple of narrowing bugs",
          "timestamp": "2026-08-25T01:56:33+02:00",
          "tree_id": "0e872af4d5604994ae8814c3f1d431da7bd282d3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1e8eaab3029241d594401bf6e96c0cde8bd07f62"
        },
        "date": 1787616905504,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f9a7c4b6288078b39fc3f797f6047d74495d4a28",
          "message": "Two variables filled in the same branch keep their correlated\nnullability",
          "timestamp": "2026-08-25T10:06:27+02:00",
          "tree_id": "d309b565efb6bcc832c7cf1afae272c993787504",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f9a7c4b6288078b39fc3f797f6047d74495d4a28"
        },
        "date": 1787645978048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "507a7f603bb1f1c8a5a237515ebc6b532f3b5a85",
          "message": "`analyze` reports diagnostics on the same line in a stable order",
          "timestamp": "2026-08-25T10:38:45+02:00",
          "tree_id": "a1c9d699198b8d7d1f5a79dae1b9fdad423b15ee",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/507a7f603bb1f1c8a5a237515ebc6b532f3b5a85"
        },
        "date": 1787647899895,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "07471582ad39cb61498abfd0fe17cc37646fa240",
          "message": "A property keeps its narrowing across a write the resolver cannot type",
          "timestamp": "2026-08-25T10:48:29+02:00",
          "tree_id": "8073c0c02733f8b4a57044f92b4231d453086a98",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/07471582ad39cb61498abfd0fe17cc37646fa240"
        },
        "date": 1787648491602,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "52a010ad8c695f11ae59aba3c357a9874615208e",
          "message": "Fix narrowing issues",
          "timestamp": "2026-08-25T18:22:01+02:00",
          "tree_id": "826c5c134742d182773df92e633c71b4eeadb458",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/52a010ad8c695f11ae59aba3c357a9874615208e"
        },
        "date": 1787675705705,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "4173661aceb558ce993586c10e4d0036406230f1",
          "message": "fix: PHPStan auto-detection recognises any Laravel-aware extension\n\nPHPantom will not auto-run plain PHPStan at a Laravel application,\nsince it misreads Eloquent, the facades, and the container and reports\ncorrect code as broken. Installing a PHPStan extension that teaches it\nthe framework lifts that refusal, and also certifies vendor/bin/phpstan\non a project that pulls phpstan/phpstan in transitively.\n\nThat check matched only package names ending in /larastan, so a project\nanalysed by calebdw/phpstan-laravel was refused PHPStan entirely unless\nit had also hand-authored a phpstan.neon. What the gate is really\nasking is whether anything is installed that can explain the framework\nto PHPStan, not which package does the explaining, so it now reads a\nlist of extension name suffixes -- /larastan and /phpstan-laravel --\nthat covers both and forks of either.\n\nThe rest is vocabulary. Comments crediting shared behaviour to Larastan\nby name now say \"the Laravel PHPStan extensions\", and the ones naming a\nspecific upstream class describe the behaviour instead: two extensions\nthat disagree make such a reference rot. References that are genuinely\nabout Larastan, such as ported test fixtures and pointers for\nunimplemented work, are left alone.\n\nFiles L53 (collection key types resolved from the column for keyBy,\ngroupBy and pluck) and L54 (audit custom-builder and relation-closure\ninference) for the places where the extensions have moved past what we\nmirror.",
          "timestamp": "2026-08-25T11:59:15-05:00",
          "tree_id": "2fa9d42f772eef35bccd0cf229a418db5e9b2e8b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4173661aceb558ce993586c10e4d0036406230f1"
        },
        "date": 1787677956243,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fe41071c85f2b81264331061e1d485e69ba7d7e5",
          "message": "`array_merge` describes everything it was handed\n\n`array_merge` sat in `ARRAY_PRESERVING_FUNCS` alongside `array_filter`\nand friends, so its result took the element type of its *first* argument.\nIt is the one member of that family that concatenates rather than\nrearranges, and the difference matters for the accumulator idiom:\n\n    $out = [];\n    foreach ($items as $item) {\n        $out = array_merge($out, $item->getParts());\n    }\n\nArg 0 is the empty `array{}` the accumulator started as, which names no\nelement type at all, so the rule declined and the stub's bare `array`\nstood. Every read off `$out` from there on had nothing to go by --\niterating it, and reading one entry back out by a variable index\n(`$out[$i]->method()`) behind an `array_key_exists()` check, both\nreported that the type could not be resolved.\n\n`array_merge` now has its own rule, matching PHPStan's own extension:\nthe value type is the union of every argument's, and the key type the\nunion of their key domains, with the result a `list` when every argument\npromises integer keys. An empty shape contributes neither and is\nskipped; an argument that names only its value type (`array<T>`, `T[]`)\nkeeps the result's key domain open rather than spelling it out as\n`int|string`, which would turn a `T[]` that signatures accept today into\none they reject. A spread, or an argument that cannot be typed at all,\ndeclines rather than claim a union missing a member -- which needed a\nnew `is_spread` on `ArrayFuncArgs`, since the rule now walks the whole\nlist instead of reading arg 0.\n\nAcross the reference corpora this clears 13 diagnostics on phpstan-src\n(11 of them the reported `$arraysToProcess[$i]` case) and 20 on psalm,\nand leaves the `projects/` corpus byte-identical.\n\nFiled B269 while confirming the one changed message that remains: an\nunqualified class name in an inline `@var` resolves to a same-named stub\nclass instead of the current namespace, so `@var list<Error>` inside\n`namespace PHPStan\\Analyser` compares unequal to `list<PHPStan\\Analyser\\Error>`.\n\nCo-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>",
          "timestamp": "2026-08-25T19:18:26+02:00",
          "tree_id": "fbcadf1ebc50d174009976a4788723ebcfd9f43a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fe41071c85f2b81264331061e1d485e69ba7d7e5"
        },
        "date": 1787679057048,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fe54fcc105c14d08d1d5a6396d95f38d817aad66",
          "message": "`deprecated_usage` no longer flags a deprecated method delegating to\nitself on another instance",
          "timestamp": "2026-08-25T19:21:24+02:00",
          "tree_id": "c987377ab41d1c47a1d62e7cc03fbd2231887029",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fe54fcc105c14d08d1d5a6396d95f38d817aad66"
        },
        "date": 1787680374057,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f864284ff100c33332ea751ece0a9d8b71beaff5",
          "message": "An inline `@var` reads a class name the way PHP reads it: from the\ncurrent namespace outwards",
          "timestamp": "2026-08-25T19:44:59+02:00",
          "tree_id": "b9b7e3d0cf5ce164b35e01a2bccd707cf1768894",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f864284ff100c33332ea751ece0a9d8b71beaff5"
        },
        "date": 1787688867465,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a622f4f5105ea02519b5c64c87fbad8ee0f88ad1",
          "message": "`array_merge()` describes everything it was handed, not just its first\nargument",
          "timestamp": "2026-08-25T22:41:22+02:00",
          "tree_id": "3070eba2284ed1283341d91e2cebe5634c98ea1d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a622f4f5105ea02519b5c64c87fbad8ee0f88ad1"
        },
        "date": 1787691451395,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "78e3e3ffec72385242e5b8dadcbdd9a150795cd7",
          "message": "A standard-library call whose result depends on its arguments no longer\ncarries every shape it could have returned",
          "timestamp": "2026-08-26T10:25:05+02:00",
          "tree_id": "beb4b3e752a305a5acbd4cd35389f4e999f9097f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/78e3e3ffec72385242e5b8dadcbdd9a150795cd7"
        },
        "date": 1787733479159,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ba8a526afc36fa251ebf79658eed139ec7630e95",
          "message": "Fix a few type issues",
          "timestamp": "2026-08-26T11:10:06+02:00",
          "tree_id": "816e89c53dbf8a88b0c7353ec8a180cd720a664f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ba8a526afc36fa251ebf79658eed139ec7630e95"
        },
        "date": 1787736174624,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7721931afd1ddbdfd999b4ac9f8ea3167ea37191",
          "message": "A guard that rules out two types at once leaves the value narrowed to\nthem",
          "timestamp": "2026-08-26T18:53:28+02:00",
          "tree_id": "f6e1bfcb228686a5d02ff18a6aed1f7853529c46",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7721931afd1ddbdfd999b4ac9f8ea3167ea37191"
        },
        "date": 1787764383062,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ce16c4c2c12fccfda845362db9b73e6052b18cb2",
          "message": "Fix a few type issues",
          "timestamp": "2026-08-26T23:39:11+02:00",
          "tree_id": "b7543c3188b9e6f10a3a29b2540754a3b5c72c89",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ce16c4c2c12fccfda845362db9b73e6052b18cb2"
        },
        "date": 1787781132740,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 67.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8744e5990c23760dd38390ca0316be2051073b6a",
          "message": "A class named `Scalar` or `Numeric` is a class, not a PHPDoc pseudo-type",
          "timestamp": "2026-08-27T05:40:32+02:00",
          "tree_id": "ffc87a0aa6d04f7b528c5a4c46c1ce90114a0915",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8744e5990c23760dd38390ca0316be2051073b6a"
        },
        "date": 1787802746516,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0fe3d825f7f538eb0a245d58ef7269d1503db0e9",
          "message": "Fix several type resolve issues",
          "timestamp": "2026-08-27T11:59:24+02:00",
          "tree_id": "4dea24bbcd3ea3f87962bef7ebca0cf0336d5056",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0fe3d825f7f538eb0a245d58ef7269d1503db0e9"
        },
        "date": 1787825510235,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4c5de77082323c110f2ff9288582062c47546e9d",
          "message": "Nothing is reported inside a branch the code rules out",
          "timestamp": "2026-08-27T14:37:39+02:00",
          "tree_id": "89ec658cbcf4ad4b860ca4f797bd238860e8b593",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4c5de77082323c110f2ff9288582062c47546e9d"
        },
        "date": 1787835051306,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "b4e7a4b97231df777bef019becd569a44815da53",
          "message": "A callable's signature is completed from where it is written, not only\nfrom what it declares",
          "timestamp": "2026-08-27T16:15:00+02:00",
          "tree_id": "950de113fbb536e56ca7e0dbbea3b5edd5304ef1",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/b4e7a4b97231df777bef019becd569a44815da53"
        },
        "date": 1787840880499,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "651f5c086c25da8b425bbd5cd76434ed9b151ac2",
          "message": "A value PHP itself leaves open is `mixed`, and the diagnostic says so",
          "timestamp": "2026-08-27T21:30:52+02:00",
          "tree_id": "028e23f36140f3d7f5899cef5e8e1f291e4d7cda",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/651f5c086c25da8b425bbd5cd76434ed9b151ac2"
        },
        "date": 1787859823644,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 68,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f544aca2eb5ebd06b7632c41012e0e283e0a41b0",
          "message": "A value PHP itself leaves open is `mixed`, and the diagnostic says so",
          "timestamp": "2026-08-27T21:31:02+02:00",
          "tree_id": "1b0bd43d72d2e4f458140d5a8e94bb16a85e17b3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f544aca2eb5ebd06b7632c41012e0e283e0a41b0"
        },
        "date": 1787859874895,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a9f329e2dcd3fc863a0bb0bb447f36eab8378ed2",
          "message": "Fix array narrowing",
          "timestamp": "2026-08-27T23:33:17+02:00",
          "tree_id": "afe271f2c4248418dd2d65fefd07f5bbaa8bbddc",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a9f329e2dcd3fc863a0bb0bb447f36eab8378ed2"
        },
        "date": 1787867237402,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "ef000d6691402f5f59df0321a258800314e28f50",
          "message": "Fix unknown members diagnostics bug",
          "timestamp": "2026-08-28T13:51:47+02:00",
          "tree_id": "75c48a4bba7c880f95ea45cdd100c0e961cd2ce3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/ef000d6691402f5f59df0321a258800314e28f50"
        },
        "date": 1787918700161,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7f6f70860d038d16a677cd2433f971c128860fcb",
          "message": "Fix a few narrowing issues",
          "timestamp": "2026-08-28T15:05:48+02:00",
          "tree_id": "796e16bd77dec707378be5e029ec5b28bc632aa8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7f6f70860d038d16a677cd2433f971c128860fcb"
        },
        "date": 1787923148698,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "38a169128d87b8c762e221a58cfb1c299dd54d2a",
          "message": "A `@param` docblock written inline between an assignment and the closure\nit documents still types the closure",
          "timestamp": "2026-08-28T17:28:55+02:00",
          "tree_id": "66d8fce4db3d655c9304701dc151654dd5be5734",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/38a169128d87b8c762e221a58cfb1c299dd54d2a"
        },
        "date": 1787931726902,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "f14bb576626549702b372e928765063d6ea06246",
          "message": "A check a boolean stands for reaches every place the boolean is read",
          "timestamp": "2026-08-29T10:46:27+02:00",
          "tree_id": "6bf5e07ee1efc8eed0aa55c64c013662c7a895ae",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f14bb576626549702b372e928765063d6ea06246"
        },
        "date": 1787993935596,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a7ed660e36011adff555dc2df2a669039775a8d9",
          "message": "A loop over an array the code proved has entries no longer leaves the\nsentinel above it behind",
          "timestamp": "2026-08-29T13:41:30+02:00",
          "tree_id": "c04edf0193e7c8bfbcb161bc04cd6d1d4d05e844",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a7ed660e36011adff555dc2df2a669039775a8d9"
        },
        "date": 1788004487286,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6789da823d7b9d35cab8c5a8357b2abc220fee73",
          "message": "A proof the condition never states outright is reconstructed where it is\nread",
          "timestamp": "2026-08-29T18:41:10+02:00",
          "tree_id": "2c7f74c3f398ef127d64914ae11f521f622a23e5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6789da823d7b9d35cab8c5a8357b2abc220fee73"
        },
        "date": 1788022411239,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6c8c06f2e9f6e5407321fb833ef9e72e31bde5c7",
          "message": "A by-reference out-parameter is typed by what the callee writes, and the\nvalue it already held is left alone",
          "timestamp": "2026-08-29T18:51:44+02:00",
          "tree_id": "284bbe1fae3b5ddfcd8a6c44870fa37f9cb19074",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6c8c06f2e9f6e5407321fb833ef9e72e31bde5c7"
        },
        "date": 1788024694012,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e8758dcf6c971c7788f95c823b04f9ed3a0de0c3",
          "message": "A relative class name in a static call or `new` expression still finds\nthe by-reference parameters it passed",
          "timestamp": "2026-08-29T19:28:10+02:00",
          "tree_id": "1644f000ed89f8d151197a6494a16448c7cf82e2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e8758dcf6c971c7788f95c823b04f9ed3a0de0c3"
        },
        "date": 1788025270588,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a47fc196b1baae061e04ba462277030126bbc24c",
          "message": "The branch a truthy test skips knows what the value could still have\nbeen",
          "timestamp": "2026-08-29T20:21:17+02:00",
          "tree_id": "ec4cfa5d2888d66e84a3bf3f7616759914c88b3b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a47fc196b1baae061e04ba462277030126bbc24c"
        },
        "date": 1788028451056,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "268cb53c311a57bc4beb096642d74911c147582c",
          "message": "The branch a truthy test skips knows what the value could still have\nbeen",
          "timestamp": "2026-08-29T20:21:24+02:00",
          "tree_id": "9f7c817ed4f3cc187f5b52493dfea31eb4573627",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/268cb53c311a57bc4beb096642d74911c147582c"
        },
        "date": 1788028462701,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "341ff96aa3d77d42e961fceb664e6b43fcdc5b75",
          "message": "An `instanceof` written beside an unrelated operand in an `||` no longer\ntypes the branch as though it had held",
          "timestamp": "2026-08-29T20:21:59+02:00",
          "tree_id": "40aa800e463154007afeb94bb3132708b58fef07",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/341ff96aa3d77d42e961fceb664e6b43fcdc5b75"
        },
        "date": 1788028496827,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1082bb7ba7fcbbd10a2a16769479eaa47f4be244",
          "message": "A branch that narrows a value to two classes at once no longer erases\nwhat the other path carried",
          "timestamp": "2026-08-29T20:49:42+02:00",
          "tree_id": "ac4798299c38133c1788922f60ed49c9be7e7919",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1082bb7ba7fcbbd10a2a16769479eaa47f4be244"
        },
        "date": 1788030174227,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9e6a213b0f3d843b34c5b7016663da0f0fd51119",
          "message": "A call inside a loop that names a member the class does not have says\nso, instead of blaming the variable it was assigned to",
          "timestamp": "2026-08-29T21:44:01+02:00",
          "tree_id": "f16ab3cd0ddafa6fe1adf360a2d546ace5614753",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9e6a213b0f3d843b34c5b7016663da0f0fd51119"
        },
        "date": 1788033407871,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0d38a48fcabbf904d6e10193d0a9dfd1f34e4e1f",
          "message": "Fix a couple of narrowing issues",
          "timestamp": "2026-08-29T22:03:12+02:00",
          "tree_id": "fe7ba1686c526e495465dcc5c5bfc195a13bcfb4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0d38a48fcabbf904d6e10193d0a9dfd1f34e4e1f"
        },
        "date": 1788034598178,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cdb21e51be591965475bee46078dcf48d7961f52",
          "message": "Fix a couple of narrowing issues",
          "timestamp": "2026-08-29T23:10:32+02:00",
          "tree_id": "d1797845b925f326d1482957fb1e6e7d0962b8c6",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cdb21e51be591965475bee46078dcf48d7961f52"
        },
        "date": 1788038735657,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "e4769f9897fbe21ae93f756e27495faa85469cc4",
          "message": "Iterating an array whose docblock names only a value type binds the key\nPHP really hands out",
          "timestamp": "2026-08-29T23:44:07+02:00",
          "tree_id": "8c378996dee25982442bda4ad663baa8204a5b38",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/e4769f9897fbe21ae93f756e27495faa85469cc4"
        },
        "date": 1788040639821,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "716684c3fe0610dd979e092c9543c73e781a11be",
          "message": "Re-testing a call or a property path re-applies what the first test's\nbranch proved",
          "timestamp": "2026-08-29T23:46:07+02:00",
          "tree_id": "aaa4de232beaf0fe5ffb8622945405d2401f8b9a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/716684c3fe0610dd979e092c9543c73e781a11be"
        },
        "date": 1788040659214,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0837ea0df0199772925632a015bf3fed200cef1d",
          "message": "An array collected out of keys nobody described keeps the leniency a\nsingle key already had",
          "timestamp": "2026-08-30T00:18:44+02:00",
          "tree_id": "c3c3f333553b23b3d3ed7256f63f698a71b78372",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0837ea0df0199772925632a015bf3fed200cef1d"
        },
        "date": 1788042699657,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "a8767c2269d0fdea461397b86dc58586d4ed3d3a",
          "message": "An assignment used as a value resolves to what it just wrote",
          "timestamp": "2026-08-30T00:20:08+02:00",
          "tree_id": "f591160e56c2140da1c68855eb71086375b20d01",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/a8767c2269d0fdea461397b86dc58586d4ed3d3a"
        },
        "date": 1788042780122,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "82feb3dcf7473ec62c295dd20838acded769dd2b",
          "message": "A value the type engine holds leniently stays lenient past an `if` and\ninside an array key",
          "timestamp": "2026-08-30T11:39:52+02:00",
          "tree_id": "c5d04555357603623bcca3df936b2abbfe99aa12",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/82feb3dcf7473ec62c295dd20838acded769dd2b"
        },
        "date": 1788083567853,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8092ae2e398b288a05b895f18029928314b8ac26",
          "message": "`analyze` takes more than one path",
          "timestamp": "2026-08-30T12:13:07+02:00",
          "tree_id": "d7c4762d7f6a425e5f3c8e9b165116e540fe44f5",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8092ae2e398b288a05b895f18029928314b8ac26"
        },
        "date": 1788085580751,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "2b22445de4bb80937bbf9940b0924358d66298bf",
          "message": "Round off some index eclude cases",
          "timestamp": "2026-08-31T15:42:17+02:00",
          "tree_id": "4154c7f10e6b52fa93e65b2b9e7dfd0ef2ba13b2",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/2b22445de4bb80937bbf9940b0924358d66298bf"
        },
        "date": 1788184529340,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 69.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "5aa55e0beaf4ff802df9622e6dbcaf6777d7f36b",
          "message": "feat(php): add call hierarchy\n\nReuse the existing definition and reference pipelines for standard\nincoming and outgoing call navigation.",
          "timestamp": "2026-08-31T16:20:23+02:00",
          "tree_id": "794c3b9c88a92c60eee4e038589c3d83b05945df",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/5aa55e0beaf4ff802df9622e6dbcaf6777d7f36b"
        },
        "date": 1788186759985,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "08cb8c9a82d657677ff28121191cdf3100f2b669",
          "message": "Blade directives a project registers itself",
          "timestamp": "2026-08-31T17:36:51+02:00",
          "tree_id": "117439faee432538f6dc1dc1c653eaf99b6155d8",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/08cb8c9a82d657677ff28121191cdf3100f2b669"
        },
        "date": 1788191390849,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6d32fa002113d2c297200fc98cc9a1883f3c5db2",
          "message": "A `@class`/`@style`/`@json` (and other attribute-style) directive\nwritten before a component tag no longer mistypes its bound attributes",
          "timestamp": "2026-08-31T21:00:37+02:00",
          "tree_id": "d4c488ebd9a7d5feaa812ee4dd738b6e345030c9",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6d32fa002113d2c297200fc98cc9a1883f3c5db2"
        },
        "date": 1788203633118,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "9ce5182c48620abc43e34db9da2cb4601a73585c",
          "message": "fix: oving a class into another namespace adds the import its former namespace-siblings were relying on\n\nA file in the same namespace as a class reaches it by short name with\nno `use` statement at all, so the class-move rename had nothing to\nrewrite there: it updated the `use` line wherever one existed and\nwalked straight past the files where the shared namespace had been\ndoing that job. Those files were left spelling a name that no longer\nresolves, which nothing reports until the code runs.\n\nSuch a file now gains the import as part of the same edit, inserted\nthrough the existing use-block analysis so it lands in alphabetical\norder among the imports already there. Where the short name is\nalready taken by an unrelated import, the new one is aliased and the\nreferences are rewritten to the alias, which needed a use-statement\nbuilder that can emit an `as` clause; `build_use_edit` delegates to it\nso its existing callers are unchanged.\n\nThe import is only added when the file actually spells the class by\nits short name, so a file that writes the fully-qualified name keeps\ngetting its references rewritten in full and no import, and a file\nalready sitting in the namespace the class moved into is left alone.",
          "timestamp": "2026-08-31T17:02:20-05:00",
          "tree_id": "a1500ede36ee1ac6c71f5d802735f5ea5dc14500",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9ce5182c48620abc43e34db9da2cb4601a73585c"
        },
        "date": 1788214525130,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "6010a21b6b51384701f1b69fb6977ded51d698d0",
          "message": "A rename whose destination is already taken merges or refuses, instead of scattering files\n\nTwo shapes of move emitted file operations without checking whether\nanything was already at the far end.\n\nA namespace rename was always a single rename of the source directory\nonto the destination. Where the destination already existed that\noperation cannot be carried out, but the accompanying text edits had\nalready been re-pointed at the paths it was supposed to create, so the\nrename failed, the edits landed anyway, and the editor created a\nscatter of files holding nothing but a rewritten `namespace` line.\nMerging into an existing namespace now moves the files into it one at\na time, each keeping its path relative to the namespace root; a\ndestination that does not exist yet still moves as one directory.\n\nA class move onto an FQN another class already declares, or onto a\nfile that already exists, emitted the whole edit including the rename\nthat would clobber the far end. So did a namespace merge where both\nsides declare the same class name, which has no well-defined answer:\nmoving the rest and leaving the clash behind still rewrites every\nreference to it so it names the class that was already there, which\ncompiles and is wrong. All three now refuse and emit nothing.\n\nRefusing needs a reason the user can read, so `handle_rename` answers\n`Result<Option<WorkspaceEdit>, String>` and both front ends turn the\nerror into a failed request, which is the only part of the rename\nprotocol an editor surfaces. `Ok(None)` stays the silent \"nothing\nrenameable here\" answer.\n\nAlso fixes a latent bug in the same path: the rewrite that re-points a\nmoved file's edits matched the old directory as a bare string prefix,\nso renaming `App\\Internal` claimed the edits of `App\\InternalTools`.\nIt now requires a path separator after the match.\n\nThe partial merge that would move everything except the clashing names\nis filed as F22 rather than done; leaving the clash behind correctly\nneeds per-class granularity the namespace rename does not have yet.",
          "timestamp": "2026-08-31T19:53:08-05:00",
          "tree_id": "38ab3e6c80feb08d82fe603cf83a740f7b68d6ba",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6010a21b6b51384701f1b69fb6977ded51d698d0"
        },
        "date": 1788224787470,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "0340c45c41d3757898f257e242b40e7e26d49fff",
          "message": "A class named inside a `@phpstan-type` or `@phpstan-import-type` is a reference to it\n\nBoth tags were already read for their types: the aliases they declare\nresolve, expand through inheritance chains, and drive completion. What\nwas missing is that the class names written inside them were never\nrecorded in the symbol map, because `emit_tag_symbols` had no arm for\n`TagValue::TypeAlias` or `TagValue::TypeAliasImport` and both fell\nthrough to the catch-all.\n\nThat span is what every name-level feature reads, so the omission went\nwell past colour. The tag name was highlighted (the generic `@word`\nscan catches it) and the whole rest of the line came back as one flat\ncomment run; go-to-definition on the class did nothing; it appeared in\nneither find-references nor document-highlight; and a class rename\nwalked past it, leaving the alias naming a class that no longer\nexists. The last of those is silent breakage, not a cosmetic gap.\n\nThe type behind a `@phpstan-type` now goes through `emit_type_symbols`\nlike any other docblock type, and the identifier after a\n`@phpstan-import-type`'s `from` through `emit_identifier_span`. The\n`@psalm-` and bare `@type` spellings parse to the same values and are\ncovered by a test rather than assumed.\n\nThe alias names themselves deliberately get no span: `UserRow` in\n`@phpstan-type UserRow …` and `Row` in `… as Row` are not classes, so\nclaiming them would resolve to nothing and feed the unknown-class\ndiagnostic. An alias referenced inside another alias is still not\nreported, since `unknown_classes` already skips names it finds in\n`type_aliases` — `analyze examples/php` reports the same 101\ndiagnostics before and after.",
          "timestamp": "2026-08-31T21:33:21-05:00",
          "tree_id": "02354459ec65a82d9dafc6fb163d3e672a0f6e6b",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0340c45c41d3757898f257e242b40e7e26d49fff"
        },
        "date": 1788230815055,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.5,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "cd7db5708ac27cc5060570783a8a9377d8f4206a",
          "message": "Document outline for Blade files",
          "timestamp": "2026-09-01T11:17:31+02:00",
          "tree_id": "736587a5eb5f69a7cb046defd38f833b47d8bd6f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/cd7db5708ac27cc5060570783a8a9377d8f4206a"
        },
        "date": 1788255072751,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "f2456f385cfb8e4aba97984754e66e4b5a39e987",
          "message": "fix: offer import actions at point cursors",
          "timestamp": "2026-09-01T09:48:16-05:00",
          "tree_id": "5ad847dee6d69208cd4c9e49bc1726f3ffa882a4",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f2456f385cfb8e4aba97984754e66e4b5a39e987"
        },
        "date": 1788274816862,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.4,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "6a4c95b0f9497edb13ce54f014a7860d3c766b2c",
          "message": "feat: move classes and namespaces from the command line\n\nRenaming a class's FQN or a namespace segment already moves the files\nand rewrites the references across a project, but only through\ntextDocument/rename, which needs a cursor position and hands the\nresulting WorkspaceEdit to an editor to apply. That leaves the bulk\ncase — a migration script, a pre-commit hook, an agent working through\na list of classes — with no way in short of driving an editor.\n\n`phpantom_lsp move FROM TO` exposes the same engine headlessly. Either\nside accepts a fully-qualified name or a Composer PSR-4 path, so a\nclass or a whole namespace can be named however the caller already has\nit to hand, and `--dry-run --format json` gives a validation-only form\nfor scripts.\n\nTwo cursor-free planners drive the existing rename builders rather than\nduplicating them, and the CLI applies the WorkspaceEdit itself: every\nfile is read and every edit computed in memory, and every destination\nchecked, before anything is written, so a refused move leaves the tree\nexactly as it was.\n\nMoving a class into a namespace no PSR-4 mapping covers rewrites the\ndeclaration but cannot take the file with it, leaving the class\nsomewhere the autoloader will not find it. Reporting that as a plain\nsuccess would hand a script a quietly broken project, so it is called\nout on stderr and in the JSON output instead.",
          "timestamp": "2026-09-01T21:16:50+02:00",
          "tree_id": "fac670182d27639b39b6eabaf4a9121d9b84ecb7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/6a4c95b0f9497edb13ce54f014a7860d3c766b2c"
        },
        "date": 1788290987258,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "73c9caddcb86b4e1ed2d41ba9efee5287028fa60",
          "message": "feat: import qualified symbols with a code action\n\nAllow qualified class, function, and constant references to be replaced with imports and short names across the current namespace. Generate aliases when existing imports occupy the natural short name.",
          "timestamp": "2026-09-03T12:34:01-05:00",
          "tree_id": "bd1bfed8143ab8109196908ae808461ace8cc76f",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/73c9caddcb86b4e1ed2d41ba9efee5287028fa60"
        },
        "date": 1788457891840,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "committer": {
            "email": "cdwhite3@pm.me",
            "name": "Caleb White",
            "username": "calebdw"
          },
          "distinct": true,
          "id": "9c3f121059da6b9a8c3d1e910a6cdfe06cb17b66",
          "message": "test: distinguish import class quick fixes",
          "timestamp": "2026-09-03T12:56:29-05:00",
          "tree_id": "177c385111e4e7788b33169cc70d9b5dafe7d907",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9c3f121059da6b9a8c3d1e910a6cdfe06cb17b66"
        },
        "date": 1788459203831,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fff13f58619f86f770216102122fcfe03ae2cc5f",
          "message": "Add back \"Import all FQCNs\"",
          "timestamp": "2026-09-05T01:55:44+02:00",
          "tree_id": "2438c2e1dac388ff53adfda64d80218fc547cc9a",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fff13f58619f86f770216102122fcfe03ae2cc5f"
        },
        "date": 1788567122995,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "590dade75d4afeb060d2a6ab00c3423d03fb4d80",
          "message": "A namespace move rewrites each reference the way the file holding it\nactually reads",
          "timestamp": "2026-09-05T02:49:03+02:00",
          "tree_id": "3aad2be7c929e9a9961ad8e341201a2cb1f03556",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/590dade75d4afeb060d2a6ab00c3423d03fb4d80"
        },
        "date": 1788570318262,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "88db8ec3deec2958ad29482b87df07425c037a9f",
          "message": "Renames and moves reach Blade templates",
          "timestamp": "2026-09-05T03:51:21+02:00",
          "tree_id": "a52998de92429c1d2cc6e71c39c7b8b19da1ac54",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/88db8ec3deec2958ad29482b87df07425c037a9f"
        },
        "date": 1788574102069,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7d2ff12e51b9cbf0f52ffe6bfaa449fe98a51f86",
          "message": "The Blade lowering's own declarations stay out of the project's symbols",
          "timestamp": "2026-09-05T23:32:10+02:00",
          "tree_id": "4a12743789f21d9cbe936e291654dd9f7dcd3485",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7d2ff12e51b9cbf0f52ffe6bfaa449fe98a51f86"
        },
        "date": 1788644883952,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "4ac5bc0aa2059e8de24a7cf4dc1e26c9cc67a2f7",
          "message": "A moved class keeps the namespace-siblings it was reaching by short name",
          "timestamp": "2026-09-06T00:00:33+02:00",
          "tree_id": "921681f496f8acd8c547ae88963d3fc70b776901",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/4ac5bc0aa2059e8de24a7cf4dc1e26c9cc67a2f7"
        },
        "date": 1788646672653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "9ed45bfe1c49f3d958f82057893fa7e8d09ba58a",
          "message": "Moving a class into the global namespace removes its `namespace`\ndeclaration",
          "timestamp": "2026-09-06T21:37:49+02:00",
          "tree_id": "c99a5286d603eeef7d1a9858aa794b901d5fb18e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/9ed45bfe1c49f3d958f82057893fa7e8d09ba58a"
        },
        "date": 1788724496581,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "c8494bf88b5cd16518852c5a9fced3554bd96104",
          "message": "A rewritten import keeps its indentation",
          "timestamp": "2026-09-06T21:56:58+02:00",
          "tree_id": "fdeff91df1d9e1f6456b4d86afbb2d2bec9ebf5d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c8494bf88b5cd16518852c5a9fced3554bd96104"
        },
        "date": 1788725643849,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 76.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "102437433+iz-ahmad@users.noreply.github.com",
            "name": "Nafis Ahmad",
            "username": "iz-ahmad"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c2cd2ce991f45001d17efed0e1c59cb97d6a2d49",
          "message": "Add downgrade-nullable-argument-mismatch config option for nullability-only argument type mismatches (#426)",
          "timestamp": "2026-09-07T01:24:41+02:00",
          "tree_id": "634160ba9511e2a570001d8f80a4694e1361258e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/c2cd2ce991f45001d17efed0e1c59cb97d6a2d49"
        },
        "date": 1788738114814,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.8,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "20650a906a4feae1f03eab9b4b25461c1c30ea85",
          "message": "Add Laravel storage disk name intelligence (#371)",
          "timestamp": "2026-09-07T02:38:58+02:00",
          "tree_id": "ac1493f62d8a423cad2dfadd9ee63ea65daef080",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/20650a906a4feae1f03eab9b4b25461c1c30ea85"
        },
        "date": 1788742549242,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.7,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.1,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "petr@mediasolution.cz",
            "name": "petrovo-as",
            "username": "petrovo-as"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "8e3a29d9e9cc8c389c5d8e8183e1eabea35501b1",
          "message": "Report a `private` or `protected` member reached from outside its scope\n\nPHP resolves a member access to a declaration first and enforces that\ndeclaration's visibility second, so reading a private property from\noutside its class is a fatal error rather than a missing member. Neither\nwas reported.\n\nThe check runs inside the unknown-member walk, which has already\nresolved the subject expression to a class. It looks the member up in\nthe raw declarations — the class as parsed, then its traits, then its\nancestors — rather than in the merged class, because the inheritance\nmerge drops a parent's private members and would make them look absent\ninstead of unreachable. Walking the raw chain is also what supplies the\ndeclaring class, which is the scope `private` and `protected` are\nmeasured against: a member declared on a shared parent stays reachable\nfrom every branch below it while one declared on a sibling does not.\n\nProperties, methods, class constants, and static properties are checked.\nNothing is reported unless a declaration is positively found and\npositively out of reach, so an unresolvable ancestor, a virtual member,\nor a class the loader cannot produce end in silence. A class declaring\n`__get`, `__set`, `__call`, or `__callStatic` anywhere in its hierarchy\nanswers for members the caller cannot see directly and is left alone, as\nis a trait body, whose host class is unknown, and a `@see` tag, which\ndocuments a member rather than reading one.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-07T04:01:36+02:00",
          "tree_id": "359850c515b8170f8ace33cd13d2a02d35f5d967",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/8e3a29d9e9cc8c389c5d8e8183e1eabea35501b1"
        },
        "date": 1788747527201,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "fc644acf1343cb9c74f92b5a9f7e422ce0ee9bd8",
          "message": "Support custom Eloquent pivot accessor names - #381",
          "timestamp": "2026-09-07T10:40:43+02:00",
          "tree_id": "b4f44c1f3c29dbaab8a0d441a190dd84e78d0b89",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fc644acf1343cb9c74f92b5a9f7e422ce0ee9bd8"
        },
        "date": 1788771494466,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.6,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 71.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "1242225+sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f46ea5d80341ab251a7d16edcea445ddb97fa2a9",
          "message": "feat(php): add scalable reference CodeLens (#392)",
          "timestamp": "2026-09-07T11:35:16+02:00",
          "tree_id": "2e3f188034d64d7ebc453eed0fe933a8c520123e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/f46ea5d80341ab251a7d16edcea445ddb97fa2a9"
        },
        "date": 1788774755586,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.9,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "shuvro.nsu.cse@gmail.com",
            "name": "Shuvro Roy",
            "username": "shuvroroy"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "93860a7cb0cf63c0a357838ef3225be3391ddb9e",
          "message": "fix(blade): ignore escaped frontend interpolations\n\nTreat @{{ ... }} and @{!! ... !!} as literal template text so\nfrontend-only expressions never reach the PHP parser.\n\nCloses #419",
          "timestamp": "2026-09-08T00:02:03+02:00",
          "tree_id": "6d9865bf34152f05a3b1532925dbe1acb3c3492e",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/93860a7cb0cf63c0a357838ef3225be3391ddb9e"
        },
        "date": 1788819556682,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 38,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 74.7,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "nguyentranchung52th@gmail.com",
            "name": "Chung",
            "username": "nguyentranchung"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "d4f0ce2119ee3c82b0a54ba9c73585be4c68728d",
          "message": "chore: update Mago dependencies",
          "timestamp": "2026-09-08T00:43:22+02:00",
          "tree_id": "2439bf95fa12699860b6cb5fc4f1a6710240cce3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/d4f0ce2119ee3c82b0a54ba9c73585be4c68728d"
        },
        "date": 1788821988667,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 77.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "1242225+sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "969ac2d013b4dda80d590b0f1511baa87799ef75",
          "message": "feat(navigation): navigate PHP symbols in YAML and XM",
          "timestamp": "2026-09-08T02:22:17+02:00",
          "tree_id": "41d41cce4994802d1306885c9c62171fc0089311",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/969ac2d013b4dda80d590b0f1511baa87799ef75"
        },
        "date": 1788827845664,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.8,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "fa7f03175fd20c2e59df3c30d8b0423b14b7f30f",
          "message": "fix(types): Normalize nullable array shapes\n\nTreat union-with-null and nullable wrappers as the same shape during\nrepeated branch merges.",
          "timestamp": "2026-09-08T02:56:29+02:00",
          "tree_id": "ac2e8eed6272e6f7d686b24cf84d9ef9b67caf2d",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/fa7f03175fd20c2e59df3c30d8b0423b14b7f30f"
        },
        "date": 1788830023080,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 35.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 73.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "sidux@users.noreply.github.com",
            "name": "sidux",
            "username": "sidux"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "75e18e394f42323679a8ee86c8621b43050409b1",
          "message": "fix(release): support unsigned macOS builds\n\nKeep signing and notarization enabled when the complete Apple credential\nset is available. Package unsigned macOS artifacts when credentials are\nabsent so release workflows remain usable from forks.",
          "timestamp": "2026-09-08T03:04:23+02:00",
          "tree_id": "4abebcb5f42e7b0f90d4414adb97f182e1988f17",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/75e18e394f42323679a8ee86c8621b43050409b1"
        },
        "date": 1788830445253,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.1,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 75.6,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "1d78397550c491416f3ae2242f3728e5aa217acf",
          "message": "\"Create missing view\" quick fix",
          "timestamp": "2026-09-08T21:58:22+02:00",
          "tree_id": "3183727849bcbe06b5b93fcb31b11ac872bf0964",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/1d78397550c491416f3ae2242f3728e5aa217acf"
        },
        "date": 1788898567353,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 36.9,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 70.2,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "7ecf35e1ca386e54c5088092717daab582f060b6",
          "message": "A directive between an echo's opening tag and its terminator no longer\nbreaks the rest of the template",
          "timestamp": "2026-09-08T22:09:00+02:00",
          "tree_id": "d60abdef3919d20b6ece5ca5374d94ed596867e3",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/7ecf35e1ca386e54c5088092717daab582f060b6"
        },
        "date": 1788899167634,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.3,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 78.4,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "0f3633f7026cc871b09e3605767578e280166bcc",
          "message": "Refactor blade handeling",
          "timestamp": "2026-09-08T22:18:19+02:00",
          "tree_id": "51f8826f09d84ef24f5af47f45b04bcea201dafa",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/0f3633f7026cc871b09e3605767578e280166bcc"
        },
        "date": 1788902544077,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.2,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "committer": {
            "email": "anders@jenbo.dk",
            "name": "Anders Jenbo",
            "username": "AJenbo"
          },
          "distinct": true,
          "id": "bb627c4339fcae17bcb2a13ebad2ac5a3b9cffde",
          "message": "Echo-delimiter hover and go-to-definition no longer fire on a `{{`/`}}`\nlookalike",
          "timestamp": "2026-09-08T23:54:35+02:00",
          "tree_id": "8c91d7967adb6b41104925c2f9a5e1ff071bb9c7",
          "url": "https://github.com/PHPantom-dev/phpantom_lsp/commit/bb627c4339fcae17bcb2a13ebad2ac5a3b9cffde"
        },
        "date": 1788905521940,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "memory_hello_world",
            "value": 37.5,
            "unit": "MiB"
          },
          {
            "name": "memory_laravel_model",
            "value": 72.3,
            "unit": "MiB"
          }
        ]
      }
    ]
  }
}