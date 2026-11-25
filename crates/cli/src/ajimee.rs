//! AJIMEE-Bench JWTD_v2/v1 with the upstream exact-match and MinCER metrics.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sweet_engine::Converter;

#[derive(Deserialize)]
struct Item {
    index: String,
    context_text: String,
    input: String,
    expected_output: Vec<String>,
}

fn reading(input: &str) -> String {
    input
        .chars()
        .map(|ch| match ch {
            'ァ'..='ヶ' | 'ヽ'..='ヾ' => char::from_u32(u32::from(ch) - 0x60).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

fn cer(reference: &str, output: &str) -> f64 {
    let output: Vec<_> = output.chars().collect();
    let mut row: Vec<_> = (0..=output.len()).collect();
    let mut len = 0;
    for expected in reference.chars() {
        let mut diagonal = row[0];
        len += 1;
        row[0] = len;
        for (index, &actual) in output.iter().enumerate() {
            let above = row[index + 1];
            row[index + 1] =
                (diagonal + usize::from(expected != actual)).min(1 + row[index].min(above));
            diagonal = above;
        }
    }
    if len == 0 {
        f64::INFINITY
    } else {
        row[output.len()] as f64 / len as f64
    }
}

#[derive(Default)]
struct Scores {
    correct: usize,
    sum_min_cer: f64,
    elapsed: Vec<Duration>,
}

impl Scores {
    fn add(&mut self, correct: bool, min_cer: f64, elapsed: Duration) {
        self.correct += usize::from(correct);
        self.sum_min_cer += min_cer;
        self.elapsed.push(elapsed);
    }

    fn print(&mut self, label: &str) {
        let n = self.elapsed.len();
        if n == 0 {
            return;
        }
        self.elapsed.sort_unstable();
        let mean_ms =
            self.elapsed.iter().map(Duration::as_secs_f64).sum::<f64>() * 1000.0 / n as f64;
        let p95_ms = self.elapsed[(95 * n).div_ceil(100) - 1].as_secs_f64() * 1000.0;
        println!(
            "{label}: Acc@1 {}/{} ({:.2}%)  mean MinCER {:.2}%  mean {:.2} ms  p95 {:.2} ms",
            self.correct,
            n,
            100.0 * self.correct as f64 / n as f64,
            100.0 * self.sum_min_cer / n as f64,
            mean_ms,
            p95_ms
        );
    }
}

pub fn eval(conv: &Converter, path: &str) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {path}"))?;
    let items: Vec<Item> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {path}"))?;
    ensure!(!items.is_empty(), "AJIMEE-Bench data has no items");
    let (mut total, mut no_context, mut with_context) =
        (Scores::default(), Scores::default(), Scores::default());
    let verbose = std::env::var_os("SWEET_VERBOSE").is_some();
    for item in items {
        ensure!(
            !item.expected_output.is_empty(),
            "item {} has no references",
            item.index
        );
        let input = reading(&item.input);
        let start = Instant::now();
        let output = conv
            .convert_best(&input, &[], &item.context_text)
            .first()
            .map_or_else(String::new, |candidate| candidate.surface());
        let elapsed = start.elapsed();
        let correct = item.expected_output.contains(&output);
        let min_cer = item
            .expected_output
            .iter()
            .map(|reference| cer(reference, &output))
            .fold(f64::INFINITY, f64::min);
        if verbose && !correct {
            println!("FAIL {}: {:?} -> {:?}", item.index, item.input, output);
        }
        total.add(correct, min_cer, elapsed);
        let subset = if item.context_text.is_empty() {
            &mut no_context
        } else {
            &mut with_context
        };
        subset.add(correct, min_cer, elapsed);
    }
    total.print("all");
    no_context.print("without context");
    with_context.print("with context");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cer, reading};

    #[test]
    fn uses_kana_readings_and_unicode_edit_distance() {
        assert_eq!(
            reading("サーティー、２ニン。DＶＤ"),
            "さーてぃー、２にん。DＶＤ"
        );
        assert_eq!(cer("漢字", "漢"), 0.5);
        assert_eq!(cer("漢字", "漢界字"), 0.5);
        assert_eq!(cer("漢字", "漢語"), 0.5);
        assert_eq!(cer("漢字", "漢字"), 0.0);
        assert!(cer("", "漢字").is_infinite());
    }
}
