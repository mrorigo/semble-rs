use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{CallType, SearchResult};

pub fn stats_file() -> PathBuf {
    dirs_home().join(".semble").join("savings.jsonl")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketStats {
    pub calls: usize,
    pub snippet_chars: usize,
    pub file_chars: usize,
    pub saved_chars: usize,
}

impl BucketStats {
    pub fn add(&mut self, snippet_chars: usize, file_chars: usize) {
        self.calls += 1;
        self.snippet_chars += snippet_chars;
        self.file_chars += file_chars;
        self.saved_chars += file_chars.saturating_sub(snippet_chars);
    }
}

#[derive(Debug, Clone, Default)]
pub struct SavingsSummary {
    pub buckets: BTreeMap<String, BucketStats>,
    pub call_type_counts: BTreeMap<String, usize>,
}

pub fn save_search_stats(
    results: &[SearchResult],
    call_type: CallType,
    file_sizes: &HashMap<String, usize>,
) {
    let snippet_chars: usize = results.iter().map(|r| r.chunk.content.len()).sum();
    let file_chars: usize = results
        .iter()
        .map(|r| r.chunk.file_path.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| file_sizes.get(&path).copied())
        .sum();

    let ts = chrono::Utc::now().timestamp() as f64;
    let record = serde_json::json!({
        "ts": ts,
        "call": call_type,
        "results": results.len(),
        "snippet_chars": snippet_chars,
        "file_chars": file_chars,
    });

    let path = stats_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", record);
    }
}

pub fn build_savings_summary(path: PathBuf) -> std::io::Result<SavingsSummary> {
    let contents = fs::read_to_string(path)?;
    let mut summary = SavingsSummary::default();
    summary
        .buckets
        .insert("Today".to_string(), BucketStats::default());
    summary
        .buckets
        .insert("Last 7 days".to_string(), BucketStats::default());
    summary
        .buckets
        .insert("All time".to_string(), BucketStats::default());
    let now = chrono::Utc::now();
    let today = now.date_naive();
    let seven_days_ago = (now - chrono::TimeDelta::days(7)).date_naive();
    for line in contents.lines() {
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(line) else {
            continue;
        };
        let snippet_chars = value["snippet_chars"].as_u64().unwrap_or(0) as usize;
        let file_chars = value["file_chars"].as_u64().unwrap_or(0) as usize;
        let call = value["call"].as_str().unwrap_or("unknown").to_string();
        *summary.call_type_counts.entry(call).or_default() += 1;
        summary
            .buckets
            .get_mut("All time")
            .unwrap()
            .add(snippet_chars, file_chars);
        if let Some(ts) = value["ts"].as_f64()
            && let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(ts as i64, 0)
        {
            let date = dt.date_naive();
            if date >= seven_days_ago {
                summary
                    .buckets
                    .get_mut("Last 7 days")
                    .unwrap()
                    .add(snippet_chars, file_chars);
            }
            if date == today {
                summary
                    .buckets
                    .get_mut("Today")
                    .unwrap()
                    .add(snippet_chars, file_chars);
            }
        }
    }
    Ok(summary)
}

pub fn format_savings_report(path: Option<PathBuf>, verbose: bool) -> String {
    let path = path.unwrap_or_else(stats_file);
    if !path.exists() {
        return "No stats yet. Run a search first.".to_string();
    }
    let Ok(summary) = build_savings_summary(path) else {
        return "No stats yet. Run a search first.".to_string();
    };
    let mut out = String::from(
        "\n  Semble Token Savings\n  ════════════════════════════════════════════════════════════════\n  Period        Calls   Savings\n  ────────────────────────────────────────────────────────────────\n",
    );
    for (label, bucket) in summary.buckets {
        let saved_tokens = bucket.saved_chars / 4;
        let saved_str = if saved_tokens >= 1_000_000 {
            format!("~{:.1}M", saved_tokens as f64 / 1_000_000.0)
        } else if saved_tokens >= 1000 {
            format!("~{:.1}k", saved_tokens as f64 / 1000.0)
        } else {
            format!("~{}", saved_tokens)
        };
        let calls_str = if bucket.calls >= 1000 {
            format!("{:.1}k", bucket.calls as f64 / 1000.0)
        } else {
            bucket.calls.to_string()
        };
        let pct = (bucket.saved_chars * 100)
            .checked_div(bucket.file_chars)
            .map(|p| p.to_string())
            .unwrap_or_else(|| "0".to_string());
        out.push_str(&format!(
            "  {:<12}  {:<6}  [{}]  {} tokens ({}%)\n",
            label, calls_str, "████████████████", saved_str, pct
        ));
    }
    if verbose && !summary.call_type_counts.is_empty() {
        out.push_str("\n  Usage Breakdown\n  ────────────────────────────────────────────────────────────────\n  Call type         Calls\n");
        for (call, count) in summary.call_type_counts {
            out.push_str(&format!("  {:<16}  {}\n", call, count));
        }
        out.push_str("  ════════════════════════════════════════════════════════════════\n");
    }
    out.push('\n');
    out
}
