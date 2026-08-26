use std::fs;
use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::index::SembleIndex;
use crate::mcp::serve;
use crate::stats::format_savings_report;
use crate::types::SearchMode;
use crate::utils::{format_results, is_git_url, resolve_chunk, trace};

const CLAUDE_FILE_PATH: &str = ".claude/agents/semble-search.md";
const CLI_DISPATCH_ARGS: [&str; 8] = [
    "search",
    "find-related",
    "init",
    "savings",
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
        /// Include non-code text files in the index
        #[arg(long = "include-text-files")]
        include_text_files: bool,
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
        /// Include non-code text files in the index
        #[arg(long = "include-text-files")]
        include_text_files: bool,
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
        Some(Command::Search {
            query,
            path,
            top_k,
            mode,
            include_text_files,
        }) => {
            let path = path.unwrap_or_else(|| ".".to_string());
            let index = match open_index(&path, None, include_text_files) {
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
                print!(
                    "{}",
                    format_results(
                        &format!("Search results for: {:?} (mode={:?})", query, mode),
                        &results
                    )
                );
            }
        }
        Some(Command::FindRelated {
            file_path,
            line,
            path,
            top_k,
            include_text_files,
        }) => {
            let path = path.unwrap_or_else(|| ".".to_string());
            let index = match open_index(&path, None, include_text_files) {
                Ok(index) => index,
                Err(err) => {
                    eprintln!("{}", err);
                    process::exit(1);
                }
            };
            if let Some(chunk) = resolve_chunk(&index.chunks, &file_path, line) {
                let results = index.find_related(&chunk, top_k);
                if results.is_empty() {
                    println!("No related chunks found for {}:{}.", file_path, line);
                } else {
                    print!(
                        "{}",
                        format_results(
                            &format!("Chunks related to {}:{}", file_path, line),
                            &results
                        )
                    );
                }
            } else {
                eprintln!("No chunk found at {}:{}.", file_path, line);
                process::exit(1);
            }
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

fn open_index(
    path: &str,
    ref_name: Option<&str>,
    include_text_files: bool,
) -> Result<SembleIndex, String> {
    if is_git_url(path) {
        SembleIndex::from_git(path, ref_name, None, None, include_text_files)
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
