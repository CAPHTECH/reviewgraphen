use reviewgraphen_core::canonical_json;
use reviewgraphen_ingest::{IngestRequest, ingest};
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: slop_export REPOSITORY BASE TARGET FRESH_OUTPUT".into());
    }
    let repository = std::fs::canonicalize(&args[0])?;
    let request = IngestRequest::new(
        &repository,
        &repository,
        "local:structural-sloppiness-higher-graphen",
        &args[1],
        &args[2],
    );
    let result = ingest(&request)?;
    let bytes = canonical_json(&result.program_space)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    output.write_all(&bytes)?;
    eprintln!(
        "snapshot={} artifacts={} relations={} bytes={}",
        result.program_space.snapshot_id(),
        result.program_space.artifacts().len(),
        result.program_space.relations().len(),
        bytes.len(),
    );
    Ok(())
}
