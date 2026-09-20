#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
#[cfg(not(target_arch = "wasm32"))]
use phpantom_lsp::Backend;
#[cfg(not(target_arch = "wasm32"))]
use phpantom_lsp::LSP_CONCURRENCY;
#[cfg(not(target_arch = "wasm32"))]
use phpantom_lsp::PARSE_WORKER_STACK_SIZE;
use phpantom_lsp::config;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::TcpListener;
#[cfg(not(target_arch = "wasm32"))]
use tower_lsp::{LspService, Server};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Green.on_default().bold())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser)]
#[command(name = "phpantom_lsp", styles = STYLES)]
#[command(
    version = env!("PHPANTOM_GIT_VERSION"),
    about = "A fast and lightweight PHP Language Server Protocol implementation"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    // this allows LSP wrapper programs to pass a --stdio flag.
    // since this is the only supported communication at this time, this
    // flag can be ignored
    #[arg(long)]
    stdio: bool,

    /// Listen on a TCP address instead of stdin/stdout.
    ///
    /// Accepts a full address (e.g. 127.0.0.1:9257) or just a port number
    /// (e.g. 9257), in which case 127.0.0.1 is used as the host. Use port
    /// 0 to let the OS pick an available port. The server accepts a single
    /// connection and exits when the client disconnects.
    #[arg(long, value_name = "ADDR")]
    tcp: Option<String>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Analyze PHP files and report type-coverage gaps.
    ///
    /// Runs PHPantom's own diagnostics (no PHPStan, no external tools) across
    /// your codebase. The goal is 100% type coverage: every class, member, and
    /// function call should be resolvable. When that holds, completion works
    /// everywhere and PHPStan gets the type information it needs at every level.
    ///
    /// Use this to find and fix the spots where the LSP can't resolve a symbol,
    /// so you can achieve and maintain full completion coverage across the project.
    Analyze {
        /// Paths to analyze (files or directories). Defaults to the entire project.
        #[arg(value_name = "PATH")]
        paths: Vec<std::path::PathBuf>,

        /// Minimum severity level to report.
        #[arg(long, default_value = "all")]
        severity: SeverityArg,

        #[command(flatten)]
        project: ProjectArgs,

        /// Print each file path as it is analyzed and disable the progress
        /// bar. Combine with -v/-vv/-vvv for timing and memory detail.
        #[arg(long)]
        debug: bool,

        /// Increase verbosity (-v, -vv, -vvv). -vv and above imply --debug.
        #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
        verbose: u8,
    },

    /// Apply automated code fixes across PHP files.
    ///
    /// Works like php-cs-fixer: specify which rules (fixers) to run and
    /// PHPantom applies them across the codebase. Rules correspond to
    /// diagnostic codes (e.g. "unused_import"). When no rules are
    /// specified, all preferred native fixers run.
    ///
    /// PHPStan-based rules (prefixed with "phpstan.") require the
    /// --with-phpstan flag.
    Fix {
        /// Path to fix (file or directory). Defaults to the entire project.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,

        /// Rules to apply. Can be specified multiple times. Omit to run all
        /// preferred native fixers.
        #[arg(long = "rule", value_name = "RULE")]
        rules: Vec<String>,

        /// Show what would change without writing files.
        #[arg(long)]
        dry_run: bool,

        /// Enable PHPStan-based fixers (runs PHPStan to collect diagnostics).
        #[arg(long)]
        with_phpstan: bool,

        #[command(flatten)]
        project: ProjectArgs,
    },

    /// Format PHP files and Blade templates across the project.
    ///
    /// Runs the same formatter the editor runs on save: a Laravel Pint,
    /// php-cs-fixer, or PHP_CodeSniffer the project depends on, and the
    /// built-in formatter otherwise. Blade templates are reindented by
    /// the built-in reindenter unless the project formats them with Pint.
    ///
    /// With --check nothing is written: the files that are not formatted
    /// are listed and the command exits non-zero, so a CI job can require
    /// that a pull request ran the formatter.
    Format {
        /// Paths to format (files or directories). Defaults to the entire project.
        #[arg(value_name = "PATH")]
        paths: Vec<std::path::PathBuf>,

        /// List the files that are not formatted and write nothing.
        ///
        /// Exits with code 0 when every file is already formatted, or
        /// code 2 when at least one would change.
        #[arg(long)]
        check: bool,

        /// Spaces per indentation level for Blade templates.
        ///
        /// Only the built-in Blade reindenter reads this; it takes the
        /// value from the editor over LSP and has no other source for it
        /// on the command line. PHP files are formatted to the project's
        /// own rules either way.
        #[arg(long, value_name = "N", default_value_t = 4)]
        indent_size: usize,

        /// Indent Blade templates with tabs instead of spaces.
        #[arg(long, conflicts_with = "indent_size")]
        use_tabs: bool,

        #[command(flatten)]
        project: ProjectArgs,
    },

    /// Move a class, namespace, PHP file, or PSR-4 directory.
    Move {
        /// Source FQN, namespace, PHP file, or directory.
        #[arg(value_name = "FROM")]
        from: String,

        /// Destination FQN, namespace, PHP file, or directory.
        #[arg(value_name = "TO")]
        to: String,

        /// Show what would change without writing files.
        #[arg(long)]
        dry_run: bool,

        #[command(flatten)]
        project: ProjectArgs,
    },

    /// Create a default .phpantom.toml configuration file.
    ///
    /// Writes to the current directory, or with --global to the
    /// platform config directory that every project inherits from.
    Init {
        /// Create the user-wide config instead, in the platform config
        /// directory (~/.config/phpantom_lsp on Linux).  Every project
        /// inherits its settings, and each project's own
        /// .phpantom.toml overrides them key by key.
        #[arg(long)]
        global: bool,
    },

    /// Check for updates or upgrade to the latest version.
    ///
    /// Downloads the latest release from GitHub and replaces the current
    /// binary.  Use --check to see if an update is available without
    /// installing it.
    // There is no binary to replace in a wasm module, and the HTTP client it
    // downloads through does not build for wasm.
    #[cfg(not(target_arch = "wasm32"))]
    Update {
        /// Check for updates but do not install them.
        ///
        /// Exits with code 0 if already up-to-date, or code 1 if an
        /// update is available.
        #[arg(long, short)]
        check: bool,

        /// Skip the confirmation prompt before replacing the binary.
        #[arg(long)]
        no_confirm: bool,
    },
}

