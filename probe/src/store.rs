//! Raw data files: pretty JSON for what people read, gzip-compressed JSON for raw RPC responses, JSON lines for
//! records appended during a run.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?).with_context(|| format!("writing {}", path.display()))
}

pub fn write_json_gz(path: &Path, value: &Value) -> Result<()> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&serde_json::to_vec(value)?)?;
    fs::write(path, encoder.finish()?).with_context(|| format!("writing {}", path.display()))
}

pub fn read_json_gz(path: &Path) -> Result<Value> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    serde_json::from_reader(flate2::read::GzDecoder::new(std::io::BufReader::new(file))).with_context(|| format!("parsing {}", path.display()))
}

/// The records of a JSON-lines file (none if it does not exist).
pub fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).with_context(|| format!("parsing {}", path.display())))
        .collect()
}

pub fn append_jsonl<T: Serialize>(path: &Path, record: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}
