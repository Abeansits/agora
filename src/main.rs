mod config;
mod convergence;
mod eval;
mod protocol;
mod report;
mod substrate;
mod synthesis;
mod types;

use crate::types::*;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agora", version, about = "Multi-agent deliberation tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create and start a new deliberation forum
    New {
        /// The topic or question for deliberation
        topic: String,

        /// Participants: preset name (codex, gemini, claude, opencode, human) or name:command:"cmd"
        #[arg(short, long, required = true)]
        participant: Vec<String>,

        /// Round timeout (e.g. "5m", "30s", "1h")
        #[arg(short, long, default_value = "5m")]
        timeout: String,

        /// Maximum number of rounds
        #[arg(long, default_value_t = 5)]
        max_rounds: u32,

        /// Supplementary context (file path or inline text) included in every round prompt
        #[arg(short, long)]
        context: Option<String>,
    },

    /// Check the status of a forum
    Status {
        /// Forum ID
        forum_id: String,
    },

    /// List all forums
    List,

    /// Show the final result of a completed forum
    Result {
        /// Forum ID
        forum_id: String,

        /// Generate an HTML report to final/report.html
        #[arg(long)]
        html: bool,

        /// Publish the HTML report via here.now (requires --html)
        #[arg(long, requires = "html")]
        publish: bool,
    },

    /// Manually submit a response (for human participants)
    Respond {
        /// Forum ID
        forum_id: String,

        /// Round number
        #[arg(short, long)]
        round: u32,

        /// Participant name
        #[arg(short = 'n', long)]
        participant: String,

        /// Path to response file
        #[arg(short, long)]
        file: PathBuf,
    },

    /// Manage participant presets
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },

    /// Evaluate: compare single-model baseline vs. Agora forum (blind judged)
    Eval {
        /// The question or topic to evaluate
        topic: String,

        /// Baseline model preset (single-model response)
        #[arg(short, long)]
        baseline: String,

        /// Forum participants (comma-separated presets)
        #[arg(short, long, value_delimiter = ',')]
        forum: Vec<String>,

        /// Judge model preset (default: auto-select one not in forum)
        #[arg(short, long)]
        judge: Option<String>,

        /// Supplementary context (file path or inline text)
        #[arg(short, long)]
        context: Option<String>,

        /// Timeout per participant
        #[arg(short, long, default_value = "5m")]
        timeout: String,

        /// Max rounds for the forum
        #[arg(long, default_value_t = 3)]
        max_rounds: u32,

        /// Generate HTML report
        #[arg(long)]
        html: bool,

        /// Eval ID to generate HTML for (instead of running a new eval)
        #[arg(long)]
        report: Option<String>,
    },
}

#[derive(Subcommand)]
enum PresetAction {
    /// Add or update a custom preset
    Add {
        /// Preset name (e.g. "llama", "mistral")
        name: String,
        /// Command template (use {prompt_file} or stdin)
        command: String,
    },
    /// List all available presets (built-in + custom)
    List,
    /// Remove a custom preset
    Remove {
        /// Preset name to remove
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New {
            topic,
            participant,
            timeout,
            max_rounds,
            context,
        } => cmd_new(&topic, &participant, &timeout, max_rounds, context.as_deref()),
        Commands::Status { forum_id } => cmd_status(&forum_id),
        Commands::List => cmd_list(),
        Commands::Result {
            forum_id,
            html,
            publish,
        } => cmd_result(&forum_id, html, publish),
        Commands::Respond {
            forum_id,
            round,
            participant,
            file,
        } => cmd_respond(&forum_id, round, &participant, &file),
        Commands::Eval {
            topic,
            baseline,
            forum,
            judge,
            context,
            timeout,
            max_rounds,
            html,
            report,
        } => cmd_eval(
            &topic, &baseline, &forum, judge.as_deref(), context.as_deref(),
            &timeout, max_rounds, html, report.as_deref(),
        ),
        Commands::Preset { action } => match action {
            PresetAction::Add { name, command } => cmd_preset_add(&name, &command),
            PresetAction::List => cmd_preset_list(),
            PresetAction::Remove { name } => cmd_preset_remove(&name),
        },
    }
}

