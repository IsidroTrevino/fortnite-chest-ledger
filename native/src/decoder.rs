use super::*;
use std::io::{self, Read, Seek, SeekFrom};

const MAGIC: &[u8; 16] = b"FCLDECODER-V1\0\0\0";
const FOOTER_LEN: u64 = 40;

pub fn start(storage: &Path, replays: &Path) -> io::Result<Child> {
    let executable = std::env::current_exe()?;
    let decoder = unpack(&executable, &storage.join("runtime"))?
        .unwrap_or_else(|| executable.with_file_name("TreasureDecoder.exe"));
    Command::new(decoder)
        .args([storage, replays])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
        .spawn()
}

fn unpack(executable: &Path, runtime: &Path) -> io::Result<Option<PathBuf>> {
    let mut program = fs::File::open(executable)?;
    let size = program.metadata()?.len();
    let Some(footer_pos) = size.checked_sub(FOOTER_LEN) else { return Ok(None); };
    program.seek(SeekFrom::Start(footer_pos))?;
    let mut length = [0; 8];
    let mut id = [0; 16];
    let mut magic = [0; 16];
    program.read_exact(&mut length)?;
    program.read_exact(&mut id)?;
    program.read_exact(&mut magic)?;
    if &magic != MAGIC { return Ok(None); }

    let length = u64::from_le_bytes(length);
    let valid = length > 0 && length < footer_pos && id.iter().all(u8::is_ascii_hexdigit);
    if !valid { return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid bundled decoder")); }
    let id = std::str::from_utf8(&id).map_err(io::Error::other)?;
    fs::create_dir_all(runtime)?;
    let target = runtime.join(format!("decoder-{id}.exe"));
    if target.metadata().is_ok_and(|info| info.len() == length) { return Ok(Some(target)); }

    program.seek(SeekFrom::Start(footer_pos - length))?;
    let temp = runtime.join(format!("decoder-{id}-{}.tmp", std::process::id()));
    let extracted = (|| {
        let mut destination = fs::File::create(&temp)?;
        if io::copy(&mut program.take(length), &mut destination)? != length {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Incomplete bundled decoder"));
        }
        drop(destination);
        fs::rename(&temp, &target)
    })();
    let _ = fs::remove_file(temp);
    extracted?;
    Ok(Some(target))
}
