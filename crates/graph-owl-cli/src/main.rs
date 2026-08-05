//! `graph-owl` — the metadata-as-code CLI (Epic 20).
//!
//! **Conventions, and they are load-bearing rather than cosmetic:**
//! data to stdout and diagnostics to stderr, so `graph-owl plan --format
//! json | jq` works without stripping chatter; `--format json` on every
//! command that prints structure; exit codes that mean something (`0` no
//! changes, `1` error, `2` changes pending) so CI branches without parsing
//! text; and no interactive prompt unless stdin is a TTY, so the same
//! invocation works in a pipeline.
//!
//! **The subcommand list is closed on purpose.** `20-metadata-as-code.md`
//! decision 8 exists because a reference CLI in this space carries 40+
//! subcommands that each arrived one reasonable request at a time. The rule:
//! the CLI is for what a *terminal or a CI job* does better than an HTTP
//! call. Querying, entity CRUD, and server administration are deliberately
//! absent — they are the API's job, or the console's.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use graph_owl_cli::apply::{ParentIds, in_dependency_order, may_proceed};
use graph_owl_cli::client::{Catalog, UpsertRequest};
use graph_owl_cli::drift::detect;
use graph_owl_cli::exit::{CHANGES_PENDING, ERROR, FailOn, NO_CHANGES, code_for, redact};
use graph_owl_cli::export::{render as render_export, to_declarations};
use graph_owl_cli::format::{self, Format};
use graph_owl_cli::http::HttpCatalog;
use graph_owl_cli::plan::compute;
use graph_owl_cli::prune::{DEFAULT_PRUNE_THRESHOLD, Scope, authorize};
use graph_owl_cli::validate::validate_directory;

#[derive(Parser)]
#[command(
    name = "graph-owl",
    about = "Declare catalog state in files; plan before applying.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Where the catalog lives. Not needed by `validate`, which is local.
    #[arg(long, global = true, env = "GRAPH_OWL_SERVER")]
    server: Option<String>,

    /// From the environment by default, so a token never lands in shell
    /// history or a CI log line.
    #[arg(long, global = true, env = "GRAPH_OWL_TOKEN", hide_env_values = true)]
    token: Option<String>,

    #[arg(long, global = true, default_value = "text")]
    format: Format,
}

#[derive(Subcommand)]
enum Command {
    /// Check declarations without touching a catalog.
    Validate {
        #[arg(default_value = ".")]
        directory: PathBuf,
    },
    /// Show what applying would do. Never mutates.
    Plan {
        #[arg(default_value = ".")]
        directory: PathBuf,
        /// FQN prefixes this directory is authoritative over.
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
        /// Fail the run when the plan would change things: `nothing`,
        /// `any-change`, or `deletions`.
        #[arg(long, default_value = "nothing")]
        fail_on: FailOnArg,
    },
    /// Apply declarations to the catalog.
    Apply {
        #[arg(default_value = ".")]
        directory: PathBuf,
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
        /// Skip confirmation. Required in a pipeline — without a TTY and
        /// without this, apply refuses rather than assuming consent.
        #[arg(long)]
        yes: bool,
        /// Tombstone entities that are live, in scope, and no longer
        /// declared. Off by default (decision 5).
        #[arg(long)]
        prune: bool,
        /// Refuse the run if more than this many entities would be pruned.
        #[arg(long, default_value_t = DEFAULT_PRUNE_THRESHOLD)]
        prune_threshold: usize,
    },
    /// Report divergence between declarations and the catalog. Never
    /// corrects it — decision 3.
    Drift {
        #[arg(default_value = ".")]
        directory: PathBuf,
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
    },
    /// Emit declarations for what is already in the catalog.
    Export {
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
    },
    /// Stream the whole catalog — lossless, history included — to a
    /// `.tar.zst` archive. Distinct from `export`: that command emits
    /// declarations (Epic 20, deliberately lossy); this one is the backup
    /// and cross-instance move Epic 37b's plan is named for.
    Backup {
        #[arg(long)]
        out: PathBuf,
        /// `domain:x`, `service:x`, or `entity-type:x`. Repeatable;
        /// combining scopes is a union. Omit for the whole catalog.
        #[arg(long = "scope")]
        scopes: Vec<String>,
        /// Field names to redact — only `description` today.
        #[arg(long)]
        redact: Vec<String>,
    },
    /// Restore an archive `backup` produced.
    Restore {
        #[arg(long = "in")]
        input: PathBuf,
        #[arg(long, default_value = "fail")]
        on_conflict: ConflictPolicyArg,
        /// Mint a fresh id for every entity and relationship, rewriting
        /// references consistently — for merging two catalogs that would
        /// otherwise collide on id.
        #[arg(long)]
        regenerate_ids: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ConflictPolicyArg {
    Fail,
    Skip,
    Overwrite,
}

impl std::fmt::Display for ConflictPolicyArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        })
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum FailOnArg {
    Nothing,
    AnyChange,
    Deletions,
}

