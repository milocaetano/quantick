//! Headless reader and directory index for venue deal-counter recordings.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead as _, BufReader, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use quantick_engine::DealSample;

pub const HEADER: &str = "# quantick-deals v1";
pub const FILE_EXTENSION: &str = "deals";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealFile {
    pub symbol: String,
    pub day: String,
    pub samples: Vec<DealSample>,
    pub complete_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedDealDay {
    pub day: String,
    pub first: DealSample,
    pub last: DealSample,
    pub samples: u64,
    pub path: PathBuf,
}

pub type DayCache = BTreeMap<PathBuf, (u64, Option<SystemTime>, RecordedDealDay)>;

#[derive(Debug)]
pub struct DealRecordingWriter {
    pub day: String,
    pub path: PathBuf,
    writer: BufWriter<File>,
    pub last: Option<DealSample>,
    pub written: u64,
    pub first: Option<DealSample>,
    pub held: u64,
    dirty_since_ms: Option<i64>,
}

impl DealRecordingWriter {
    /// Open or create one day, truncating only an incomplete crash tail.
    pub fn open(
        folder: &Path,
        symbol: &str,
        day: &str,
        tz_minutes: i32,
    ) -> io::Result<(Self, Vec<DealSample>)> {
        fs::create_dir_all(folder)?;
        let path = folder.join(format!("{day}.{FILE_EXTENSION}"));
        let len = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        let torn_header = len > 0 && !first_line_terminated(&path)?;
        let (existing, complete_bytes) = if torn_header {
            (Vec::new(), Some(0))
        } else if len > 0 {
            let file = read_file(&path)?;
            (file.samples, Some(file.complete_bytes))
        } else {
            (Vec::new(), None)
        };
        if let Some(complete_bytes) = complete_bytes {
            let file = OpenOptions::new().write(true).open(&path)?;
            if file.metadata()?.len() != complete_bytes {
                file.set_len(complete_bytes)?;
            }
        }
        let fresh = complete_bytes.unwrap_or(0) == 0;
        let mut writer = BufWriter::new(OpenOptions::new().create(true).append(true).open(&path)?);
        if fresh {
            writeln!(
                writer,
                "{HEADER} symbol={symbol} day={day} tz_minutes={tz_minutes}"
            )?;
            writer.flush()?;
        }
        let last = existing.last().copied();
        Ok((
            Self {
                day: day.to_owned(),
                path,
                writer,
                last,
                written: 0,
                first: existing.first().copied(),
                held: existing.len() as u64,
                dirty_since_ms: None,
            },
            existing,
        ))
    }

    pub fn append(&mut self, sample: DealSample, now_ms: i64) -> io::Result<()> {
        match self.last {
            Some(last) if sample == last => return Ok(()),
            Some(last) => writeln!(
                self.writer,
                "+{} {}{}",
                sample.time_ms.saturating_sub(last.time_ms),
                if sample.session_deals >= last.session_deals {
                    "+"
                } else {
                    "-"
                },
                sample.session_deals.abs_diff(last.session_deals)
            )?,
            None => writeln!(self.writer, "{} {}", sample.time_ms, sample.session_deals)?,
        }
        self.last = Some(sample);
        self.first.get_or_insert(sample);
        self.written += 1;
        self.dirty_since_ms.get_or_insert(now_ms);
        Ok(())
    }

    pub fn flush_if_due(&mut self, now_ms: i64, interval_ms: i64) -> io::Result<()> {
        if self
            .dirty_since_ms
            .is_some_and(|since| now_ms - since >= interval_ms)
        {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.dirty_since_ms = None;
        Ok(())
    }
}

fn first_line_terminated(path: &Path) -> io::Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.ends_with('\n'))
}

