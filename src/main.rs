use std::net::SocketAddr;

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::AnsiColor;
use phpantom_lsp::Backend;
use phpantom_lsp::LSP_CONCURRENCY;
use phpantom_lsp::PARSE_WORKER_STACK_SIZE;
use phpantom_lsp::config;
use tokio::net::TcpListener;
use tower_lsp::{LspService, Server};

// Use mimalloc on musl builds (the static Linux binaries we ship),
// where the system allocator is 4-6x slower for our parallel,
// allocation-heavy workload. Everywhere else the system allocator
// performs fine and holds less memory when idle.
#[cfg(all(feature = "mimalloc", target_env = "musl"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
        /// Path to analyze (file or directory). Defaults to the entire project.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,

        /// Minimum severity level to report.
        #[arg(long, default_value = "all")]
        severity: SeverityArg,

        /// Disable coloured output.
        #[arg(long)]
        no_colour: bool,

        /// Project root directory. Defaults to the current working directory.
        #[arg(long, value_name = "DIR")]
        project_root: Option<std::path::PathBuf>,

        /// Output format. When running in GitHub Actions the default
        /// automatically includes workflow annotations alongside the table.
        #[arg(long, value_name = "FORMAT")]
        format: Option<FormatArg>,
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

        /// Disable coloured output.
        #[arg(long)]
        no_colour: bool,

        /// Project root directory. Defaults to the current working directory.
        #[arg(long, value_name = "DIR")]
        project_root: Option<std::path::PathBuf>,

        /// Output format. When running in GitHub Actions the default
        /// automatically includes workflow annotations alongside the table.
        #[arg(long, value_name = "FORMAT")]
        format: Option<FormatArg>,
    },

    /// Create a default .phpantom.toml configuration file in the current directory.
    Init,

    /// Check for updates or upgrade to the latest version.
    ///
    /// Downloads the latest release from GitHub and replaces the current
    /// binary.  Use --check to see if an update is available without
    /// installing it.
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

async fn async_main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => {
            let cwd = std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("Error: cannot determine current directory: {}", e);
                std::process::exit(1);
            });

            match config::create_default_config(&cwd) {
                Ok(true) => {
                    println!("Created {} in {}", config::CONFIG_FILE_NAME, cwd.display());
                }
                Ok(false) => {
                    println!(
                        "{} already exists in {}",
                        config::CONFIG_FILE_NAME,
                        cwd.display()
                    );
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
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
            path,
            severity,
            no_colour,
            project_root,
            format,
        }) => {
            let workspace_root = project_root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| {
                    eprintln!("Error: cannot determine project root directory");
                    std::process::exit(1);
                });
            // Auto-detect colour support: enabled unless --no-colour is
            // passed or stdout is not a terminal.
            let use_colour = !no_colour && atty_stdout();

            let output_format = match format {
                Some(f) => f.into(),
                None => phpantom_lsp::analyse::OutputFormat::Table,
            };

            let options = phpantom_lsp::analyse::AnalyseOptions {
                workspace_root,
                path_filter: path,
                severity_filter: severity.into(),
                use_colour,
                output_format,
            };

            let exit_code = phpantom_lsp::analyse::run(options).await;
            std::process::exit(exit_code);
        }
        Some(Command::Fix {
            path,
            rules,
            dry_run,
            with_phpstan,
            no_colour,
            project_root,
            format,
        }) => {
            let workspace_root = project_root
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| {
                    eprintln!("Error: cannot determine project root directory");
                    std::process::exit(1);
                });
            let use_colour = !no_colour && atty_stdout();

            let output_format = match format {
                Some(f) => f.into(),
                None => phpantom_lsp::analyse::OutputFormat::Table,
            };

            let options = phpantom_lsp::fix::FixOptions {
                workspace_root,
                path_filter: path,
                rules,
                dry_run,
                use_colour,
                with_phpstan,
                output_format,
            };

            let exit_code = phpantom_lsp::fix::run(options).await;
            std::process::exit(exit_code);
        }
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
fn parse_tcp_address(input: &str) -> SocketAddr {
    // Try parsing as a full SocketAddr first.
    if let Ok(addr) = input.parse::<SocketAddr>() {
        return addr;
    }

    // Try parsing as a bare port number.
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
