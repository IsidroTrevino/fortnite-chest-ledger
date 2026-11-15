use super::*;
use std::io::{self, Read, Seek, SeekFrom};

const MAGIC: &[u8; 16] = b"FCLDECODER-V1\0\0\0";
const FOOTER_LEN: u64 = 40;

pub fn start(storage: &Path, replays: &Path) -> io::Result<Child> {
    command(storage)?.args([storage, replays]).spawn()
}

pub fn command(storage: &Path) -> io::Result<Command> {
    let executable = std::env::current_exe()?;
    let decoder = unpack(&executable, &storage.join("runtime"))?
        .unwrap_or_else(|| executable.with_file_name("TreasureDecoder.exe"));
    let mut command = Command::new(decoder);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    Ok(command)
}

fn unpack(executable: &Path, runtime: &Path) -> io::Result<Option<PathBuf>> {
    let mut program = fs::File::open(executable)?;
    let size = program.metadata()?.len();
    let logical_end = certificate_offset(&mut program, size).unwrap_or(size);
    let mut footer_pos = None;
    for padding in 0..=7 {
        let Some(marker) = logical_end.checked_sub(16 + padding) else { break; };
        program.seek(SeekFrom::Start(marker))?;
        let mut magic = [0; 16];
        program.read_exact(&mut magic)?;
        if &magic == MAGIC {
            footer_pos = (marker + 16).checked_sub(FOOTER_LEN);
            break;
        }
        if logical_end == size || magic[15] != 0 { break; }
    }
    let Some(footer_pos) = footer_pos else { return Ok(None); };
    program.seek(SeekFrom::Start(footer_pos))?;
    let mut length = [0; 8];
    let mut id = [0; 16];
    program.read_exact(&mut length)?;
    program.read_exact(&mut id)?;

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

fn certificate_offset(program: &mut fs::File, size: u64) -> Option<u64> {
    fn read_u32(file: &mut fs::File, position: u64) -> Option<u32> {
        file.seek(SeekFrom::Start(position)).ok()?;
        let mut bytes = [0; 4];
        file.read_exact(&mut bytes).ok()?;
        Some(u32::from_le_bytes(bytes))
    }
    let pe = u64::from(read_u32(program, 60)?);
    if read_u32(program, pe)? != 0x4550 { return None; }
    let optional = pe + 24;
    let directories = match read_u32(program, optional)? & 0xffff {
        0x10b => optional + 96,
        0x20b => optional + 112,
        _ => return None,
    };
    let security = directories + 4 * 8;
    let offset = u64::from(read_u32(program, security)?);
    let length = u64::from(read_u32(program, security + 4)?);
    (offset > security && length >= 8 && offset + length <= size).then_some(offset)
}