/// Read one append-only recording, ignoring only an unterminated final line.
pub fn read_file(path: &Path) -> io::Result<DealFile> {
    let mut reader = BufReader::new(File::open(path)?);
    let bad = |line_no: usize, what: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: line {line_no}: {what}", path.display()),
        )
    };
    let mut buf = String::new();
    let mut header_bytes = reader.read_line(&mut buf)?;
    if header_bytes == 0 {
        return Err(bad(1, "empty file"));
    }
    let header = buf.trim_end_matches(['\r', '\n']).to_owned();
    let Some(fields) = header.strip_prefix(HEADER) else {
        return Err(bad(1, "not a quantick-deals v1 file"));
    };
    let mut symbol = None;
    let mut day = None;
    for field in fields.split_whitespace() {
        match field.split_once('=') {
            Some(("symbol", value)) => symbol = Some(value.to_owned()),
            Some(("day", value)) => day = Some(value.to_owned()),
            _ => {}
        }
    }
    let (Some(symbol), Some(day)) = (symbol, day) else {
        return Err(bad(1, "header names no symbol or day"));
    };
    if !buf.ends_with('\n') {
        header_bytes = 0;
    }
    let mut complete_bytes = header_bytes as u64;
    let mut samples = Vec::new();
    let mut line_no = 1;
    let mut torn = None;
    loop {
        buf.clear();
        let read = reader.read_line(&mut buf)?;
        if read == 0 {
            break;
        }
        line_no += 1;
        if let Some(error) = torn.take() {
            return Err(error);
        }
        let line = buf.trim();
        let terminated = buf.ends_with('\n');
        if line.is_empty() || line.starts_with('#') {
            if terminated {
                complete_bytes += read as u64;
            }
            continue;
        }
        let parsed = parse_sample_line(line, samples.last()).map_err(|what| bad(line_no, what));
        match parsed {
            Ok(sample) if terminated => {
                samples.push(sample);
                complete_bytes += read as u64;
            }
            Ok(_) => torn = Some(bad(line_no, "unterminated line")),
            Err(error) if !terminated => torn = Some(error),
            Err(error) => return Err(error),
        }
    }
    Ok(DealFile {
        symbol,
        day,
        samples,
        complete_bytes,
    })
}

fn parse_sample_line(
    line: &str,
    previous: Option<&DealSample>,
) -> Result<DealSample, &'static str> {
    let Some((time, deals)) = line.split_once(' ') else {
        return Err("expected two fields");
    };
    if let Some(delta_t) = time.strip_prefix('+') {
        let Some(last) = previous else {
            return Err("a delta line before any absolute line");
        };
        let dt: i64 = delta_t.parse().map_err(|_| "bad time delta")?;
        let (sign, magnitude) = match deals.split_at_checked(1) {
            Some(("+", rest)) => (1_i64, rest),
            Some(("-", rest)) => (-1_i64, rest),
            _ => return Err("bad deal delta"),
        };
        let dd: i64 = magnitude.parse().map_err(|_| "bad deal delta")?;
        let deals = i64::try_from(last.session_deals)
            .ok()
            .and_then(|value| value.checked_add(sign * dd))
            .and_then(|value| u64::try_from(value).ok())
            .ok_or("deal delta out of range")?;
        Ok(DealSample {
            time_ms: last.time_ms.saturating_add(dt),
            session_deals: deals,
        })
    } else {
        Ok(DealSample {
            time_ms: time.parse().map_err(|_| "bad time")?,
            session_deals: deals.parse().map_err(|_| "bad deal count")?,
        })
    }
}

/// Index valid recordings in one symbol folder, oldest first.
pub fn scan_days(folder: &Path, cache: &mut DayCache) -> Vec<RecordedDealDay> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut days = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(FILE_EXTENSION) {
            continue;
        }
        let stamp = entry
            .metadata()
            .map(|meta| (meta.len(), meta.modified().ok()))
            .unwrap_or((0, None));
        let day = match cache.get(&path) {
            Some((len, modified, day)) if (*len, *modified) == stamp => day.clone(),
            _ => {
                let Ok(file) = read_file(&path) else {
                    continue;
                };
                let (Some(first), Some(last)) = (file.samples.first(), file.samples.last()) else {
                    continue;
                };
                let day = RecordedDealDay {
                    day: file.day,
                    first: *first,
                    last: *last,
                    samples: file.samples.len() as u64,
                    path: path.clone(),
                };
                cache.insert(path, (stamp.0, stamp.1, day.clone()));
                day
            }
        };
        days.insert(day.day.clone(), day);
    }
    days.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_broken_file_is_refused_below_the_application() {
        let scratch = Scratch::new("broken-deals");
        let path = scratch.write("broken.deals", "not a deal recording\n");
        let error = read_file(&path).unwrap_err();
        assert!(error.to_string().contains("line 1"), "{error}");
    }
}