fn cmd_new(
    topic: &str,
    participants: &[String],
    timeout: &str,
    max_rounds: u32,
    context: Option<&str>,
) -> Result<()> {
    // Validate timeout format early
    config::parse_duration(timeout)?;

    // Parse participant specs
    let mut names = Vec::new();
    let mut configs: HashMap<String, ParticipantConfig> = HashMap::new();

    for spec in participants {
        let (name, pc) = config::parse_participant_spec(spec)?;
        names.push(name.clone());
        configs.insert(name, pc);
    }

    // Resolve context: if it's a file path that exists, read it; otherwise treat as inline text
    let context_text = match context {
        Some(c) => {
            let path = std::path::Path::new(c);
            if path.exists() {
                eprintln!("  Reading context from: {}", path.display());
                let content = std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read context file: {}", path.display()))?;
                Some(content)
            } else {
                Some(c.to_string())
            }
        }
        None => None,
    };

    // Generate forum ID: agora-YYYY-MM-DD-UUID8 (collision-safe)
    let id = format!(
        "agora-{}-{}",
        chrono::Utc::now().format("%Y-%m-%d"),
        &uuid::Uuid::new_v4().to_string()[..8],
    );

    let forum_config = ForumConfig {
        forum: ForumSection {
            id: id.clone(),
            topic: topic.to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            max_rounds,
            protocol: "delphi-crossexam".to_string(),
            context: context_text,
        },
        participants: ParticipantsSection { names, configs },
        timing: TimingSection {
            round_timeout: timeout.to_string(),
            participant_timeout: timeout.to_string(),
            quorum: 0,
            late_policy: "include_next".to_string(),
        },
        convergence: ConvergenceSection::default(),
        synthesis: SynthesisSection::default(),
    };

    // Validate before creating anything on disk
    config::validate(&forum_config)?;

    // Create forum directory and save config
    let forum_path = substrate::create_forum_dir(&id)?;
    config::save(&forum_config, &forum_path.join("meta.toml"))?;

    print_banner();
    eprintln!();
    eprintln!("  Forum  {}", id);
    eprintln!("  Topic  {}", topic);
    eprintln!(
        "  With   {}",
        forum_config.participants.names.join(", ")
    );
    eprintln!("  Rules  {} rounds, {} timeout", max_rounds, timeout);
    eprintln!();

    // Run the deliberation (blocking)
    protocol::run_forum(&forum_config, &forum_path)?;

    Ok(())
}

