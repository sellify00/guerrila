use clap::Parser;
use shadow_egress_tpu::{run_probe, write_summary, RunMode, TpuEgressConfig, TpuProbeSummary};
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(
    name = "shadow-egress-tpu",
    about = "Shadow Engine TPU/QUIC egress readiness probe"
)]
struct Cli {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, default_value = "dry_run")]
    mode: String,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    summary_file: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    let mode = match cli.mode.parse::<RunMode>() {
        Ok(mode) => mode,
        Err(err) => {
            eprintln!("FATAL  {err}");
            process::exit(1);
        }
    };

    let config = match TpuEgressConfig::from_file(
        &cli.config,
        mode,
        cli.rpc_url.clone(),
        cli.summary_file.clone(),
    ) {
        Ok(config) => config,
        Err(err) => {
            let summary = TpuProbeSummary::blocked(
                mode,
                "blocked_tpu_quic_config_load_failed",
                err.to_string(),
            );
            emit_summary(cli.summary_file.as_deref(), &summary);
            eprintln!("FATAL  {err}");
            process::exit(1);
        }
    };

    let summary = match run_probe(&config) {
        Ok(summary) => summary,
        Err(err) => {
            TpuProbeSummary::blocked(mode, "blocked_tpu_quic_probe_failed", err.to_string())
        }
    };

    emit_summary(cli.summary_file.as_deref(), &summary);
    if summary.tpu_quic_sender_ready {
        println!("{}", summary.render());
        process::exit(0);
    }

    eprintln!("{}", summary.render());
    process::exit(2);
}

fn emit_summary(path: Option<&std::path::Path>, summary: &TpuProbeSummary) {
    if let Some(path) = path {
        if let Err(err) = write_summary(path, summary) {
            eprintln!("failed to write summary '{}': {err}", path.display());
        }
    }
}
