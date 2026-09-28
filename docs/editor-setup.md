# Editor Setup

PHPantom communicates over stdin/stdout using the standard [Language Server Protocol](https://microsoft.github.io/language-server-protocol/). Any editor with LSP support can use it. No special initialization options are required.

## Automatic Installation

These editors download and manage the PHPantom binary for you. To use a newer version than what your editor provides, [install it manually](installation.md) and put `phpantom_lsp` on your `PATH`.

### Zed

PHPantom is supported directly by Zed's official PHP extension, no separate PHPantom extension needed. Install (or update) the PHP extension from Zed's Extensions panel, then add PHPantom to your Zed `settings.json`:

```json
{
  "languages": {
    "PHP": {
      "language_servers": ["phpantom", "!intelephense", "!phpactor", "!phptools", "..."]
    }
  }
}
```

#### File filters

Zed forwards `lsp.phpantom.initialization_options` to the server as-is, so you can tell PHPantom which paths to skip and which extra extensions to treat as PHP from the same `settings.json`:

```json
{
  "lsp": {
    "phpantom": {
      "initialization_options": {
        "indexing": {
          "exclude": ["generated", "storage/framework"],
          "extensions": ["module", "theme"]
        }
      }
    }
  }
}
```

These are the [editor-supplied file filters](configuration.md#editor-supplied-file-filters): `exclude` uses gitignore syntax relative to the workspace root, and they merge with (rather than replace) anything the project's `.phpantom.toml` already sets.

Zed's own `file_scan_exclusions` and `file_types` are not picked up automatically. Its extension API serves extensions only the `language`, `lsp`, and `context_servers` settings categories, so the PHP extension cannot read those two settings to forward them. Until Zed exposes them, list the paths and extensions you care about in the block above, or in `.phpantom.toml` if you would rather your whole team got them.

### VS Code / Cursor

Install the [PHPantom extension](https://marketplace.visualstudio.com/items?itemName=phpantom.phpantom) from the VS Code Marketplace. It automatically downloads the language server binary and starts it when you open a PHP file.

#### File filters

No setup needed. The extension reads your `files.exclude` and `files.associations` settings for each workspace folder and forwards them as [editor-supplied file filters](configuration.md#editor-supplied-file-filters), so a folder you hide is not indexed and an extension you have mapped to `php` is. Changing either setting re-sends them, and the index is reconciled with the new filters without a restart.

Two kinds of entry are skipped, because neither has an equivalent the server can act on. A `files.exclude` entry with a `"when"` clause hides a file only while a sibling exists, which is a per-file question a glob cannot answer, and hiding real source from the index on a guess is worse than indexing it. A `files.associations` pattern that does not reduce to a bare extension (`Jenkinsfile`, or `*.blade.php`, whose files are already indexed as `.php`) has nothing to contribute to a list of extensions. Put either case in `.phpantom.toml` instead.

## Manual Installation

These editors require you to [install PHPantom](installation.md) first.

### Neovim

PHPantom is included in [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig) (just needs to be enabled). If you do not use nvim-lspconfig, then you will need to manually configure it:

```lua
vim.lsp.config('phpantom_lsp', {
  cmd = { 'phpantom_lsp' },
  filetypes = { 'php' },
  root_markers = { '.phpantom.toml', '.git', 'composer.json' },
})
```

Finally, enable it with:

```lua
vim.lsp.enable('phpantom_lsp')
```

### PHPStorm

1. Install [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) from **Editor > Plugins**, then restart PHPStorm.

2. Navigate to **Languages & Frameworks > Language Servers** and click **+** to add a new server:

    - **Name:** `PHPantom`
    - **Command:** path to your `phpantom_lsp` binary
    - **Mapping:** set `PHP` on both the **Language** tab and the **File Type** tab. Setting both ensures PHPStorm activates the server reliably.

![PHPStorm new language server dialog](https://github.com/user-attachments/assets/2da88e68-d012-476e-82e7-977dbfcd9653){ width="600" }

![PHPStorm language server mapping dialog](https://github.com/user-attachments/assets/62358f9e-973c-487d-ac17-098d7dab007e){ width="600" }

### Sublime Text

1. Open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`), type `Package Control: Install Package`, and install **LSP**.

2. Open the Command Palette again, type `Preferences: LSP Server Configurations`, and add:

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

### Helix

Add PHPantom to your `languages.toml` (typically `~/.config/helix/languages.toml`):

```toml
[language-server.phpantom]
command = "phpantom_lsp"

[[language]]
name = "php"
language-servers = ["phpantom"]
```

### Emacs (eglot)

!!! note
    This configuration is untested. If you get it working (or run into issues), please [open an issue](https://github.com/PHPantom-dev/phpantom_lsp/issues).

Eglot is built into Emacs 29+. Add to your `init.el`:

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(php-mode . ("phpantom_lsp"))))
```

Then open a PHP file and run `M-x eglot`.

### Emacs (lsp-mode)

!!! note
    This configuration is untested. If you get it working (or run into issues), please [open an issue](https://github.com/PHPantom-dev/phpantom_lsp/issues).

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

### Kate

!!! note
    This configuration is untested. If you get it working (or run into issues), please [open an issue](https://github.com/PHPantom-dev/phpantom_lsp/issues).

Open **Settings > Configure Kate > LSP Client > User Server Settings** and add:

```json
{
  "servers": {
    "php": {
      "command": ["/path/to/phpantom_lsp"],
      "url": "https://github.com/PHPantom-dev/phpantom_lsp"
    }
  }
}
```

For AI coding agent setup, see [Agent Setup](agent-setup.md).