/// The options every project-wide subcommand takes.
#[derive(clap::Args)]
struct ProjectArgs {
    /// Disable coloured output.
    #[arg(long)]
    no_colour: bool,

    /// Project root directory. Defaults to the current working directory.
    #[arg(long, value_name = "DIR")]
    project_root: Option<std::path::PathBuf>,

    /// Output format. When running in GitHub Actions the default
    /// automatically includes workflow annotations alongside the
    /// human-readable output.
    #[arg(long, value_name = "FORMAT")]
    format: Option<FormatArg>,
}

/// The settled form of [`ProjectArgs`].
struct ProjectOptions {
    workspace_root: std::path::PathBuf,
    use_colour: bool,
    output_format: phpantom_lsp::analyse::OutputFormat,
}

impl ProjectArgs {
    /// Settle the root (exiting when the current directory is unknown),
    /// the colour choice, and the output format.
    fn resolve(self) -> ProjectOptions {
        let workspace_root = self
            .project_root
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| {
                eprintln!("Error: cannot determine project root directory");
                std::process::exit(1);
            });
        ProjectOptions {
            workspace_root,
            use_colour: !self.no_colour && atty_stdout(),
            output_format: self
                .format
                .map_or(phpantom_lsp::analyse::OutputFormat::Table, Into::into),
        }
    }
}

/// Minimum severity level for the analyze command.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum SeverityArg {
    /// Show all diagnostics (error, warning, info, hint).
    All,
    /// Show only errors and warnings.
    Warning,
    /// Show only errors.
    Error,
}

impl From<SeverityArg> for phpantom_lsp::analyse::SeverityFilter {
    fn from(arg: SeverityArg) -> Self {
        match arg {
            SeverityArg::All => Self::All,
            SeverityArg::Warning => Self::Warning,
            SeverityArg::Error => Self::Error,
        }
    }
}

/// Output format for the analyze and fix commands.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum FormatArg {
    /// Human-readable table (default).
    Table,
    /// GitHub Actions workflow annotations.
    Github,
    /// Machine-readable JSON object.
    Json,
}

