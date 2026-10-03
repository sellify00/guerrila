use clap::Parser;
use shadow_tier0::money_worker::{
    run_supported_path_latency_probe, SupportedPathLatencyProbeConfig,
    SupportedPathLatencyProbeSource,
};

#[derive(Debug, Parser)]
#[command(name = "dpdk-supported-path-latency-probe")]
struct Args {
    #[arg(long, default_value_t = 100_000)]
    iterations: usize,
    #[arg(long, default_value_t = 5_000)]
    warmup: usize,
    #[arg(long, default_value = "dpdk")]
    source: String,
    #[arg(long, default_value = "supported-nosend")]
    mode: String,
}

fn parse_source(source: &str) -> Result<SupportedPathLatencyProbeSource, String> {
    match source {
        "dpdk" => Ok(SupportedPathLatencyProbeSource::Dpdk),
        "afxdp" => Ok(SupportedPathLatencyProbeSource::Afxdp),
        other => Err(format!(
            "unsupported source '{other}', expected dpdk or afxdp"
        )),
    }
}

fn main() {
    let args = Args::parse();
    if args.mode != "supported-nosend" {
        eprintln!(
            "unsupported mode '{}', only supported-nosend is currently implemented",
            args.mode
        );
        std::process::exit(2);
    }

    let source = match parse_source(&args.source) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    match run_supported_path_latency_probe(SupportedPathLatencyProbeConfig {
        iterations: args.iterations,
        warmup: args.warmup,
        source,
    }) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_else(|err| format!(
                    "{{\"error\":\"failed to encode report: {err}\"}}"
                ))
            );
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
