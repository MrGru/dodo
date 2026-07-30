//! Thin wrapper: parse, run, report. All of the behaviour is in the library, so
//! it can be unit tested without a process.

use std::process::ExitCode;
use update_manifest::{args, run};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let parsed = match args::parse(argv) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("update-manifest: {message}");
            return ExitCode::FAILURE;
        }
    };

    match run(&parsed) {
        Ok(summary) => {
            println!(
                "update-manifest: hashed {} archive(s) into {}",
                summary.hashed.len(),
                parsed.sums_out.display()
            );
            for name in &summary.hashed {
                println!("  {name}");
            }
            println!(
                "update-manifest: wrote {} for channel {} ({} platform(s))",
                parsed.out.display(),
                parsed.channel,
                summary.manifest_entries.len()
            );
            for (platform, file) in &summary.manifest_entries {
                println!("  {platform} -> {file}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            // A GitHub Actions annotation, so the reason reaches the run summary
            // page rather than living only inside the step log.
            eprintln!("::error::update-manifest failed");
            eprintln!("update-manifest: {message}");
            ExitCode::FAILURE
        }
    }
}