impl From<FailOnArg> for FailOn {
    fn from(value: FailOnArg) -> Self {
        match value {
            FailOnArg::Nothing => FailOn::Nothing,
            FailOnArg::AnyChange => FailOn::AnyChange,
            FailOnArg::Deletions => FailOn::Deletions,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(message) => {
            // Diagnostics to stderr, always — a failure must not land in the
            // stdout somebody is piping into `jq`.
            eprintln!("{}", redact(&message));
            ExitCode::from(u8::try_from(ERROR).unwrap_or(1))
        }
    }
}

/// Everything fallible, so `main` only decides the exit code.
fn run(cli: &Cli) -> Result<i32, String> {
    match &cli.command {
        Command::Validate { directory } => match validate_directory(directory) {
            Ok(declarations) => {
                eprintln!("{} declarations, no problems", declarations.by_fqn.len());
                Ok(NO_CHANGES)
            }
            Err(errors) => {
                // The errors are **data** when JSON was asked for, so they go
                // to stdout; in text mode they are diagnostics and go to
                // stderr. Same content, different stream, because the two
                // modes are used by different readers.
                let rendered = format::errors(&errors, cli.format).map_err(|e| e.to_string())?;
                match cli.format {
                    Format::Json => println!("{rendered}"),
                    Format::Text => eprintln!("{rendered}"),
                }
                Ok(ERROR)
            }
        },

        Command::Plan {
            directory,
            scopes,
            fail_on,
        } => {
            let (plan, _, _) = load(cli, directory, scopes)?;
            print_plan(cli, &plan)?;
            Ok(code_for(&plan, (*fail_on).into()))
        }

        Command::Apply {
            directory,
            scopes,
            yes,
            prune,
            prune_threshold,
        } => {
            let (plan, declarations, catalog) = load(cli, directory, scopes)?;
            print_plan(cli, &plan)?;

            if !plan.has_changes() {
                return Ok(NO_CHANGES);
            }
            if !may_proceed(*yes, std::io::stdin().is_terminal()) {
                return Err(
                    "refusing to apply: no --yes and no terminal to ask. A pipeline that \
                     meant to pass --yes and did not must fail rather than mutate a catalog \
                     because nobody was watching."
                        .to_string(),
                );
            }

            // Parents before children, resolving each parent's id from the
            // write that created it.
            let live = catalog.live_within(scopes).map_err(|e| e.to_string())?;
            let mut parents = ParentIds::from_live(&live);
            let mut failed = 0;
            for entity in in_dependency_order(&plan) {
                let (_, declaration) = &declarations.by_fqn[&entity.fully_qualified_name];
                let parent_id = declaration
                    .metadata
                    .parent
                    .as_deref()
                    .and_then(|fqn| parents.get(fqn))
                    .map(ToString::to_string);
                match catalog.upsert(&UpsertRequest {
                    kind: declaration.kind.clone(),
                    name: declaration.metadata.name.clone(),
                    parent_id,
                    description: declaration.metadata.description.clone(),
                }) {
                    Ok(id) => parents.learn(&entity.fully_qualified_name, id),
                    // **One failure does not abort the run.** A single
                    // unappliable entity must not cost the other nine
                    // hundred; it is reported and reflected in the exit code.
                    Err(error) => {
                        failed += 1;
                        eprintln!("{}: {error}", entity.fully_qualified_name);
                    }
                }
            }

            if *prune {
                let scope = Scope {
                    prefixes: scopes.clone(),
                };
                let to_prune =
                    authorize(&plan, &scope, *prune_threshold).map_err(|r| r.to_string())?;
                for fqn in to_prune {
                    if let Err(error) = catalog.tombstone(&fqn) {
                        failed += 1;
                        eprintln!("{fqn}: {error}");
                    }
                }
            }

            if failed > 0 {
                return Err(format!("{failed} entities could not be applied"));
            }
            Ok(NO_CHANGES)
        }

        Command::Drift { directory, scopes } => {
            let (plan, _, _) = load(cli, directory, scopes)?;
            // No record of what was last applied yet, so every difference
            // reads as `Unapplied` rather than as an accusation that someone
            // edited live state. Conservative on purpose.
            let report = detect(&plan, &|_| None);
            let rendered = format::drift(&report, cli.format).map_err(|e| e.to_string())?;
            println!("{rendered}");
            Ok(if report.is_clean() {
                NO_CHANGES
            } else {
                CHANGES_PENDING
            })
        }

        Command::Export { scopes } => {
            let catalog = connect(cli)?;
            let live = catalog.live_within(scopes).map_err(|e| e.to_string())?;
            let declarations = to_declarations(&live);
            match cli.format {
                Format::Text => {
                    println!(
                        "{}",
                        render_export(&declarations).map_err(|e| e.to_string())?
                    );
                }
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&declarations).map_err(|e| e.to_string())?
                ),
            }
            Ok(NO_CHANGES)
        }

