use crate::phonemes::Unit;
use crate::text_normaliser::*;
use crate::CmuDictionary;
use csv::{ReaderBuilder, WriterBuilder};
use std::collections::HashSet;
use std::fs::File;
use std::io;
use std::path::Path;
use tracing::{debug, error, info};

pub struct Entry {
    pub id: String,
    pub text: String,
}

pub struct Dataset {
    pub entries: Vec<Entry>,
}

impl Dataset {
    pub fn load(p: impl AsRef<Path>) -> anyhow::Result<Self> {
        let f = File::open(p)?;
        let reader = io::BufReader::new(f);
        let mut rdr = ReaderBuilder::new()
            .has_headers(false)
            .delimiter(b'|')
            .quoting(false) // LJ004-0076 and others don't close quotes on first channel transcript...
            .flexible(true)
            .from_reader(reader);

        let mut entries = vec![];

        for result in rdr.records() {
            let record = result?;
            // So LJ Speech contains normalised transcripts as the 2nd field, we should prefer that
            // instead of normalising ourselves
            match (record.get(0), record.get(2).or_else(|| record.get(1))) {
                (Some(id), Some(text)) => {
                    assert!(!text.contains("|"), "Failed to split: {:?}", record);
                    entries.push(Entry {
                        id: id.to_string(),
                        text: text.to_string(),
                    });
                }
                _ => error!("Incomplete record: {:?}", record),
            }
        }
        Ok(Self { entries })
    }

    pub fn write_csv(&self, writer: impl io::Write) -> anyhow::Result<()> {
        let mut writer = WriterBuilder::new()
            .has_headers(false)
            .delimiter(b'|')
            .flexible(true)
            .from_writer(writer);

        for entry in &self.entries {
            writer.write_record(&[entry.id.as_str(), entry.text.as_str(), entry.text.as_str()])?;
        }
        Ok(())
    }

    pub fn convert_to_pronunciation(&mut self, dict: &CmuDictionary) {
        for entry in self.entries.iter_mut() {
            let mut normalised = normalise_text(&entry.text);
            normalised.words_to_pronunciation(dict);
            let mut new_string = String::new();
            for chunk in normalised.drain_all() {
                match chunk {
                    NormaliserChunk::Pronunciation(units) if !units.is_empty() => {
                        let mut tmp = String::new();
                        let mut in_pronunciation = false;
                        for unit in units.iter() {
                            match unit {
                                Unit::Phone(p) => {
                                    if !in_pronunciation {
                                        tmp.push('{');
                                        in_pronunciation = true;
                                    }
                                    tmp.push_str(p.to_string().as_str());
                                    tmp.push(' ');
                                }
                                Unit::Space => {
                                    if in_pronunciation {
                                        tmp.push('}');
                                    }
                                    in_pronunciation = false;
                                    tmp.push(' ');
                                }
                                Unit::Punct(p) => {
                                    if in_pronunciation {
                                        tmp.push('}');
                                    }
                                    in_pronunciation = false;
                                    tmp.push_str(p.to_string().as_str());
                                    tmp.push(' ');
                                }
                                e => panic!("Unexpected unit: {:?}", e),
                            }
                        }
                        new_string.push_str(tmp.as_str());
                    }
                    NormaliserChunk::Punct(p) => {
                        new_string.push_str(p.to_string().as_str());
                        new_string.push(' ');
                    }
                    NormaliserChunk::Pronunciation(_) => {}
                    e => {
                        panic!("Didn't expect: {:?}", e);
                    }
                }
            }
            debug!("Replacing string!");
            debug!("Old string: {}", entry.text);
            debug!("New string: {}", new_string);
            entry.text = new_string;
        }
    }

    /// Validates there's nothing wrong with the dataset. Will log any errors it finds and return
    /// false
    pub fn validate(&self) -> bool {
        info!("Validating dataset");
        let mut ids = HashSet::new();
        let mut success = true;
        for entry in &self.entries {
            if entry.text.trim().is_empty() {
                error!("Transcript for {} is empty", entry.id);
                success = false;
            }
            let normalised = normalise_text(&entry.text).to_string();
            match normalised {
                Ok(s) if s.trim().is_empty() => {
                    error!(
                        "{} transcript '{}' normalises to an empty string",
                        entry.id, entry.text
                    );
                    success = false;
                }
                Err(e) => {
                    error!(
                        "{} failed to generate string from normaliser output: {}",
                        entry.id, e
                    );
                    success = false;
                }
                Ok(_) => {}
            }
            if ids.contains(entry.id.as_str()) {
                error!("Duplicate ID: {}", entry.id);
                success = false;
            }
            ids.insert(entry.id.as_str());
        }
        info!("Validation complete");
        success
    }
}
