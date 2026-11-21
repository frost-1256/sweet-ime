//! Compiles the Mozc OSS dictionary sources into a sweet-ime dictionary.
//!
//! Usage: sweet-dictc <dictionary_oss dir> <output.dict>

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::BufWriter;
use std::path::Path;
use sweet_engine::dict::{SourceEntry, VRCHAT_TERMS, build};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        bail!("usage: sweet-dictc <dictionary_oss dir> <output.dict>");
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
        for (i, line) in VRCHAT_TERMS.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let [reading, surface, cost] = line.split('\t').collect::<Vec<_>>()[..] else {
                bail!("vrchat.tsv:{}: expected reading, surface and cost", i + 1);
            };
            entries.push(SourceEntry {
                reading: reading.into(),
                lid: noun as u16,
                rid: noun as u16,
                cost: cost.parse().with_context(|| format!("vrchat.tsv:{}: invalid cost", i + 1))?,
                surface: surface.into(),
            });
        }
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
