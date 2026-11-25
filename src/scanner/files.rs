use std::{
    fs::{self, File, Metadata},
    io::{self, Read as _},
    path::Path,
};

pub(super) fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Avoid special files, links, and allocating on the basis of untrusted file sizes.
pub(super) fn read_small_text(path: &Path, limit: u64) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Not a small regular file",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "File exceeded size limit",
        ));
    }
    String::from_utf8(bytes)
        .map(|mut text| {
            if text.starts_with('\u{feff}') {
                text.drain(..3);
            }
            text
        })
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}
