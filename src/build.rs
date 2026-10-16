//! Self-contained executables (docs/architecture.md, decision 9): this
//! binary, then the A-code's text, its length in 8 bytes little-endian,
//! and a magic. A binary that ends so runs the A-code it carries.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const MAGIC: &[u8; 8] = b"HBDSACOD";

/// The A-code this binary carries, if it was built with some.
pub fn carried() -> io::Result<Option<String>> {
    let mut f = File::open(std::env::current_exe()?)?;
    let size = f.seek(SeekFrom::End(0))?;
    if size < 16 {
        return Ok(None);
    }
    f.seek(SeekFrom::End(-16))?;
    let mut tail = [0u8; 16];
    f.read_exact(&mut tail)?;
    if &tail[8..] != MAGIC {
        return Ok(None);
    }
    let length = u64::from_le_bytes(tail[..8].try_into().expect("8 bytes"));
    let broken = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the A-code this program carries is damaged",
        )
    };
    if length > size - 16 {
        return Err(broken());
    }
    f.seek(SeekFrom::Start(size - 16 - length))?;
    let mut text = vec![0u8; length as usize];
    f.read_exact(&mut text)?;
    String::from_utf8(text).map(Some).map_err(|_| broken())
}

/// Writes `out`, an executable: this binary carrying `acode`.
pub fn write(out: &Path, acode: &str) -> io::Result<()> {
    let mut bytes = fs::read(std::env::current_exe()?)?;
    bytes.extend_from_slice(acode.as_bytes());
    bytes.extend_from_slice(&(acode.len() as u64).to_le_bytes());
    bytes.extend_from_slice(MAGIC);
    fs::write(out, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(out, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}
