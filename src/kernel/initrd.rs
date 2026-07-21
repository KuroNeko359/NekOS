//! 内嵌 newc CPIO initrd。

use crate::println;

const HEADER_SIZE: usize = 110;
const MAX_FILES: usize = 16;

#[derive(Clone, Copy)]
struct Entry {
    name: &'static str,
    data: &'static [u8],
}

static mut FILES: [Option<Entry>; MAX_FILES] = [None; MAX_FILES];
static mut FILE_COUNT: usize = 0;

extern "C" {
    static _binary_rootfs_cpio_start: u8;
    static _binary_rootfs_cpio_end: u8;
}

fn hex(bytes: &[u8]) -> Option<usize> {
    let mut value = 0usize;
    for &byte in bytes {
        value = value.checked_mul(16)?;
        value = value.checked_add(match byte {
            b'0'..=b'9' => (byte - b'0') as usize,
            b'a'..=b'f' => (byte - b'a' + 10) as usize,
            b'A'..=b'F' => (byte - b'A' + 10) as usize,
            _ => return None,
        })?;
    }
    Some(value)
}

const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

pub fn init() {
    let start = unsafe { &_binary_rootfs_cpio_start as *const u8 as usize };
    let end = unsafe { &_binary_rootfs_cpio_end as *const u8 as usize };
    let archive = unsafe { core::slice::from_raw_parts(start as *const u8, end - start) };
    let mut pos = 0usize;
    let mut count = 0usize;

    while pos.checked_add(HEADER_SIZE).is_some_and(|end| end <= archive.len()) {
        let header = &archive[pos..pos + HEADER_SIZE];
        if &header[..6] != b"070701" {
            println!("initrd: invalid header at {}", pos);
            break;
        }
        let namesize = match hex(&header[94..102]) { Some(v) => v, None => break };
        let filesize = match hex(&header[54..62]) { Some(v) => v, None => break };
        let name_start = pos + HEADER_SIZE;
        let name_end = match name_start.checked_add(namesize) { Some(v) => v, None => break };
        if namesize == 0 || name_end > archive.len() { break; }
        let name_bytes = &archive[name_start..name_end - 1];
        let name = match core::str::from_utf8(name_bytes) { Ok(v) => v, Err(_) => break };
        if name == "TRAILER!!!" { break; }
        let data_start = align4(name_end);
        let data_end = match data_start.checked_add(filesize) { Some(v) => v, None => break };
        if data_end > archive.len() { break; }
        if count < MAX_FILES && name != "." {
            unsafe { FILES[count] = Some(Entry { name, data: &archive[data_start..data_end] }); }
            count += 1;
        }
        pos = align4(data_end);
    }
    unsafe { FILE_COUNT = count; }
    println!("initrd: loaded {} files", count);
}

pub fn find(name: &str) -> Option<&'static [u8]> {
    unsafe {
        FILES[..FILE_COUNT]
            .iter()
            .flatten()
            .find(|entry| entry.name == name)
            .map(|entry| entry.data)
    }
}
