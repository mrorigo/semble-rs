use std::fs;
use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::commands;
use crate::index::SembleIndex;
use crate::mcp::serve;
use crate::stats::format_savings_report;
use crate::types::SearchMode;
use crate::utils::{
    build_expanded_context, format_results, format_symbol_reports, is_git_url,
    resolve_chunk_detailed, trace,
};

const CLAUDE_FILE_PATH: &str = ".claude/agents/semble-search.md";
const CLI_DISPATCH_ARGS: [&str; 10] = [
    "search",
    "find-related",
    "symbol",
    "init",
    "savings",
    "index",
    "help",
    "-h",
    "--help",
    "--version",
];

#[derive(Parser)]
#[command(
    name = "semble",
    about = "Instant local code search for agents.",
    long_about = "Instant local code search for agents.\n\n\
        Run with a subcommand below for CLI usage. Run without any subcommand \
        (or with --path/--ref) to start the MCP server over stdio."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Parser)]
struct McpArgs {
    path: Option<String>,
    #[arg(long = "ref")]
    ref_name: Option<String>,
    #[arg(long = "include-text-files")]
    include_text_files: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Search the index for code matching a query
    Search {
        /// Query text (natural language or code snippet)
        query: String,
        /// Path or git URL to search (defaults to current directory)
        path: Option<String>,
        /// Number of results to return
        #[arg(short = 'k', long = "top-k", default_value_t = 5)]
        top_k: usize,
        /// Search strategy to use
        #[arg(short = 'm', long = "mode", value_enum, default_value_t = SearchModeArg::Hybrid)]
        mode: SearchModeArg,
        /// Number of source lines of context to include above and below each result
        #[arg(long = "context-lines")]
        context_lines: Option<u32>,
        /// Include non-code text files in the index
        #[arg(long = "include-text-files")]
        include_text_files: bool,
        /// Disable the persistent on-disk index cache
        #[arg(long = "no-cache")]
        no_cache: bool,
    },
    /// Find chunks related to the code at a given file and line
    FindRelated {
        /// Path of the file to start from
        file_path: String,
        /// Line number within the file
        line: usize,
        /// Path or git URL to search (defaults to current directory)
        path: Option<String>,
        /// Number of results to return
        #[arg(short = 'k', long = "top-k", default_value_t = 5)]
        top_k: usize,
        /// Number of source lines of context to include above and below each result
        #[arg(long = "context-lines")]
        context_lines: Option<u32>,
        /// Include non-code text files in the index
        #[arg(long = "include-text-files")]
        include_text_files: bool,
        /// Disable the persistent on-disk index cache
        #[arg(long = "no-cache")]
        no_cache: bool,
    },
    /// Trace a symbol: its definition and referencing chunks
    Symbol {
        /// Symbol name to trace (ignored when FILE_PATH is given)
        name: Option<String>,
        /// File path of an anchor whose declared symbols to list
        file_path: Option<String>,
        /// Line number for --file-path
        #[arg(long)]
        line: Option<usize>,
        /// Path or git URL to search (defaults to current directory)
        #[arg(long)]
        path: Option<String>,
        /// Include non-code text files in the index
        #[arg(long = "include-text-files")]
        include_text_files: bool,
        /// Disable the persistent on-disk index cache
        #[arg(long = "no-cache")]
        no_cache: bool,
    },
    /// Install the semble-search agent file into .claude/agents/
    Init {
        /// Overwrite an existing agent file
        #[arg(long)]
        force: bool,
    },
    /// Print a report of token savings from using semble
    Savings {
        /// Show detailed per-query statistics
        #[arg(long)]
        verbose: bool,
    },
    /// Manage the persistent on-disk index cache
    Index {
        #[command(subcommand)]
        sub: IndexCmd,
    },
}

/// Subcommands under `semble index` for inspecting and managing the cache.
#[derive(Subcommand)]
enum IndexCmd {
    /// Show cache and corpus status for an index
    Status {
        /// Path or git URL to inspect (defaults to the current directory)
        path: Option<String>,
        /// Git ref for git URL sources
        #[arg(long = "ref")]
        ref_name: Option<String>,
        /// Include non-code text files (part of the cache key)
        #[arg(long = "include-text-files")]
        include_text_files: bool,
        /// Build/seed the index and report full corpus statistics
        #[arg(long)]
        build: bool,
    },
    /// Remove the cached index for a single repository, or all of them
    Clear {
        /// Path or git URL whose cache to remove
        path: Option<String>,
        /// Git ref for git URL sources
        #[arg(long = "ref")]
        ref_name: Option<String>,
        /// Include non-code text files (part of the cache key)
        #[arg(long = "include-text-files")]
        include_text_files: bool,
        /// Remove the entire cache root instead of a single repository
        #[arg(long)]
        all: bool,
        /// Do not prompt for confirmation when --all is used
        #[arg(long)]
        force: bool,
    },
    /// Print the cache root location and total on-disk size
    Cache,
}

pub fn main() {
    let arg1 = std::env::args().nth(1);
    trace(format!("argv[1]={:?}", arg1));
    if matches!(arg1.as_deref(), Some(arg) if CLI_DISPATCH_ARGS.contains(&arg)) {
        cli_main();
    } else {
        mcp_main();
    }
}

