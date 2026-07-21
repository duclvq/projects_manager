use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Read up to `cap` bytes from the start and `cap` bytes from the end of a file,
/// joined with a newline. Small files are returned whole. Lossy UTF-8.
pub fn read_head_tail(path: &Path, cap: usize) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len() as usize;

    if len <= cap * 2 {
        let mut buf = Vec::with_capacity(len);
        f.read_to_end(&mut buf).ok()?;
        return Some(String::from_utf8_lossy(&buf).into_owned());
    }

    let mut head = vec![0u8; cap];
    f.read_exact(&mut head).ok()?;

    let mut tail = vec![0u8; cap];
    f.seek(SeekFrom::End(-(cap as i64))).ok()?;
    f.read_exact(&mut tail).ok()?;

    let mut out = String::from_utf8_lossy(&head).into_owned();
    out.push('\n');
    out.push_str(&String::from_utf8_lossy(&tail));
    Some(out)
}