impl From<FormatArg> for phpantom_lsp::analyse::OutputFormat {
    fn from(arg: FormatArg) -> Self {
        match arg {
            FormatArg::Table => Self::Table,
            FormatArg::Github => Self::Github,
            FormatArg::Json => Self::Json,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // Tune the allocator before the runtime spawns any threads so freed
    // parse data is returned to the OS after indexing rather than held
    // resident. Must run before the runtime builder below.
    phpantom_lsp::configure_allocator();

    // Build the Tokio runtime by hand (rather than `#[tokio::main]`) so
    // its worker and blocking threads get a larger stack. LSP request
    // handlers parse and walk PHP ASTs on these threads (didOpen runs
    // `update_ast`; completion and hover run the forward walker), and the
    // recursive `mago-syntax` parser and our AST walkers overflow the
    // 2 MB default that Tokio threads otherwise receive. A parse-triggered
    // stack overflow aborts the whole server, so give every runtime thread
    // the same headroom as the analyse workers. See `PARSE_WORKER_STACK_SIZE`.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(PARSE_WORKER_STACK_SIZE)
        .build()
        .expect("failed to build Tokio runtime");
    runtime.block_on(async_main());
}

// wasm has no threads, so there is no allocator to tune, no multi-threaded
// runtime to build, and no worker stacks to size. The CLI subcommands still
// run on a current-thread runtime.
#[cfg(target_arch = "wasm32")]
fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init { global }) => {
            let result = if global {
                config::create_global_config()
            } else {
                let cwd = std::env::current_dir().unwrap_or_else(|e| {
                    eprintln!("Error: cannot determine current directory: {}", e);
                    std::process::exit(1);
                });
                config::create_default_config(&cwd)
                    .map(|created| (created, cwd.join(config::CONFIG_FILE_NAME)))
            };

            match result {
                Ok((true, path)) => {
                    println!("Created {}", path.display());
                }
                Ok((false, path)) => {
                    println!("{} already exists", path.display());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Some(Command::Update { check, no_confirm }) => {
            use phpantom_lsp::self_update::{self, UpdateStatus};
            match self_update::run(check, no_confirm) {
                Ok(UpdateStatus::UpToDate(v)) => {
                    eprintln!("Already up-to-date ({v})");
                }
                Ok(UpdateStatus::UpdateAvailable(_)) => {
                    // Exit with code 1 to signal "update available" so
                    // scripts can branch on `update --check`.
                    std::process::exit(1);
                }
                Ok(UpdateStatus::Updated(_)) => {}
                Err(self_update::UpdateError::Cancelled) => {
                    eprintln!("Update cancelled.");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Analyze {
            paths,
            severity,
            project,
            debug,
            verbose,
        }) => {
            let ProjectOptions {
                workspace_root,
                use_colour,
                output_format,
            } = project.resolve();

            let options = phpantom_lsp::analyse::AnalyseOptions {
                workspace_root,
                path_filters: paths
                    .into_iter()
                    .map(|p| resolve_path_filter(p, 2))
                    .collect(),
                severity_filter: severity.into(),
                use_colour,
                output_format,
                debug: debug || verbose >= 2,
                verbosity: verbose,
                global_config: phpantom_lsp::config::global_config_path(),
            };

            let exit_code = phpantom_lsp::analyse::run(options).await;
            std::process::exit(exit_code);
        }
        Some(Command::Fix {
            path,
            rules,
            dry_run,
            with_phpstan,
            project,
        }) => {
            let ProjectOptions {
                workspace_root,
                use_colour,
                output_format,
            } = project.resolve();

            let options = phpantom_lsp::fix::FixOptions {
                workspace_root,
                path_filter: path.map(|p| resolve_path_filter(p, 1)),
                rules,
                dry_run,
                use_colour,
                with_phpstan,
                output_format,
                global_config: phpantom_lsp::config::global_config_path(),
            };

            let exit_code = phpantom_lsp::fix::run(options).await;
            std::process::exit(exit_code);
        }
        Some(Command::Format {
            paths,
            check,
            indent_size,
            use_tabs,
            project,
        }) => {
            let ProjectOptions {
                workspace_root,
                use_colour,
                output_format,
            } = project.resolve();

            let options = phpantom_lsp::format_cli::FormatOptions {
                workspace_root,
                path_filters: paths
                    .into_iter()
                    .map(|p| resolve_path_filter(p, 1))
                    .collect(),
                check,
                indent: if use_tabs {
                    "\t".to_string()
                } else {
                    " ".repeat(indent_size)
                },
                use_colour,
                output_format,
                global_config: phpantom_lsp::config::global_config_path(),
            };

            let exit_code = phpantom_lsp::format_cli::run(options);
            std::process::exit(exit_code);
        }
        Some(Command::Move {
            from,
            to,
            dry_run,
            project,
        }) => {
            let ProjectOptions {
                workspace_root,
                use_colour,
                output_format,
            } = project.resolve();
            let options = phpantom_lsp::move_cli::MoveOptions {
                from,
                to,
                workspace_root,
                dry_run,
                use_colour,
                output_format,
                global_config: phpantom_lsp::config::global_config_path(),
            };
            let exit_code = phpantom_lsp::move_cli::run(options).await;
            std::process::exit(exit_code);
        }
        // The wasm build drives the server through the exported `lsp_handle`
        // entry point instead (see `wasm_wasi`), so it carries no stdio or TCP
        // transport.
        #[cfg(target_arch = "wasm32")]
        None => {
            eprintln!("Error: the stdio/TCP LSP transport is not available in the wasm build");
            std::process::exit(1);
        }
        #[cfg(not(target_arch = "wasm32"))]
        None => {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_writer(std::io::stderr)
                .init();

            if let Some(addr_str) = cli.tcp {
                // TCP transport: accept a single connection and serve the LSP over it.
                let addr = parse_tcp_address(&addr_str);
                let listener = TcpListener::bind(addr).await.unwrap_or_else(|e| {
                    eprintln!("Error: failed to bind to {}: {}", addr, e);
                    std::process::exit(1);
                });

                let bound_addr = listener.local_addr().unwrap();
                eprintln!("PHPantom LSP listening on tcp://{}", bound_addr);

                let (stream, peer) = listener.accept().await.unwrap_or_else(|e| {
                    eprintln!("Error: failed to accept connection: {}", e);
                    std::process::exit(1);
                });
                eprintln!("Client connected from {}", peer);

                let (read, write) = tokio::io::split(stream);
                let (service, socket) = LspService::build(Backend::new).finish();
                Server::new(read, write, socket)
                    .concurrency_level(LSP_CONCURRENCY)
                    .serve(service)
                    .await;
                // The serve loop exited (client disconnected or an
                // internal error occurred).  Exit the process so the
                // editor can restart us instead of leaving a zombie
                // that consumes no CPU but never responds.
                tracing::warn!("tower-lsp serve loop exited (TCP), shutting down");
                std::process::exit(0);
            } else {
                // Default: run the LSP server over stdin/stdout.
                let stdin = tokio::io::stdin();
                let stdout = tokio::io::stdout();

                let (service, socket) = LspService::build(Backend::new).finish();
                Server::new(stdin, stdout, socket)
                    .concurrency_level(LSP_CONCURRENCY)
                    .serve(service)
                    .await;
                // Same as above: the serve loop exited.  Without this
                // explicit exit, the process hangs because the tokio
                // blocking stdin reader thread keeps the runtime alive
                // even though no tasks are consuming the data.
                tracing::warn!("tower-lsp serve loop exited (stdio), shutting down");
                std::process::exit(0);
            }
        }
    }
}

/// Parse a TCP address string into a `SocketAddr`.
///
/// Accepts either a full address like `127.0.0.1:9257` or just a port number
/// like `9257`. When only a port is given, defaults to `127.0.0.1`.
#[cfg(not(target_arch = "wasm32"))]
fn parse_tcp_address(input: &str) -> SocketAddr {
    if let Ok(addr) = input.parse::<SocketAddr>() {
        return addr;
    }

    if let Ok(port) = input.parse::<u16>() {
        return SocketAddr::from(([127, 0, 0, 1], port));
    }

    eprintln!(
        "Error: invalid TCP address '{}'. Expected HOST:PORT or just PORT.",
        input
    );
    std::process::exit(1);
}

/// Check if stdout is a terminal (for colour auto-detection).
fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Resolve a `PATH` positional argument against the current working
/// directory (not `--project-root`) and exit with `error_exit_code` if it
/// does not exist, matching what shell tab-completion and every other
/// analyzer CLI produce for a typed-out relative path.
fn resolve_path_filter(path: std::path::PathBuf, error_exit_code: i32) -> std::path::PathBuf {
    let abs = if path.is_absolute() {
        path
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(&path),
            Err(e) => {
                eprintln!("Error: cannot determine current directory: {e}");
                std::process::exit(error_exit_code);
            }
        }
    };
    if !abs.exists() {
        eprintln!("Error: path not found: {}", abs.display());
        std::process::exit(error_exit_code);
    }
    abs
}