fn cmd_status(forum_id: &str) -> Result<()> {
    let forum_path = substrate::forum_dir(forum_id);
    if !forum_path.exists() {
        anyhow::bail!("Forum not found: {}", forum_id);
    }

    let cfg = config::load(&forum_path.join("meta.toml"))?;
    let current = substrate::current_round(&forum_path);
    let completed = substrate::is_completed(&forum_path);

    println!("Forum:        {}", forum_id);
    println!("Topic:        {}", cfg.forum.topic);
    println!(
        "Status:       {}",
        if completed { "completed" } else { "in progress" }
    );
    println!("Round:        {} / {}", current, cfg.forum.max_rounds);
    println!("Participants: {}", cfg.participants.names.join(", "));

    if current > 0 {
        let responses =
            substrate::read_all_responses(&forum_path, current, &cfg.participants.names)?;
        let responded: Vec<&String> = responses.keys().collect();
        let missing: Vec<&String> = cfg
            .participants
            .names
            .iter()
            .filter(|n| !responses.contains_key(*n))
            .collect();

        println!("\nRound {} responses:", current);
        if !responded.is_empty() {
            println!("  Received: {}", responded.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if !missing.is_empty() {
            println!("  Missing:  {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
    }

    if completed {
        println!(
            "\nFinal output: {}/final/",
            forum_path.display()
        );
    }

    Ok(())
}

fn cmd_list() -> Result<()> {
    let forums = substrate::list_forums()?;

    if forums.is_empty() {
        println!("No forums found.");
        return Ok(());
    }

    println!("{:<32} {:<10} {}", "ID", "Status", "Topic");
    println!("{}", "-".repeat(72));

    for (id, path) in &forums {
        let completed = substrate::is_completed(path);
        let status = if completed { "done" } else { "active" };

        let topic = config::load(&path.join("meta.toml"))
            .map(|c| c.forum.topic)
            .unwrap_or_else(|_| "<error>".into());

        let topic_display = if topic.len() > 35 {
            format!("{}...", &topic[..32])
        } else {
            topic
        };

        println!("{:<32} {:<10} {}", id, status, topic_display);
    }

    Ok(())
}

fn cmd_result(forum_id: &str, html: bool, publish: bool) -> Result<()> {
    let forum_path = substrate::forum_dir(forum_id);
    let final_dir = forum_path.join("final");

    if !final_dir.exists() {
        anyhow::bail!(
            "Forum '{}' has not completed yet. Run: agora status {}",
            forum_id,
            forum_id
        );
    }

    if html {
        let cfg = config::load(&forum_path.join("meta.toml"))?;
        let report_path = final_dir.join("report.html");
        let html_content = report::generate_html_report(&cfg, &forum_path)?;
        std::fs::write(&report_path, &html_content)
            .with_context(|| "Failed to write report.html")?;
        eprintln!("Report written to: {}", report_path.display());

        if publish {
            eprintln!("Publishing via here.now...");
            let output = std::process::Command::new("herenow")
                .arg("publish")
                .arg(&report_path)
                .output()
                .with_context(|| "Failed to run 'herenow publish'. Is here.now installed?")?;
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout);
                println!("{}", url.trim());
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("herenow publish failed: {}", stderr);
            }
        }
        return Ok(());
    }

    // Default: print to terminal
    let synthesis_path = final_dir.join("synthesis.md");
    if synthesis_path.exists() {
        println!("{}", substrate::read_file(&synthesis_path)?);
    }

    let dissent_path = final_dir.join("dissent.md");
    if dissent_path.exists() {
        let content = substrate::read_file(&dissent_path)?;
        if !content.contains("No unresolved disagreements") {
            println!("\n---\n\n{}", content);
        }
    }

    let meta_path = final_dir.join("meta-summary.toml");
    if meta_path.exists() {
        eprintln!("\n--- Meta ---");
        eprintln!("{}", substrate::read_file(&meta_path)?);
    }

    Ok(())
}

fn cmd_respond(forum_id: &str, round: u32, participant: &str, file: &PathBuf) -> Result<()> {
    let forum_path = substrate::forum_dir(forum_id);
    if !forum_path.exists() {
        anyhow::bail!("Forum not found: {}", forum_id);
    }

    let round_dir = forum_path.join(format!("round-{}", round));
    if !round_dir.exists() {
        anyhow::bail!("Round {} does not exist for forum {}", round, forum_id);
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read response file: {}", file.display()))?;

    let response_path = round_dir.join(format!("{}.md", participant));
    substrate::write_atomic(&response_path, &content)?;

    eprintln!(
        "Response submitted: {} -> round-{}/{}.md",
        participant, round, participant
    );

    Ok(())
}

fn print_banner() {
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let (dim, reset, accent) = if is_tty {
        ("\x1b[2m", "\x1b[0m", "\x1b[38;5;75m")
    } else {
        ("", "", "")
    };
    eprint!("{}", accent);
    eprintln!(r"");
    eprintln!(r"   █████╗  ██████╗  ██████╗ ██████╗  █████╗ ");
    eprintln!(r"  ██╔══██╗██╔════╝ ██╔═══██╗██╔══██╗██╔══██╗");
    eprintln!(r"  ███████║██║  ███╗██║   ██║██████╔╝███████║");
    eprintln!(r"  ██╔══██║██║   ██║██║   ██║██╔══██╗██╔══██║");
    eprintln!(r"  ██║  ██║╚██████╔╝╚██████╔╝██║  ██║██║  ██║");
    eprintln!(r"  ╚═╝  ╚═╝ ╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝");
    eprint!("{}", reset);
    eprintln!(
        "  {}v{}  Structured deliberation between AI models{}",
        dim,
        env!("CARGO_PKG_VERSION"),
        reset,
    );
}

#[allow(clippy::too_many_arguments)]
fn cmd_eval(
    topic: &str,
    baseline: &str,
    forum: &[String],
    judge: Option<&str>,
    context: Option<&str>,
    timeout: &str,
    max_rounds: u32,
    html: bool,
    report: Option<&str>,
) -> Result<()> {
    // HTML report for existing eval
    if let Some(eval_id) = report {
        let eval_dir = eval::evals_dir().join(eval_id);
        if !eval_dir.exists() {
            anyhow::bail!("Eval not found: {}", eval_id);
        }
        let html_content = eval::generate_eval_html(&eval_dir)?;
        let report_path = eval_dir.join("report.html");
        std::fs::write(&report_path, &html_content)?;
        eprintln!("Report: {}", report_path.display());
        return Ok(());
    }

    config::parse_duration(timeout)?;

    // Auto-select judge: pick a model not in the forum
    let judge_preset = match judge {
        Some(j) => j.to_string(),
        None => {
            let candidates = ["claude", "gemini", "codex", "opencode"];
            candidates
                .iter()
                .find(|c| !forum.contains(&c.to_string()) && **c != baseline)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "claude".to_string())
        }
    };

    // Resolve context
    let context_text = match context {
        Some(c) => {
            let path = std::path::Path::new(c);
            if path.exists() {
                Some(std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read context file: {}", path.display()))?)
            } else {
                Some(c.to_string())
            }
        }
        None => None,
    };

    print_banner();
    eprintln!();
    eprintln!("  Mode     EVAL (baseline vs. forum)");
    eprintln!("  Topic    {}", topic);
    eprintln!("  Baseline {}", baseline);
    eprintln!("  Forum    {}", forum.join(", "));
    eprintln!("  Judge    {}", judge_preset);
    eprintln!();

    let cfg = eval::EvalConfig {
        topic: topic.to_string(),
        context: context_text,
        baseline_preset: baseline.to_string(),
        forum_presets: forum.to_vec(),
        judge_preset,
        timeout: timeout.to_string(),
        max_rounds,
    };

    let result = eval::run_eval(&cfg)?;

    if html {
        let html_content = eval::generate_eval_html(&result.eval_dir)?;
        let report_path = result.eval_dir.join("report.html");
        std::fs::write(&report_path, &html_content)?;
        eprintln!("HTML report: {}", report_path.display());
    }

    Ok(())
}

fn cmd_preset_add(name: &str, command: &str) -> Result<()> {
    config::save_user_preset(name, command)?;
    eprintln!("Preset '{}' saved: {}", name, command);
    Ok(())
}

fn cmd_preset_list() -> Result<()> {
    let presets = config::list_all_presets();
    println!("{:<14} {:<6} {}", "Name", "Type", "Command");
    println!("{}", "-".repeat(70));
    for (name, cmd, is_custom) in &presets {
        let tag = if *is_custom { "custom" } else { "built-in" };
        let cmd_display = if cmd.len() > 45 {
            format!("{}...", &cmd[..42])
        } else {
            cmd.clone()
        };
        println!("{:<14} {:<9} {}", name, tag, cmd_display);
    }
    println!("{:<14} {:<9} {}", "human", "built-in", "(manual — writes files directly)");
    Ok(())
}

fn cmd_preset_remove(name: &str) -> Result<()> {
    let path = std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
        .join(".agora")
        .join("config.toml");
    if !path.exists() {
        anyhow::bail!("No custom presets configured");
    }
    let content = std::fs::read_to_string(&path)?;
    let mut table: toml::Table = content.parse().unwrap_or_default();
    if let Some(toml::Value::Table(p)) = table.get_mut("presets") {
        if p.remove(name).is_none() {
            anyhow::bail!("Preset '{}' not found in custom presets", name);
        }
    } else {
        anyhow::bail!("No custom presets configured");
    }
    let output = toml::to_string_pretty(&table)?;
    std::fs::write(&path, output)?;
    eprintln!("Preset '{}' removed", name);
    Ok(())
}