        Command::Backup {
            out,
            scopes,
            redact,
        } => {
            let server = cli
                .server
                .as_deref()
                .ok_or("no catalog given: pass --server or set GRAPH_OWL_SERVER")?;
            graph_owl_cli::backup::backup(server, cli.token.as_deref(), out, scopes, redact)?;
            eprintln!("wrote {}", out.display());
            Ok(NO_CHANGES)
        }

        Command::Restore {
            input,
            on_conflict,
            regenerate_ids,
        } => {
            let server = cli
                .server
                .as_deref()
                .ok_or("no catalog given: pass --server or set GRAPH_OWL_SERVER")?;
            let outcome = graph_owl_cli::backup::restore(
                server,
                cli.token.as_deref(),
                input,
                &on_conflict.to_string(),
                *regenerate_ids,
            )?;
            match cli.format {
                Format::Json => println!("{outcome}"),
                Format::Text => println!(
                    "{}",
                    serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())?
                ),
            }
            if outcome["aborted"].as_bool() == Some(true) {
                return Ok(CHANGES_PENDING);
            }
            Ok(NO_CHANGES)
        }
    }
}

fn connect(cli: &Cli) -> Result<HttpCatalog, String> {
    let server = cli
        .server
        .as_deref()
        .ok_or("no catalog given: pass --server or set GRAPH_OWL_SERVER")?;
    HttpCatalog::new(server, cli.token.clone()).map_err(|e| e.to_string())
}

/// Validate, read the scope, and diff — the three steps every catalog-facing
/// command starts with.
type Loaded = (
    graph_owl_cli::plan::Plan,
    graph_owl_cli::validate::Declarations,
    HttpCatalog,
);

fn load(cli: &Cli, directory: &std::path::Path, scopes: &[String]) -> Result<Loaded, String> {
    let declarations = validate_directory(directory).map_err(|errors| {
        format!(
            "{} declarations are invalid; run `validate` for the detail:\n{}",
            errors.len(),
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    })?;
    let catalog = connect(cli)?;
    let live = catalog.live_within(scopes).map_err(|e| e.to_string())?;
    let plan = compute(&declarations, &live);
    Ok((plan, declarations, catalog))
}

/// The plan is **data**: stdout in both formats, so `| jq` and `| less` both
/// work and neither has to filter out progress chatter.
fn print_plan(cli: &Cli, plan: &graph_owl_cli::plan::Plan) -> Result<(), String> {
    let rendered = format::plan(plan, cli.format).map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", redact(&rendered)).map_err(|e| e.to_string())?;
    Ok(())
}
