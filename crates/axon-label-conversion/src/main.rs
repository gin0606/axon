use axon_label_conversion::{Labels, run};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: axon-label-conversion --labels FILE SOURCE OUTPUT

Converts the axon-records/v1 store of the management root SOURCE to axon-records/v2 and writes
it to OUTPUT/.axon, which must not exist, after checking it against SOURCE. SOURCE is not changed.

FILE lists one `ENTITY-ID LABEL` pair per line (`#` starts a comment line). An Entity without a
line gets `chore` when its current value (every head of a conflict) is Completed or Cancelled;
any other Entity without a line stops the conversion and is listed. A store that is corrupt, has missing records (a gap) or
has unmerged paths under .axon in the Git index is not converted.

After the conversion, check OUTPUT with `axon storage check OUTPUT` and then replace
SOURCE/.axon/records and SOURCE/.axon/header.json with those of OUTPUT/.axon, keeping the
originals until `axon storage check SOURCE` passes. The storage guide (docs/guide/storage.md)
gives the steps, also for other branches.";

fn arguments() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let mut labels = None;
    let mut paths = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            Some("--labels") => {
                labels = Some(PathBuf::from(args.next().ok_or("--labels needs a FILE")?));
            }
            Some(other) if other.starts_with('-') => {
                return Err(format!("unknown option {other}"));
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let labels = labels.ok_or("--labels FILE is required")?;
    let [source, output] =
        <[PathBuf; 2]>::try_from(paths).map_err(|_| "expected SOURCE and OUTPUT".to_string())?;
    Ok((labels, source, output))
}

fn main() -> ExitCode {
    let (labels, source, output) = match arguments() {
        Ok(arguments) => arguments,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match Labels::read(&labels).and_then(|labels| run(&source, &labels, &output)) {
        Ok(summary) => {
            println!(
                "Converted {} records and {} Notes of {} Entities into {}",
                summary.records,
                summary.notes,
                summary.entities,
                summary.output.display()
            );
            if !summary.unused.is_empty() {
                println!(
                    "These lines of the labels file name Entities the store does not hold (another branch's, or mistyped):"
                );
                for id in &summary.unused {
                    println!("  {id}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
