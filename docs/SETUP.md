# Installation & Editor Setup

## Installation

### Homebrew (macOS and Linux)

```bash
brew install phpantom-lsp
```

### Cargo

```bash
cargo install phpantom_lsp --locked
```

See [phpantom_lsp on crates.io](https://crates.io/crates/phpantom_lsp).

### Pre-built Binaries

Download the latest binary for your platform from [GitHub Releases](https://github.com/AJenbo/phpantom_lsp/releases/latest). Available for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

### Build from Source

See [BUILDING.md](BUILDING.md) for full instructions. Quick version:

```bash
cargo build --release
# Binary is at target/release/phpantom_lsp
```

## Editor Setup

PHPantom communicates over stdin/stdout using the standard [Language Server Protocol](https://microsoft.github.io/language-server-protocol/). Any editor with LSP support can use it. Point the client at the `phpantom_lsp` binary with `php` as the file type. No special initialization options are required.

<details>
<summary><b>Zed</b></summary>

A Zed extension is included in the `zed-extension/` directory:

1. Ensure you have `rustc` available in your `$PATH`. This is part of the Rust [toolchain](https://rust-lang.org/tools/install/)
2. Open Zed
3. Open the Extensions panel
4. Click **Install Dev Extension**
5. Select the `zed-extension/` directory

The extension automatically downloads the correct pre-built binary from GitHub releases for your platform. If you'd prefer to use a locally built binary, ensure `phpantom_lsp` is on your `PATH` and the extension will use it instead.

To make PHPantom the default PHP language server, add to your Zed `settings.json`:

```json
{
  "languages": {
    "PHP": {
      "language_servers": ["phpantom_lsp", "!intelephense", "!phpactor", "!phptools", "..."]
    }
  }
}
```

</details>

<details>
<summary><b>Neovim</b></summary>

PHPantom is included in [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig). If you use nvim-lspconfig, enable it with:

```lua
require('lspconfig').phpantom.setup({})
```

Alternatively, with Neovim's built-in LSP client (no plugins required):

```lua
vim.lsp.config['phpantom'] = {
  cmd = { 'phpantom_lsp' },
  filetypes = { 'php' },
  root_markers = { 'composer.json', '.git' },
}
vim.lsp.enable('phpantom')
```

</details>

<details>
<summary><b>VS Code / Cursor</b></summary>

Install the [PHPantom extension](https://marketplace.visualstudio.com/items?itemName=phpantom.phpantom) from the VS Code Marketplace. It automatically downloads the language server binary and starts it when you open a PHP file.

</details>

<details>
<summary><b>PHPStorm</b></summary>

1. **Download PHPantom LSP binary**

   * Get it from [GitHub Releases](https://github.com/AJenbo/phpantom_lsp/releases/latest)
   * Extract the binary to a preferred location

2. **Install and configure LSP plugin**

   * Go to **Editor → Plugins** and install [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij)
   * Restart PHPStorm
   * Navigate to **Languages & Frameworks → Language Servers**
   * Click **+** to add a new server

     * Name: `PHPantom`
     * Command: path to your PHPantom binary
     * Mapping: set `PHP` on both the **Language** tab and the **File Type** tab (the dialogs are identical). Setting both ensures PHPStorm activates the server reliably.

<img width="779" height="645" alt="PHPStorm new language server dialog" src="https://github.com/user-attachments/assets/2da88e68-d012-476e-82e7-977dbfcd9653" />

<img width="779" height="645" alt="PHPStorm language server mapping dialog" src="https://github.com/user-attachments/assets/62358f9e-973c-487d-ac17-098d7dab007e" />

</details>

<details>
<summary><b>Sublime Text</b></summary>

1. **Install the LSP package.** Open the Command Palette (`Ctrl+Shift+P` on Linux/Windows, `Cmd+Shift+P` on macOS), type `Package Control: Install Package`, press Enter, then search for `LSP` and install it.

2. **Configure PHPantom.** Open the Command Palette again and type `Preferences: LSP Server Configurations`. This opens `LanguageServers.sublime-settings`. Add the following:

```json
{
  "phpantom": {
    "enabled": true,
    "command": ["phpantom_lsp"],
    "selector": "embedding.php",
    "priority_selector": "source.php"
  }
}
```

Make sure `phpantom_lsp` is on your `PATH`, or replace it with the full path to the binary.

</details>

<details>
<summary><b>Helix</b></summary>

Helix has built-in LSP support. Add PHPantom to your `languages.toml` (typically `~/.config/helix/languages.toml`):

```toml
[language-server.phpantom]
command = "phpantom_lsp"

[[language]]
name = "php"
language-servers = ["phpantom"]
```

</details>

<details>
<summary><b>Emacs (eglot)</b></summary>

> [!NOTE]
> This configuration is untested. If you get it working (or run into issues), please [open an issue](../../issues).

Eglot is built into Emacs 29+. Add to your `init.el`:

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(php-mode . ("phpantom_lsp"))))
```

Then open a PHP file and run `M-x eglot`.

</details>

<details>
<summary><b>Emacs (lsp-mode)</b></summary>

> [!NOTE]
> This configuration is untested. If you get it working (or run into issues), please [open an issue](../../issues).

Add to your `init.el`:

```elisp
(with-eval-after-load 'lsp-mode
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection '("phpantom_lsp"))
    :activation-fn (lsp-activate-on "php")
    :server-id 'phpantom)))
```

Then open a PHP file and run `M-x lsp`.

</details>

<details>
<summary><b>Kate</b></summary>

> [!NOTE]
> This configuration is untested. If you get it working (or run into issues), please [open an issue](../../issues).

Kate (KDE) has built-in LSP support. Open **Settings → Configure Kate → LSP Client → User Server Settings** and add:

```json
{
  "servers": {
    "php": {
      "command": ["/path/to/phpantom_lsp"],
      "url": "https://github.com/AJenbo/phpantom_lsp"
    }
  }
}
```

</details>
<details>
<summary><b>Opencode</b></summary>
You can add this to your `opencode.json` file, it will be invoked when the AI uses the edit tool to edit a PHP file from within opencode.

```json
{
  "$schema": "https://opencode.ai/config.json",
  "lsp": {
    "php": {
      "command": ["phpantom_lsp", "--stdio"],
      "extensions": [".php"]
    }
  }
}
```

You can add additional extensions to invoke it on if appropriate for your project.  
</details>

## Project Configuration

PHPantom works best with Composer projects. It reads `composer.json` to discover autoload directories and vendor packages, so completions and go-to-definition only surface classes that your autoloader can actually load. Projects without `composer.json` fall back to scanning every PHP file in the workspace.

### `.phpantom.toml`

PHPantom supports an optional per-project configuration file for settings like PHP version overrides and diagnostic toggles.

To generate a default config file with all options documented and commented out:

```bash
phpantom_lsp init
```

This creates a `.phpantom.toml` in the current directory. Currently supported settings:

```toml
[php]
# Override the detected PHP version (default: inferred from composer.json, or 8.5).
# version = "8.5"

[diagnostics]
# Report member access on subjects whose type could not be resolved.
# Useful for discovering gaps in type coverage. Off by default.
# unresolved-member-access = true

[indexing]
# How PHPantom discovers classes across the workspace.
#   "composer" (default) - use Composer classmap, self-scan on fallback
#   "self"    - always self-scan, ignore Composer classmap
#   "none"    - no proactive scanning, Composer classmap only
# strategy = "composer"

[formatting]
# Explicit path to an external formatter: always use this tool and skip
# require-dev auto-detection. Set to "" to disable that tool entirely.
# pint = "/usr/local/bin/pint"
# php-cs-fixer = "/usr/local/bin/php-cs-fixer"
# phpcbf = "/usr/local/bin/phpcbf"

# Timeout for external formatters, in milliseconds (default: 10000).
# timeout = 10000
```

The file is optional. When absent, all settings use their defaults. New settings will be added as features land. Unknown keys are silently ignored, so the file is forward-compatible.

### Code Formatting

PHPantom ships a built-in PHP formatter (mago-formatter) that works out of the box, so `textDocument/formatting` requests are answered without any setup. The formatter is chosen per project in this order:

1. **Explicit config wins.** A tool path set under `[formatting]` in `.phpantom.toml` (`pint`, `php-cs-fixer`, or `phpcbf`) is always used. Setting a tool to `""` disables it.
2. **Composer `require-dev` wins over the built-in formatter.** If `composer.json` lists `laravel/pint`, `friendsofphp/php-cs-fixer`, or `squizlabs/php_codesniffer` in `require-dev`, PHPantom resolves the binary through Composer's bin-dir and runs it as a subprocess. These tools discover their own project config (`pint.json`, `.php-cs-fixer.php`, `.phpcs.xml`, etc.) as they normally would.
3. **Otherwise, the built-in formatter is used.**

The built-in formatter defaults to the PER-CS 2.0 style. If a `mago.toml` is present at the workspace root, its `[formatter]` table is honoured instead, so PHPantom formats with the same preset and settings your project already uses with the Mago CLI:

```toml
# mago.toml
[formatter]
preset = "psr-12"
print-width = 100
use-tabs = false
```

For the full list of `[formatter]` options (presets, brace placement, blank-line handling, casing, and the rest), refer to the upstream Mago documentation: [Formatter configuration reference](https://mago.carthage.software/latest/en/tools/formatter/configuration-reference).

### Indexing Strategy

By default, PHPantom trusts Composer's autoloader to determine which classes exist in your project. This is intentional: it means completions, diagnostics, and go-to-definition reflect what your code will actually see at runtime. Classes that aren't autoloadable don't appear, because using them would be an error.

The `strategy` setting controls this behaviour:

| Strategy | Behaviour |
| --- | --- |
| `"composer"` (default) | Use Composer's classmap when available, self-scan to fill gaps. Results match what `composer dump-autoload` knows about. |
| `"self"` | Ignore Composer's classmap entirely and scan every PHP file in the workspace. Discovers all classes regardless of autoloading. |
| `"none"` | Use only Composer's classmap with no fallback scanning. The most conservative option. |

Most projects should leave this at the default. Change it to `"self"` if your project loads classes outside of Composer (custom autoloaders, `require_once`, legacy inclusion patterns). Be aware that `"self"` will also surface vendor-internal classes and potential duplicates that Composer's autoloader would never load.

### Classes from other files are not found

PHPantom resolves cross-file classes through Composer's autoloading rules (PSR-4 mappings and the generated classmap). If a class exists in your project but PHPantom reports it as unknown, the most common causes are:

1. **The class isn't Composer-autoloadable.** If your project loads classes via `require_once`, `include`, or a custom autoloader alongside Composer, those classes won't be discovered by default. Set `strategy = "self"` in `.phpantom.toml` to scan all files.

2. **Composer's classmap is stale.** Run `composer dump-autoload` to regenerate it. PHPantom reads the classmap at startup.

3. **The class is in a directory not covered by `autoload` or `autoload-dev`.** Check that your `composer.json` PSR-4 mappings cover the directory where the class lives.