fn mcp_main() {
    let args = McpArgs::parse();
    trace(format!(
        "starting MCP mode path={:?} ref={:?} include_text_files={}",
        args.path, args.ref_name, args.include_text_files
    ));
    let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");
    if let Err(err) = runtime.block_on(serve(args.path, args.ref_name, args.include_text_files)) {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn cli_main() {
    let args = Args::parse();
    trace("starting CLI mode");
    match args.command {
        Some(Command::Init { force }) => run_init(force),
        Some(Command::Savings { verbose }) => print!("{}", format_savings_report(None, verbose)),
        Some(Command::Index { sub }) => match sub {
            IndexCmd::Status {
                path,
                ref_name,
                include_text_files,
                build,
            } => {
                let path = path.unwrap_or_else(|| ".".to_string());
                if let Err(err) =
                    commands::run_status(&path, ref_name.as_deref(), include_text_files, build)
                {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            }
            IndexCmd::Clear {
                path,
                ref_name,
                include_text_files,
                all,
                force,
            } => {
                let result = if all {
                    commands::run_clear_all(force)
                } else {
                    let path = path.unwrap_or_else(|| ".".to_string());
                    commands::run_clear_one(&path, ref_name.as_deref(), include_text_files)
                };
                if let Err(err) = result {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            }
            IndexCmd::Cache => commands::run_cache_info(),
        },
        Some(Command::Search {
            query,
            path,
            top_k,
            mode,
            context_lines,
            include_text_files,
            no_cache,
        }) => {
            let path = path.unwrap_or_else(|| ".".to_string());
            let index = match open_index(&path, None, include_text_files, !no_cache) {
                Ok(index) => index,
                Err(err) => {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            };
            let mode: SearchMode = mode.into();
            let results = index.search(&query, top_k, mode, None, None, None);
            if results.is_empty() {
                println!("No results found.");
            } else {
                let expanded = context_window(context_lines)
                    .map(|n| build_expanded_context(&index, &results, n));
                print!(
                    "{}",
                    format_results(
                        &format!("Search results for: {:?} (mode={:?})", query, mode),
                        &results,
                        expanded.as_ref()
                    )
                );
            }
        }
        Some(Command::FindRelated {
            file_path,
            line,
            path,
            top_k,
            context_lines,
            include_text_files,
            no_cache,
        }) => {
            let path = path.unwrap_or_else(|| ".".to_string());
            let index = match open_index(&path, None, include_text_files, !no_cache) {
                Ok(index) => index,
                Err(err) => {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            };
            let resolved_path = index.resolve_path(&file_path);
            if let Some(resolution) = resolve_chunk_detailed(&index.chunks, &resolved_path, line) {
                if !resolution.exact {
                    eprintln!(
                        "note: no exact chunk at {}:{}; using nearest chunk: {}",
                        file_path,
                        line,
                        resolution.chunk.location()
                    );
                }
                let results = index.find_related(&resolution.chunk, top_k);
                if results.is_empty() {
                    println!("No related chunks found for {}:{}.", file_path, line);
                } else {
                    let expanded = context_window(context_lines)
                        .map(|n| build_expanded_context(&index, &results, n));
                    print!(
                        "{}",
                        format_results(
                            &format!("Chunks related to {}:{}", file_path, line),
                            &results,
                            expanded.as_ref()
                        )
                    );
                }
            } else {
                eprintln!("No chunk found at {}:{}.", file_path, line);
                process::exit(1);
            }
        }
        Some(Command::Symbol {
            name,
            file_path,
            line,
            path,
            include_text_files,
            no_cache,
        }) => {
            let path = path.unwrap_or_else(|| ".".to_string());
            let index = match open_index(&path, None, include_text_files, !no_cache) {
                Ok(index) => index,
                Err(err) => {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            };
            let reports = if let Some(file_path) = file_path {
                index.symbols_at(&file_path, line.unwrap_or(1))
            } else {
                let Some(name) = name else {
                    eprintln!("Provide either a symbol `name` or `--file-path` + `line`.");
                    process::exit(1);
                };
                index.symbol(&name).into_iter().collect()
            };
            if reports.is_empty() {
                eprintln!("No symbols found.");
                process::exit(1);
            }
            print!("{}", format_symbol_reports(&reports));
        }
        None => {
            let _ = Args::command().print_help();
            println!();
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum SearchModeArg {
    Hybrid,
    Semantic,
    Bm25,
}

impl From<SearchModeArg> for SearchMode {
    fn from(value: SearchModeArg) -> Self {
        match value {
            SearchModeArg::Hybrid => SearchMode::Hybrid,
            SearchModeArg::Semantic => SearchMode::Semantic,
            SearchModeArg::Bm25 => SearchMode::Bm25,
        }
    }
}

/// Normalizes a CLI `--context-lines` value for context expansion.
///
/// Clamps the value to `0..=200` and treats `0` as disabled (None).
fn context_window(context_lines: Option<u32>) -> Option<usize> {
    context_lines
        .map(|n| n.min(200) as usize)
        .filter(|&n| n > 0)
}

fn open_index(
    path: &str,
    ref_name: Option<&str>,
    include_text_files: bool,
    use_cache: bool,
) -> Result<SembleIndex, String> {
    if is_git_url(path) {
        if use_cache {
            SembleIndex::from_git_cached(path, ref_name, None, None, include_text_files)
        } else {
            SembleIndex::from_git(path, ref_name, None, None, include_text_files)
        }
    } else if use_cache {
        SembleIndex::from_path_cached(path, None, None, include_text_files)
    } else {
        SembleIndex::from_path(path, None, None, include_text_files)
    }
}

pub fn run_init(force: bool) {
    let dest = PathBuf::from(CLAUDE_FILE_PATH);
    if dest.exists() && !force {
        eprintln!(
            "{} already exists. Run with --force to overwrite.",
            dest.display()
        );
        process::exit(1);
    }
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&dest, include_str!("../assets/semble-search.md")).expect("write agent file");
    println!("Created {}", dest.display());
}
