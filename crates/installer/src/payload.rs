//! The files to install, compressed and appended to the installer exe.
//!
//! Layout of a bundled installer:
//!   <plain sweet-setup.exe> <payload> <payload length: u64 LE> <FOOTER_MAGIC>
//! Payload:
//!   PAYLOAD_MAGIC, count: u32, then per file:
//!   name_len: u16, name (UTF-8, `\`-separated relative path),
//!   size: u64, compressed_len: u64, deflate data.
//!
//! The plain exe (everything before the payload) doubles as the
//! uninstaller, so it is small.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const PAYLOAD_MAGIC: &[u8; 8] = b"SWPAY001";
const FOOTER_MAGIC: &[u8; 8] = b"SWSETUP1";

pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect(root, &p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// Writes `<exe without payload><payload of dir><footer>` to `out`.
pub fn bundle(dir: &Path, out: &Path) -> io::Result<()> {
    let plain = plain_exe()?;
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    let mut payload = Vec::new();
    payload.extend_from_slice(PAYLOAD_MAGIC);
    payload.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for f in &files {
        let rel = f.strip_prefix(dir).map_err(|_| invalid("path"))?;
        let name = rel.to_string_lossy().replace('/', "\\");
        let data = std::fs::read(f)?;
        let comp = miniz_oxide::deflate::compress_to_vec(&data, 6);
        println!("  {name}: {} -> {} bytes", data.len(), comp.len());
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&(data.len() as u64).to_le_bytes());
        payload.extend_from_slice(&(comp.len() as u64).to_le_bytes());
        payload.extend_from_slice(&comp);
    }
    let mut w = io::BufWriter::new(std::fs::File::create(out)?);
    w.write_all(&plain)?;
    w.write_all(&payload)?;
    w.write_all(&(payload.len() as u64).to_le_bytes())?;
    w.write_all(FOOTER_MAGIC)?;
    w.flush()
}

/// (offset, length) of the payload in this exe, if bundled.
fn locate(f: &mut std::fs::File) -> io::Result<Option<(u64, u64)>> {
    let len = f.seek(SeekFrom::End(0))?;
    if len < 16 {
        return Ok(None);
    }
    f.seek(SeekFrom::End(-16))?;
    let mut tail = [0u8; 16];
    f.read_exact(&mut tail)?;
    if &tail[8..] != FOOTER_MAGIC {
        return Ok(None);
    }
    let plen = u64::from_le_bytes(tail[..8].try_into().unwrap());
    let start = len.checked_sub(16 + plen).ok_or_else(|| invalid("payload length"))?;
    Ok(Some((start, plen)))
}

/// This exe's bytes without any payload.
pub fn plain_exe() -> io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(std::env::current_exe()?)?;
    let end = match locate(&mut f)? {
        Some((start, _)) => start,
        None => f.seek(SeekFrom::End(0))?,
    };
    let mut buf = vec![0u8; end as usize];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Whether this exe carries files to install.
pub fn has_payload() -> bool {
    std::env::current_exe()
        .and_then(std::fs::File::open)
        .and_then(|mut f| locate(&mut f))
        .is_ok_and(|p| p.is_some())
}

/// Reads and decompresses the payload.
pub fn read() -> io::Result<Vec<Entry>> {
    let mut f = std::fs::File::open(std::env::current_exe()?)?;
    let (start, len) = locate(&mut f)?.ok_or_else(|| invalid("no payload"))?;
    let mut p = vec![0u8; len as usize];
    f.seek(SeekFrom::Start(start))?;
    f.read_exact(&mut p)?;
    if p.len() < 12 || &p[..8] != PAYLOAD_MAGIC {
        return Err(invalid("bad payload"));
    }
    let count = u32::from_le_bytes(p[8..12].try_into().unwrap()) as usize;
    let mut pos = 12;
    let mut take = |n: usize| -> io::Result<&[u8]> {
        let s = p.get(pos..pos + n).ok_or_else(|| invalid("truncated payload"))?;
        pos += n;
        Ok(s)
    };
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let nlen = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
        let name = String::from_utf8(take(nlen)?.to_vec()).map_err(|_| invalid("name"))?;
        let size = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        let clen = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        let data = miniz_oxide::inflate::decompress_to_vec_with_limit(take(clen)?, size)
            .map_err(|_| invalid("corrupt file data"))?;
        if data.len() != size || name.contains("..") {
            return Err(invalid("corrupt payload entry"));
        }
        out.push(Entry { name, data });
    }
    Ok(out)
}
