//! Reading a corpus, its manifest and each writer's stream of pieces.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::BufRead as _;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Manifest {
    writers: BTreeMap<String, WriterEntry>,
}

#[derive(Deserialize)]
struct WriterEntry {
    split: String,
}

/// A piece of one writer's writing.
#[derive(Clone, Debug, Deserialize)]
pub struct Piece {
    /// Seconds since the writer's previous piece.
    #[serde(default)]
    pub gap_s: u64,
    #[serde(default)]
    pub category: String,
    /// `document` or `composer`.
    pub surface: String,
    pub segments: Vec<Segment>,
}

/// Text the writer typed, or pasted.
#[derive(Clone, Debug, Deserialize)]
pub struct Segment {
    #[serde(default)]
    pub pasted: bool,
    pub text: String,
}

pub struct Corpus {
    root: PathBuf,
    manifest: Manifest,
}

impl Corpus {
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let manifest = std::fs::read(root.join("manifest.json"))?;
        let manifest = serde_json::from_slice(&manifest).map_err(std::io::Error::other)?;
        Ok(Self {
            root: root.to_path_buf(),
            manifest,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every writer with their split, in order of their ids.
    pub fn writers(&self) -> Vec<(String, String)> {
        self.manifest
            .writers
            .iter()
            .map(|(id, entry)| (id.clone(), entry.split.clone()))
            .collect()
    }

    /// The writer's pieces, oldest first.
    pub fn pieces(&self, writer: &str) -> std::io::Result<Vec<Piece>> {
        let file =
            std::fs::File::open(self.root.join("writers").join(writer).join("stream.jsonl"))?;
        std::io::BufReader::new(file)
            .lines()
            .filter(|line| line.as_ref().map_or(true, |line| !line.trim().is_empty()))
            .map(|line| serde_json::from_str(&line?).map_err(std::io::Error::other))
            .collect()
    }
}
