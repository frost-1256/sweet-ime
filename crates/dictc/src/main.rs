//! Compiles the Mozc OSS dictionary sources, and optionally the SudachiDict
//! lexicon sources, into a sweet-ime dictionary.
//!
//! Usage: sweet-dictc <dictionary_oss dir> <output.dict> [<sudachidict dir>]

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::BufWriter;
use std::path::Path;
use sweet_engine::dict::{NET_SLANG, SourceEntry, TECH_TERMS, VRCHAT_TERMS, build};
use sweet_engine::romaji;

mod sudachi;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        bail!("usage: sweet-dictc <dictionary_oss dir> <output.dict> [<sudachidict dir>]");
    }
    let src = Path::new(&args[1]);
    let t0 = std::time::Instant::now();

    let id_def = fs::read_to_string(src.join("id.def")).context("id.def")?;
    let mut features = Vec::new();
    for (i, line) in id_def.lines().enumerate() {
        let (id, feat) = line.split_once(' ').context("bad id.def line")?;
        if id.parse::<usize>()? != i {
            bail!("id.def is not dense at line {i}");
        }
        features.push(feat.to_string());
    }
    let dim = features.len();

    let conn = fs::read_to_string(src.join("connection_single_column.txt"))?;
    let mut it = conn.lines();
    let n: usize = it.next().context("empty matrix")?.trim().parse()?;
    if n != dim {
        bail!("matrix dim {n} != id.def count {dim}");
    }
    let matrix: Vec<i16> = it
        .map(|l| l.trim().parse::<i32>().map(|v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16))
        .collect::<Result<_, _>>()?;
    if matrix.len() != dim * dim {
        bail!("matrix has {} values, expected {}", matrix.len(), dim * dim);
    }

    let mut entries = Vec::new();
    for i in 0..10 {
        let path = src.join(format!("dictionary{i:02}.txt"));
        let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 5 {
                continue;
            }
            entries.push(SourceEntry {
                reading: f[0].to_string(),
                lid: f[1].parse()?,
                rid: f[2].parse()?,
                cost: f[3].parse::<i32>()?.clamp(0, i16::MAX as i32) as i16,
                surface: f[4].to_string(),
            });
        }
    }
    // Mozc's aux dictionary: words that take the POS and cost of a base
    // entry, plus an offset (身に覚え from 見に覚え, -1), so that they win.
    if let Ok(aux) = fs::read_to_string(src.join("aux_dictionary.tsv")) {
        let mut added = Vec::new();
        for (i, line) in aux.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            let [key, value, base_key, base_value, offset] = f[..] else {
                bail!("aux_dictionary.tsv:{}: expected 5 fields", i + 1);
            };
            let offset: i32 = offset
                .parse()
                .with_context(|| format!("aux_dictionary.tsv:{}: invalid cost offset", i + 1))?;
            for e in entries
                .iter()
                .filter(|e| e.reading == base_key && e.surface == base_value)
            {
                added.push(SourceEntry {
                    reading: key.into(),
                    surface: value.into(),
                    cost: (e.cost as i32 + offset).clamp(0, i16::MAX as i32) as i16,
                    ..e.clone()
                });
            }
        }
        entries.extend(added);
    }
    // Keep the compound's connections to preceding and following words.
    let conversion = entries.iter().find(|e| e.reading == "へんかん" && e.surface == "変換");
    let accuracy = entries.iter().find(|e| e.reading == "せいど" && e.surface == "精度");
    if let (Some(conversion), Some(accuracy)) = (conversion, accuracy) {
        entries.push(SourceEntry {
            reading: "へんかんせいど".into(),
            lid: conversion.lid,
            rid: accuracy.rid,
            cost: 700,
            surface: "変換精度".into(),
        });
    }
    if let Some(noun) = features.iter().position(|f| f == "名詞,一般,*,*,*,*,*") {
        for (file, terms) in [
            ("vrchat.tsv", VRCHAT_TERMS),
            ("tech_terms.tsv", TECH_TERMS),
            ("net_slang.tsv", NET_SLANG),
        ] {
            for (i, line) in terms.lines().enumerate() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut fields = line.split('\t');
                let (Some(reading), Some(surface), Some(cost)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    bail!("{file}:{}: expected reading, surface and cost", i + 1);
                };
                let kana = fields.next();
                if fields.next().is_some() {
                    bail!(
                        "{file}:{}: expected at most one kana candidate per row",
                        i + 1
                    );
                }
                let entry = SourceEntry {
                    reading: reading.into(),
                    lid: noun as u16,
                    rid: noun as u16,
                    cost: cost
                        .parse()
                        .with_context(|| format!("{file}:{}: invalid cost", i + 1))?,
                    surface: surface.into(),
                };
                if let Some(kana) = kana.filter(|s| !s.is_empty()) {
                    let katakana = romaji::to_katakana(reading);
                    if katakana != reading && katakana != kana {
                        entries.push(SourceEntry {
                            reading: katakana,
                            ..entry.clone()
                        });
                    }
                    if kana != reading {
                        entries.push(SourceEntry {
                            reading: kana.into(),
                            ..entry.clone()
                        });
                    }
                }
                entries.push(entry);
            }
        }
    }
    if let Some(dir) = args.get(3) {
        let added = sudachi::words(Path::new(dir), &features, &entries)?;
        eprintln!("{} words from SudachiDict", added.len());
        entries.extend(added);
    }
    let count = entries.len();
    let out = fs::File::create(&args[2])?;
    let mut w = BufWriter::new(out);
    build(&mut w, entries, &features, &matrix)?;
    drop(w);
    let size = fs::metadata(&args[2])?.len();
    eprintln!(
        "{count} entries, {dim} POS ids -> {} ({:.1} MiB) in {:.2?}",
        args[2],
        size as f64 / 1048576.0,
        t0.elapsed()
    );
    Ok(())
}
