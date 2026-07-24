#![no_std]
#![no_main]

use userlib::{entry, ipc, syscall};

// ── IPC 协议 ────────────────────────────────────────────────

const FS_ENDPOINT: usize = 2; // 文件服务器的 IPC endpoint

// 请求类型（放在 words[0]）
const FS_OPEN: usize = 1;
const FS_CLOSE: usize = 2;
const FS_READ: usize = 3;
const FS_WRITE: usize = 4;
const FS_STAT: usize = 5;
const FS_MKDIR: usize = 6;
const FS_READDIR: usize = 7;
const FS_CREATE_OPEN: usize = 8;   // O_CREAT：不存在则创建，存在则打开
const FS_UPDATE_SIZE: usize = 9;   // 写入后更新文件大小和簇号
const FS_DELETE: usize = 10;       // 删除文件
const FS_TRUNCATE: usize = 11;     // 截断文件（O_TRUNC）

// 返回码
const FS_OK: usize = 0;
const FS_ERR: usize = usize::MAX;

// ── 块设备抽象 ──────────────────────────────────────────────

const BLOCK_SIZE: usize = 512;

// RAM Disk：4MB，共 8192 个扇区
const RAMDISK_SIZE: usize = 4 * 1024 * 1024;
const RAMDISK_SECTORS: usize = RAMDISK_SIZE / BLOCK_SIZE;
const IPC_BUFFER_SIZE: usize = 4096;

static mut RAMDISK: [u8; RAMDISK_SIZE] = [0u8; RAMDISK_SIZE];
// 用户栈目前只有一页，长期存在的大缓冲区必须放在静态存储区。
static mut RECV_PATH_BUF: [u8; IPC_BUFFER_SIZE] = [0u8; IPC_BUFFER_SIZE];
static mut READ_DATA_BUF: [u8; IPC_BUFFER_SIZE] = [0u8; IPC_BUFFER_SIZE];

fn disk_read(sector: u64, buf: &mut [u8]) -> bool {
    let offset = sector as usize * BLOCK_SIZE;
    if offset + buf.len() > RAMDISK_SIZE {
        return false;
    }
    buf.copy_from_slice(unsafe { &RAMDISK[offset..offset + buf.len()] });
    true
}

fn disk_write(sector: u64, buf: &[u8]) -> bool {
    let offset = sector as usize * BLOCK_SIZE;
    if offset + buf.len() > RAMDISK_SIZE {
        return false;
    }
    unsafe {
        RAMDISK[offset..offset + buf.len()].copy_from_slice(buf);
    }
    true
}

// ── FAT32 BPB（BIOS Parameter Block）───────────────────────

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Fat32Bpb {
    jmp_boot: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sector_count: u16,
    num_fats: u8,
    root_entry_count: u16,       // FAT32 必须为 0
    total_sectors_16: u16,
    media: u8,
    fat_size_16: u16,            // FAT32 必须为 0
    sectors_per_track: u16,
    num_heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    fat_size_32: u32,
    ext_flags: u16,
    fs_version: u16,
    root_cluster: u32,
    fs_info: u16,
    backup_boot_sector: u16,
    reserved: [u8; 12],
    drive_number: u8,
    reserved1: u8,
    boot_signature: u8,
    volume_serial: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

const FAT32_EOC: u32 = 0x0FFF_FFF8; // End-of-chain marker
const FAT32_FREE: u32 = 0;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_VOLUME_ID: u8 = 0x08;
const LAST_LONG_ENTRY: u8 = 0x40;

// FAT32 文件系统状态
struct Fat32Fs {
    bpb: Fat32Bpb,
    fat_start: u64,              // FAT 表起始扇区
    data_start: u64,             // 数据区起始扇区
    total_clusters: u32,
    root_cluster: u32,
}

// ── 目录条目 ────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DirEntry {
    name: [u8; 8],
    ext: [u8; 3],
    attr: u8,
    nt_res: u8,
    create_time_tenth: u8,
    create_time: u16,
    create_date: u16,
    access_date: u16,
    first_cluster_hi: u16,
    modify_time: u16,
    modify_date: u16,
    first_cluster_lo: u16,
    file_size: u32,
}

// LFN（长文件名）条目
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct LfnEntry {
    order: u8,
    name1: [u16; 5],    // 5 个 UTF-16 字符
    attr: u8,           // 0x0F
    entry_type: u8,     // 0
    checksum: u8,
    name2: [u16; 6],    // 6 个 UTF-16 字符
    zero: u16,          // 0
    name3: [u16; 2],    // 2 个 UTF-16 字符
}

// ── FAT32 操作 ──────────────────────────────────────────────

impl Fat32Fs {
    fn open(bpb_sector: &[u8; BLOCK_SIZE]) -> Option<Self> {
        let bpb = unsafe { core::ptr::read(bpb_sector.as_ptr() as *const Fat32Bpb) };

        // 验证 BPB
        if bpb.bytes_per_sector as usize != BLOCK_SIZE {
            return None;
        }
        if bpb.root_entry_count != 0 || bpb.fat_size_16 != 0 {
            return None; // 不是 FAT32
        }
        if bpb.num_fats == 0 || bpb.fat_size_32 == 0 {
            return None;
        }

        let fat_start = bpb.reserved_sector_count as u64;
        let data_start = fat_start + (bpb.num_fats as u64) * bpb.fat_size_32 as u64;
        let total_clusters = (bpb.total_sectors_32 as u64 - data_start)
            / bpb.sectors_per_cluster as u64;

        Some(Fat32Fs {
            bpb,
            fat_start,
            data_start,
            total_clusters: total_clusters as u32,
            root_cluster: bpb.root_cluster,
        })
    }

    /// 簇号 → 扇区号
    fn cluster_to_sector(&self, cluster: u32) -> u64 {
        self.data_start + (cluster as u64 - 2) * self.bpb.sectors_per_cluster as u64
    }

    /// 读取 FAT 表，返回 cluster 的下一个簇号
    fn fat_next(&self, cluster: u32) -> Option<u32> {
        let fat_offset = cluster as u64 * 4;
        let sector = self.fat_start + fat_offset / BLOCK_SIZE as u64;
        let offset_in_sector = (fat_offset % BLOCK_SIZE as u64) as usize;
        let mut buf = [0u8; BLOCK_SIZE];
        if !disk_read(sector, &mut buf) {
            return None;
        }
        let next = u32::from_le_bytes([
            buf[offset_in_sector],
            buf[offset_in_sector + 1],
            buf[offset_in_sector + 2],
            buf[offset_in_sector + 3],
        ]) & 0x0FFF_FFFF; // 高 4 位是保留的
        Some(next)
    }

    /// 设置 FAT 表中 cluster 的值
    fn fat_set(&self, cluster: u32, value: u32) -> bool {
        let fat_offset = cluster as u64 * 4;
        let sector = self.fat_start + fat_offset / BLOCK_SIZE as u64;
        let offset_in_sector = (fat_offset % BLOCK_SIZE as u64) as usize;
        let mut buf = [0u8; BLOCK_SIZE];
        if !disk_read(sector, &mut buf) {
            return false;
        }
        let bytes = value.to_le_bytes();
        buf[offset_in_sector..offset_in_sector + 4].copy_from_slice(&bytes);
        disk_write(sector, &buf)
    }

    /// 查找空闲簇
    fn find_free_cluster(&self) -> Option<u32> {
        for c in 2..self.total_clusters {
            if let Some(val) = self.fat_next(c) {
                if val == FAT32_FREE {
                    return Some(c);
                }
            }
        }
        None
    }

    /// 分配一个新簇并清零
    fn alloc_cluster(&self) -> Option<u32> {
        let cluster = self.find_free_cluster()?;
        // 标记为 EOC
        if !self.fat_set(cluster, FAT32_EOC) {
            return None;
        }
        // 清零数据区
        let sector = self.cluster_to_sector(cluster);
        let zero = [0u8; BLOCK_SIZE];
        for i in 0..self.bpb.sectors_per_cluster as u64 {
            disk_write(sector + i, &zero);
        }
        Some(cluster)
    }

    /// 遍历目录中的所有条目，对每个有效 DirEntry 调用回调。
    fn walk_dir<F: FnMut(&DirEntry, &[u8])>(&self, dir_cluster: u32, mut f: F) -> bool {
        let mut cluster = dir_cluster;
        let spc = self.bpb.sectors_per_cluster as u64;
        let entries_per_sector = BLOCK_SIZE / 32;
        let mut lfn_buf = [0u16; 256];
        let mut lfn_len = 0usize;

        loop {
            for s in 0..spc {
                let sector = self.cluster_to_sector(cluster) + s;
                let mut buf = [0u8; BLOCK_SIZE];
                if !disk_read(sector, &mut buf) {
                    return false;
                }
                for i in 0..entries_per_sector {
                    let entry_bytes = &buf[i * 32..(i + 1) * 32];
                    let first = entry_bytes[0];
                    if first == 0x00 {
                        return true; // 目录结束
                    }
                    if first == 0xE5 {
                        lfn_len = 0;
                        continue; // 已删除
                    }
                    let attr = entry_bytes[11];
                    if attr == 0x0F {
                        // LFN 条目 — 使用 raw pointer 避免 packed 字段对齐问题
                        let idx = {
                            let lfn = entry_bytes.as_ptr() as *const LfnEntry;
                            let order = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*lfn).order)) };
                            ((order & 0x3F) as usize).saturating_sub(1) as usize
                        };
                        if idx < 20 {
                            let base = idx * 13;
                            if base + 13 <= lfn_buf.len() {
                                unsafe {
                                    let lfn_ptr = entry_bytes.as_ptr() as *const LfnEntry;
                                    let p1 = core::ptr::addr_of!((*lfn_ptr).name1) as *const u16;
                                    let p2 = core::ptr::addr_of!((*lfn_ptr).name2) as *const u16;
                                    let p3 = core::ptr::addr_of!((*lfn_ptr).name3) as *const u16;
                                    core::ptr::copy_nonoverlapping(p1, lfn_buf[base..].as_mut_ptr(), 5);
                                    core::ptr::copy_nonoverlapping(p2, lfn_buf[base + 5..].as_mut_ptr(), 6);
                                    core::ptr::copy_nonoverlapping(p3, lfn_buf[base + 11..].as_mut_ptr(), 2);
                                }
                            }
                            lfn_len = lfn_len.max(base + 13);
                        }
                        continue;
                    }
                    // 短目录条目
                    let entry = unsafe {
                        &*(entry_bytes.as_ptr() as *const DirEntry)
                    };
                    // 构建名字：优先用 LFN，否则用短名
                    let mut name_buf = [0u8; 256];
                    let name_len;
                    if lfn_len > 0 {
                        // 转换 UTF-16 → UTF-8
                        let mut pos = 0;
                        for j in 0..lfn_len {
                            if lfn_buf[j] == 0 || lfn_buf[j] == 0xFFFF {
                                break;
                            }
                            let ch = lfn_buf[j] as u32;
                            if ch < 0x80 && pos < name_buf.len() {
                                name_buf[pos] = ch as u8;
                                pos += 1;
                            }
                        }
                        lfn_len = 0;
                        name_len = pos;
                    } else {
                        lfn_len = 0;
                        // 短名：8.3
                        let mut pos = 0;
                        for j in 0..8 {
                            if entry.name[j] == b' ' { break; }
                            name_buf[pos] = entry.name[j];
                            pos += 1;
                        }
                        if entry.ext[0] != b' ' {
                            name_buf[pos] = b'.';
                            pos += 1;
                            for j in 0..3 {
                                if entry.ext[j] == b' ' { break; }
                                name_buf[pos] = entry.ext[j];
                                pos += 1;
                            }
                        }
                        name_len = pos;
                    };
                    f(entry, &name_buf[..name_len]);
                }
            }
            // 下一个簇
            match self.fat_next(cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cluster = next,
                _ => return true,
            }
        }
    }

    /// 在目录中查找指定名字的条目
    fn find_entry(&self, dir_cluster: u32, name: &str) -> Option<(DirEntry, [u8; 256], usize)> {
        let mut result = None;
        let search = name.as_bytes();
        self.walk_dir(dir_cluster, |entry, entry_name| {
            if result.is_some() { return; }
            if entry_name.len() == search.len()
                && entry_name.iter().zip(search.iter()).all(|(a, b)| {
                    a.eq_ignore_ascii_case(b)
                })
            {
                let mut name_buf = [0u8; 256];
                let n = entry_name.len().min(256);
                name_buf[..n].copy_from_slice(&entry_name[..n]);
                result = Some((*entry, name_buf, n));
            }
        });
        result
    }

    /// 沿路径逐级查找（如 "/dir/file" → root → dir → file）
    fn resolve_path(&self, path: &str) -> Option<(DirEntry, [u8; 256], usize)> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            // 根目录
            let mut name = [0u8; 256];
            name[0] = b'/';
            let entry = DirEntry {
                name: [b' '; 8],
                ext: [b' '; 3],
                attr: ATTR_DIRECTORY,
                nt_res: 0,
                create_time_tenth: 0,
                create_time: 0,
                create_date: 0,
                access_date: 0,
                first_cluster_hi: (self.root_cluster >> 16) as u16,
                modify_time: 0,
                modify_date: 0,
                first_cluster_lo: (self.root_cluster & 0xFFFF) as u16,
                file_size: 0,
            };
            return Some((entry, name, 1));
        }

        let mut cluster = self.root_cluster;
        let mut last_entry = None;
        let mut last_name = [0u8; 256];
        let mut last_len = 0;

        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }
            match self.find_entry(cluster, component) {
                Some((entry, name, len)) => {
                    let next_cluster =
                        ((entry.first_cluster_hi as u32) << 16) | entry.first_cluster_lo as u32;
                    last_entry = Some(entry);
                    last_name = name;
                    last_len = len;
                    if entry.attr & ATTR_DIRECTORY != 0 {
                        cluster = next_cluster;
                    } else if path[path.find(component).unwrap() + component.len()..]
                        .contains('/')
                    {
                        // 文件路径中间出现文件（非目录），无法继续
                        return None;
                    }
                }
                None => return None,
            }
        }

        last_entry.map(|e| (e, last_name, last_len))
    }

    /// 读取文件数据到 buf，返回实际读取的字节数
    fn read_file(&self, cluster: u32, offset: u32, file_size: u32, buf: &mut [u8]) -> usize {
        if offset >= file_size || cluster < 2 {
            return 0;
        }
        let spc = self.bpb.sectors_per_cluster as u32;
        let cluster_size = spc * BLOCK_SIZE as u32;
        let to_read = buf.len().min((file_size - offset) as usize);
        let mut bytes_read = 0usize;
        let mut cur_cluster = cluster;
        let mut cur_offset = 0u32; // 已跳过的字节数

        // 跳到 offset 所在的簇
        while cur_offset + cluster_size <= offset {
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => {
                    cur_cluster = next;
                    cur_offset += cluster_size;
                }
                _ => return 0,
            }
        }

        // 开始读取
        let skip_in_cluster = (offset - cur_offset) as usize;
        while bytes_read < to_read {
            let sector = self.cluster_to_sector(cur_cluster);
            let mut sec_buf = [0u8; BLOCK_SIZE];

            // 遍历当前簇的每个扇区
            for s in 0..spc as u64 {
                if !disk_read(sector + s, &mut sec_buf) {
                    return bytes_read;
                }
                let start = if cur_offset + (s as u32) * BLOCK_SIZE as u32 <= offset
                    && bytes_read == 0
                {
                    skip_in_cluster.min(BLOCK_SIZE)
                } else {
                    0
                };
                let available = BLOCK_SIZE - start;
                let remaining = to_read - bytes_read;
                let n = available.min(remaining);
                if n == 0 {
                    continue;
                }
                buf[bytes_read..bytes_read + n].copy_from_slice(&sec_buf[start..start + n]);
                bytes_read += n;
                if bytes_read >= to_read {
                    return bytes_read;
                }
            }

            cur_offset += cluster_size;
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cur_cluster = next,
                _ => return bytes_read,
            }
        }
        bytes_read
    }

    /// 写入文件数据
    fn write_file(&self, cluster: u32, offset: u32, _file_size: u32, buf: &[u8]) -> (usize, u32) {
        if cluster < 2 {
            return (0, cluster);
        }
        let spc = self.bpb.sectors_per_cluster as u32;
        let cluster_size = spc * BLOCK_SIZE as u32;
        let mut bytes_written = 0usize;
        let mut cur_cluster = cluster;
        #[allow(unused_assignments)]
        let mut prev_cluster = 0u32;
        let mut cur_offset = 0u32;

        // 跳到 offset 所在的簇
        while cur_offset + cluster_size <= offset {
            prev_cluster = cur_cluster;
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => {
                    cur_cluster = next;
                    cur_offset += cluster_size;
                }
                _ => {
                    // 需要分配新簇来延伸到 offset
                    let new = match self.alloc_cluster() {
                        Some(c) => c,
                        None => return (bytes_written, cluster),
                    };
                    self.fat_set(prev_cluster, new);
                    cur_cluster = new;
                    cur_offset += cluster_size;
                }
            }
        }

        let skip_in_cluster = (offset - cur_offset) as usize;

        while bytes_written < buf.len() {
            let sector = self.cluster_to_sector(cur_cluster);

            for s in 0..spc as u64 {
                let mut sec_buf = [0u8; BLOCK_SIZE];
                // 如果不是从头写，先读出原数据
                let start = if cur_offset + (s as u32) * BLOCK_SIZE as u32 <= offset
                    && bytes_written == 0
                {
                    let sk = skip_in_cluster.min(BLOCK_SIZE);
                    if sk > 0 {
                        let _ = disk_read(sector + s, &mut sec_buf);
                    }
                    sk
                } else {
                    0
                };

                let available = BLOCK_SIZE - start;
                let remaining = buf.len() - bytes_written;
                let n = available.min(remaining);
                if n == 0 {
                    continue;
                }
                sec_buf[start..start + n]
                    .copy_from_slice(&buf[bytes_written..bytes_written + n]);
                if !disk_write(sector + s, &sec_buf) {
                    return (bytes_written, cur_cluster);
                }
                bytes_written += n;
                if bytes_written >= buf.len() {
                    return (bytes_written, cur_cluster);
                }
            }

            cur_offset += cluster_size;
            prev_cluster = cur_cluster;
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cur_cluster = next,
                _ => {
                    // 需要分配新簇
                    let new = match self.alloc_cluster() {
                        Some(c) => c,
                        None => return (bytes_written, cur_cluster),
                    };
                    self.fat_set(prev_cluster, new);
                    cur_cluster = new;
                }
            }
        }
        (bytes_written, cur_cluster)
    }

    // ── 短名生成 + LFN checksum ────────────────────────────

    /// 计算 FAT32 短名条目的 checksum（用于 LFN 条目校验）
    fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
        let mut sum: u8 = 0;
        for i in 0..11 {
            sum = (sum >> 1).wrapping_add(sum << 7).wrapping_add(short_name[i]);
        }
        sum
    }

    /// 将长文件名转换为 8.3 短名。
    /// 返回 ([u8;8], [u8;3])，如果名字本身已经是合法 8.3 则直接使用。
    /// 否则截断并加 ~N 后缀（N 从 1 开始，冲突时递增）。
    fn make_short_name(&self, long_name: &str, dir_cluster: u32) -> ([u8; 8], [u8; 3]) {
        let bytes = long_name.as_bytes();
        // 分离 name 和 ext
        let (name_part, ext_part) = if let Some(dot_pos) = long_name.rfind('.') {
            (&bytes[..dot_pos], &bytes[dot_pos + 1..])
        } else {
            (bytes, &b""[..])
        };

        // 尝试生成基础短名：大写、截断到 8 字符
        let mut base = [b' '; 8];
        let mut ext = [b' '; 3];

        // 检查是否可以原样使用（纯 ASCII 大写字母/数字，长度合法）
        let is_valid_83 = name_part.len() > 0
            && name_part.len() <= 8
            && ext_part.len() <= 3
            && name_part.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && ext_part.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_');

        if is_valid_83 {
            for (i, c) in name_part.iter().enumerate() {
                base[i] = c.to_ascii_uppercase();
            }
            for (i, c) in ext_part.iter().enumerate() {
                ext[i] = c.to_ascii_uppercase();
            }
        } else {
            // 截断 name 部分，留空间给 ~N
            let mut pos = 0;
            for c in name_part.iter() {
                if pos >= 6 { break; }
                if c.is_ascii_alphanumeric() || *c == b'_' {
                    base[pos] = c.to_ascii_uppercase();
                    pos += 1;
                }
            }
            // ~1 占 2 个字符
            base[6] = b'~';
            base[7] = b'1';
            for (i, c) in ext_part.iter().take(3).enumerate() {
                ext[i] = c.to_ascii_uppercase();
            }
        }

        // 检查冲突并递增 ~N
        let mut short_name = [0u8; 11];
        short_name[..8].copy_from_slice(&base);
        short_name[8..].copy_from_slice(&ext);

        if !is_valid_83 {
            for n in 1u8..=99u8 {
                // 检查当前短名是否已存在
                let mut found_conflict = false;
                self.walk_dir(dir_cluster, |entry, _name| {
                    if found_conflict { return; }
                    let existing = unsafe {
                        core::ptr::read_unaligned(
                            (entry as *const DirEntry) as *const [u8; 11]
                        )
                    };
                    if existing == short_name {
                        found_conflict = true;
                    }
                });
                if !found_conflict {
                    break;
                }
                // 递增 ~N
                if n < 10 {
                    base[7] = b'0' + n;
                } else {
                    base[6] = b'~';
                    base[7] = b'0' + (n / 10);
                    // 不够空间了，简化处理
                    base[6] = b'~';
                    base[7] = b'0' + n;
                }
                short_name[..8].copy_from_slice(&base);
            }
        }

        let mut result_name = [b' '; 8];
        let mut result_ext = [b' '; 3];
        result_name.copy_from_slice(&short_name[..8]);
        result_ext.copy_from_slice(&short_name[8..]);
        (result_name, result_ext)
    }

    // ── 目录条目创建/更新 ─────────────────────────────────

    /// 在目录中找到空闲槽位，写入 LFN 条目 + 短名条目。
    /// 返回写入的短名条目所在扇区和偏移，用于后续更新。
    fn create_dir_entry(
        &self,
        dir_cluster: u32,
        long_name: &str,
        attr: u8,
        first_cluster: u32,
        file_size: u32,
    ) -> Option<(u64, usize)> {
        let (short_name, short_ext) = self.make_short_name(long_name, dir_cluster);
        let mut full_short = [0u8; 11];
        full_short[..8].copy_from_slice(&short_name);
        full_short[8..].copy_from_slice(&short_ext);
        let checksum = Self::lfn_checksum(&full_short);

        // 将长文件名转为 UTF-16，存入固定大小数组
        let mut name_chars = [0u16; 256];
        let mut name_len = 0usize;
        for ch in long_name.encode_utf16() {
            if name_len >= 256 { break; }
            name_chars[name_len] = ch;
            name_len += 1;
        }
        let lfn_count = if name_len <= 13 { 0 } else { (name_len + 12) / 13 };
        let total_entries = lfn_count + 1; // +1 for short name entry

        // 沿目录簇链找连续空闲槽位
        let mut cluster = dir_cluster;
        let spc = self.bpb.sectors_per_cluster as u64;
        let entries_per_sector = BLOCK_SIZE / 32;

        loop {
            for s in 0..spc {
                let sector = self.cluster_to_sector(cluster) + s;
                let mut buf = [0u8; BLOCK_SIZE];
                if !disk_read(sector, &mut buf) {
                    return None;
                }
                // 扫描空闲槽位（0x00 或 0xE5）
                let mut free_start = None;
                let mut free_count = 0usize;
                for i in 0..entries_per_sector {
                    let first = buf[i * 32];
                    if first == 0x00 || first == 0xE5 {
                        if free_start.is_none() {
                            free_start = Some(i);
                        }
                        free_count += 1;
                        if free_count >= total_entries {
                            // 找到足够的连续空闲槽位
                            let start_idx = free_start.unwrap();
                            // 写入 LFN 条目（倒序，从最后一个 LFN 开始）
                            for lfn_idx in (0..lfn_count).rev() {
                                let entry_idx = start_idx + (lfn_count - 1 - lfn_idx);
                                let entry_offset = entry_idx * 32;
                                let order = (lfn_idx + 1) as u8;
                                let last_flag = if lfn_idx == lfn_count - 1 {
                                    order | 0x40
                                } else {
                                    order
                                };
                                // 构建 LFN 条目
                                buf[entry_offset] = last_flag;
                                buf[entry_offset + 11] = 0x0F; // attr = LFN
                                buf[entry_offset + 12] = 0;    // entry_type
                                buf[entry_offset + 13] = checksum;
                                // name1: 5 chars at offset 1..11
                                // name2: 6 chars at offset 14..26
                                // name3: 2 chars at offset 28..32
                                let char_base = lfn_idx * 13;
                                // name1 (offset 1, 5 UTF-16)
                                for ci in 0..5usize {
                                    let char_off = entry_offset + 1 + ci * 2;
                                    let char_idx = char_base + ci;
                                    let ch = if char_idx < name_len {
                                        name_chars[char_idx]
                                    } else if char_idx == name_len {
                                        0x0000
                                    } else {
                                        0xFFFF
                                    };
                                    buf[char_off] = (ch & 0xFF) as u8;
                                    buf[char_off + 1] = (ch >> 8) as u8;
                                }
                                // name2 (offset 14, 6 UTF-16)
                                for ci in 0..6usize {
                                    let char_off = entry_offset + 14 + ci * 2;
                                    let char_idx = char_base + 5 + ci;
                                    let ch = if char_idx < name_len {
                                        name_chars[char_idx]
                                    } else if char_idx == name_len {
                                        0x0000
                                    } else {
                                        0xFFFF
                                    };
                                    buf[char_off] = (ch & 0xFF) as u8;
                                    buf[char_off + 1] = (ch >> 8) as u8;
                                }
                                // name3 (offset 28, 2 UTF-16)
                                for ci in 0..2usize {
                                    let char_off = entry_offset + 28 + ci * 2;
                                    let char_idx = char_base + 11 + ci;
                                    let ch = if char_idx < name_len {
                                        name_chars[char_idx]
                                    } else if char_idx == name_len {
                                        0x0000
                                    } else {
                                        0xFFFF
                                    };
                                    buf[char_off] = (ch & 0xFF) as u8;
                                    buf[char_off + 1] = (ch >> 8) as u8;
                                }
                            }

                            // 写入短名条目
                            let short_idx = start_idx + lfn_count;
                            let short_offset = short_idx * 32;
                            buf[short_offset..short_offset + 8].copy_from_slice(&short_name);
                            buf[short_offset + 8..short_offset + 11].copy_from_slice(&short_ext);
                            buf[short_offset + 11] = attr;
                            buf[short_offset + 20] = (first_cluster >> 16) as u8;
                            buf[short_offset + 21] = (first_cluster >> 24) as u8;
                            buf[short_offset + 26] = (first_cluster & 0xFF) as u8;
                            buf[short_offset + 27] = ((first_cluster >> 8) & 0xFF) as u8;
                            let size_bytes = file_size.to_le_bytes();
                            buf[short_offset + 28..short_offset + 32].copy_from_slice(&size_bytes);

                            if !disk_write(sector, &buf) {
                                return None;
                            }
                            return Some((sector, short_offset));
                        }
                    } else {
                        free_start = None;
                        free_count = 0;
                    }
                }
            }
            // 检查是否需要扩展目录
            match self.fat_next(cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cluster = next,
                _ => {
                    // 目录末尾，分配新簇
                    let new_cluster = self.alloc_cluster()?;
                    self.fat_set(cluster, new_cluster);
                    // 继续下一轮循环，会在新簇中找到空闲槽位
                    cluster = new_cluster;
                }
            }
        }
    }

    /// 更新目录条目的 file_size 和 first_cluster（用于写入后更新大小）。
    fn update_dir_entry_size(
        &self,
        dir_cluster: u32,
        name: &str,
        new_size: u32,
        new_cluster: u32,
    ) -> bool {
        let mut found = false;
        let search = name.as_bytes();
        let mut cur_cluster = dir_cluster;
        let spc = self.bpb.sectors_per_cluster as u64;
        let entries_per_sector = BLOCK_SIZE / 32;
        let mut lfn_buf = [0u16; 256];
        let mut lfn_len = 0usize;

        loop {
            for s in 0..spc {
                let sector = self.cluster_to_sector(cur_cluster) + s;
                let mut buf = [0u8; BLOCK_SIZE];
                if !disk_read(sector, &mut buf) {
                    return false;
                }
                let mut modified = false;
                for i in 0..entries_per_sector {
                    let entry_bytes = &buf[i * 32..(i + 1) * 32];
                    let first = entry_bytes[0];
                    if first == 0x00 { return found; }
                    if first == 0xE5 { lfn_len = 0; continue; }
                    let attr = entry_bytes[11];
                    if attr == 0x0F {
                        // LFN 条目
                        let idx = {
                            let order = entry_bytes[0];
                            ((order & 0x3F) as usize).saturating_sub(1) as usize
                        };
                        if idx < 20 {
                            let base = idx * 13;
                            if base + 13 <= lfn_buf.len() {
                                for ci in 0..5usize {
                                    let off = 1 + ci * 2;
                                    lfn_buf[base + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                                for ci in 0..6usize {
                                    let off = 14 + ci * 2;
                                    lfn_buf[base + 5 + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                                for ci in 0..2usize {
                                    let off = 28 + ci * 2;
                                    lfn_buf[base + 11 + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                            }
                            lfn_len = lfn_len.max(base + 13);
                        }
                        continue;
                    }
                    // 短名条目 — 比较名字
                    let mut name_matches = false;
                    if lfn_len > 0 {
                        let mut pos = 0;
                        let mut name_buf = [0u8; 256];
                        for j in 0..lfn_len {
                            if lfn_buf[j] == 0 || lfn_buf[j] == 0xFFFF { break; }
                            let ch = lfn_buf[j] as u32;
                            if ch < 0x80 && pos < name_buf.len() {
                                name_buf[pos] = ch as u8;
                                pos += 1;
                            }
                        }
                        lfn_len = 0;
                        name_matches = pos == search.len()
                            && name_buf[..pos].iter().zip(search.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b));
                    } else {
                        lfn_len = 0;
                        let mut pos = 0;
                        let mut name_buf = [0u8; 13];
                        for j in 0..8 {
                            if entry_bytes[j] == b' ' { break; }
                            name_buf[pos] = entry_bytes[j];
                            pos += 1;
                        }
                        if entry_bytes[8] != b' ' {
                            name_buf[pos] = b'.';
                            pos += 1;
                            for j in 0..3 {
                                if entry_bytes[8 + j] == b' ' { break; }
                                name_buf[pos] = entry_bytes[8 + j];
                                pos += 1;
                            }
                        }
                        name_matches = pos == search.len()
                            && name_buf[..pos].iter().zip(search.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b));
                    };

                    if name_matches {
                        // 找到了，更新 size 和 cluster
                        let off = i * 32;
                        let size_bytes = new_size.to_le_bytes();
                        buf[off + 28..off + 32].copy_from_slice(&size_bytes);
                        buf[off + 26] = (new_cluster & 0xFF) as u8;
                        buf[off + 27] = ((new_cluster >> 8) & 0xFF) as u8;
                        buf[off + 20] = (new_cluster >> 16) as u8;
                        buf[off + 21] = (new_cluster >> 24) as u8;
                        modified = true;
                        found = true;
                    }
                }
                if modified {
                    if !disk_write(sector, &buf) {
                        return false;
                    }
                }
                if found { return true; }
            }
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cur_cluster = next,
                _ => return found,
            }
        }
    }

    /// 释放 FAT 链（从 cluster 开始直到 EOC），返回释放的簇数。
    fn free_chain(&self, cluster: u32) -> u32 {
        if cluster < 2 { return 0; }
        let mut cur = cluster;
        let mut count = 0u32;
        loop {
            match self.fat_next(cur) {
                Some(next) => {
                    self.fat_set(cur, FAT32_FREE);
                    count += 1;
                    if next >= 2 && next < FAT32_EOC {
                        cur = next;
                    } else {
                        return count;
                    }
                }
                None => return count,
            }
        }
    }

    /// 删除目录中的指定文件/目录条目（标记为 0xE5）。
    fn delete_dir_entry(&self, dir_cluster: u32, name: &str) -> bool {
        let search = name.as_bytes();
        let mut cur_cluster = dir_cluster;
        let spc = self.bpb.sectors_per_cluster as u64;
        let entries_per_sector = BLOCK_SIZE / 32;
        let mut lfn_buf = [0u16; 256];
        let mut lfn_len = 0usize;
        // 记录 LFN 条目的起始位置
        let mut lfn_start_sector = 0u64;
        let mut lfn_start_idx = 0usize;

        loop {
            for s in 0..spc {
                let sector = self.cluster_to_sector(cur_cluster) + s;
                let mut buf = [0u8; BLOCK_SIZE];
                if !disk_read(sector, &mut buf) {
                    return false;
                }
                let mut modified = false;
                for i in 0..entries_per_sector {
                    let entry_bytes = &buf[i * 32..(i + 1) * 32];
                    let first = entry_bytes[0];
                    if first == 0x00 { return false; }
                    if first == 0xE5 {
                        lfn_len = 0;
                        continue;
                    }
                    let attr = entry_bytes[11];
                    if attr == 0x0F {
                        // LFN 条目
                        if lfn_len == 0 {
                            lfn_start_sector = sector;
                            lfn_start_idx = i;
                        }
                        let idx = {
                            let order = entry_bytes[0];
                            ((order & 0x3F) as usize).saturating_sub(1) as usize
                        };
                        if idx < 20 {
                            let base = idx * 13;
                            if base + 13 <= lfn_buf.len() {
                                for ci in 0..5usize {
                                    let off = 1 + ci * 2;
                                    lfn_buf[base + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                                for ci in 0..6usize {
                                    let off = 14 + ci * 2;
                                    lfn_buf[base + 5 + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                                for ci in 0..2usize {
                                    let off = 28 + ci * 2;
                                    lfn_buf[base + 11 + ci] = u16::from_le_bytes([entry_bytes[off], entry_bytes[off + 1]]);
                                }
                            }
                            lfn_len = lfn_len.max(base + 13);
                        }
                        continue;
                    }
                    // 短名条目
                    let mut name_matches = false;
                    if lfn_len > 0 {
                        let mut pos = 0;
                        let mut name_buf = [0u8; 256];
                        for j in 0..lfn_len {
                            if lfn_buf[j] == 0 || lfn_buf[j] == 0xFFFF { break; }
                            let ch = lfn_buf[j] as u32;
                            if ch < 0x80 && pos < name_buf.len() {
                                name_buf[pos] = ch as u8;
                                pos += 1;
                            }
                        }
                        lfn_len = 0;
                        name_matches = pos == search.len()
                            && name_buf[..pos].iter().zip(search.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b));
                    } else {
                        lfn_len = 0;
                        let mut pos = 0;
                        let mut name_buf = [0u8; 13];
                        for j in 0..8 {
                            if entry_bytes[j] == b' ' { break; }
                            name_buf[pos] = entry_bytes[j];
                            pos += 1;
                        }
                        if entry_bytes[8] != b' ' {
                            name_buf[pos] = b'.';
                            pos += 1;
                            for j in 0..3 {
                                if entry_bytes[8 + j] == b' ' { break; }
                                name_buf[pos] = entry_bytes[8 + j];
                                pos += 1;
                            }
                        }
                        name_matches = pos == search.len()
                            && name_buf[..pos].iter().zip(search.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b));
                    };

                    if name_matches {
                        // 找到了 — 标记短名条目为已删除
                        buf[i * 32] = 0xE5;
                        modified = true;
                        // 如果有 LFN，也需要标记已删除
                        // LFN 条目在 lfn_start_sector..sector, lfn_start_idx..当前
                        if lfn_len > 0 && lfn_start_sector == sector {
                            for li in lfn_start_idx..i {
                                buf[li * 32] = 0xE5;
                            }
                        }
                        if modified {
                            let _ = disk_write(sector, &buf);
                        }
                        lfn_len = 0;
                        return true;
                    }
                    lfn_len = 0;
                }
                if modified {
                    let _ = disk_write(sector, &buf);
                }
            }
            match self.fat_next(cur_cluster) {
                Some(next) if next >= 2 && next < FAT32_EOC => cur_cluster = next,
                _ => return false,
            }
        }
    }

    /// 创建空目录：分配簇，写入 "." 和 ".." 条目。
    fn create_empty_dir(&self, parent_cluster: u32) -> Option<u32> {
        let cluster = self.alloc_cluster()?;
        let sector = self.cluster_to_sector(cluster);
        let mut buf = [0u8; BLOCK_SIZE];

        // "." 条目（指向自己）
        buf[0] = b'.';
        for i in 1..8 { buf[i] = b' '; }
        for i in 8..11 { buf[i] = b' '; }
        buf[11] = ATTR_DIRECTORY;
        buf[20] = (cluster >> 16) as u8;
        buf[21] = (cluster >> 24) as u8;
        buf[26] = (cluster & 0xFF) as u8;
        buf[27] = ((cluster >> 8) & 0xFF) as u8;
        // file_size = 0

        // ".." 条目（指向父目录）
        buf[32] = b'.';
        buf[33] = b'.';
        for i in 34..40 { buf[i] = b' '; }
        for i in 40..43 { buf[i] = b' '; }
        buf[43] = ATTR_DIRECTORY;
        buf[52] = (parent_cluster >> 16) as u8;
        buf[53] = (parent_cluster >> 24) as u8;
        buf[58] = (parent_cluster & 0xFF) as u8;
        buf[59] = ((parent_cluster >> 8) & 0xFF) as u8;

        if !disk_write(sector, &buf) {
            return None;
        }
        Some(cluster)
    }

    // ── 文件截断 ───────────────────────────────────────────

    /// 截断文件：释放 FAT 链（保留首簇），将 size 归零。
    fn truncate_file(&self, cluster: u32, dir_cluster: u32, name: &str) -> bool {
        if cluster < 2 {
            return true;
        }
        // 释放首簇之后的链
        if let Some(next) = self.fat_next(cluster) {
            if next >= 2 && next < FAT32_EOC {
                self.free_chain(next);
            }
        }
        // 标记首簇为 EOC
        self.fat_set(cluster, FAT32_EOC);
        // 清零首簇数据
        let sector = self.cluster_to_sector(cluster);
        let zero = [0u8; BLOCK_SIZE];
        for i in 0..self.bpb.sectors_per_cluster as u64 {
            disk_write(sector + i, &zero);
        }
        // 更新目录项 size = 0
        self.update_dir_entry_size(dir_cluster, name, 0, cluster)
    }
}

// ── 格式化 RAM Disk 为 FAT32 ────────────────────────────────

fn format_fat32() -> bool {
    let total_sectors = RAMDISK_SECTORS as u32;
    let spc: u8 = 1; // 每簇 1 扇区（512 字节）
    let reserved: u16 = 32;
    let num_fats: u8 = 1;
    // FAT32 表大小（扇区数）：每个簇占 4 字节，每扇区 512 字节
    let fat_entries = total_sectors / spc as u32;
    let fat_size = (fat_entries * 4 + BLOCK_SIZE as u32 - 1) / BLOCK_SIZE as u32;

    // 构建 BPB
    let mut bpb = [0u8; BLOCK_SIZE];
    // 跳转指令
    bpb[0] = 0xEB;
    bpb[1] = 0x58;
    bpb[2] = 0x90;
    // OEM 名
    bpb[3..11].copy_from_slice(b"NEKOS   ");
    // bytes_per_sector = 512
    bpb[11..13].copy_from_slice(&512u16.to_le_bytes());
    // sectors_per_cluster
    bpb[13] = spc;
    // reserved_sector_count
    bpb[14..16].copy_from_slice(&reserved.to_le_bytes());
    // num_fats
    bpb[16] = num_fats;
    // root_entry_count = 0 (FAT32)
    bpb[17..19].copy_from_slice(&0u16.to_le_bytes());
    // total_sectors_16 = 0
    bpb[19..21].copy_from_slice(&0u16.to_le_bytes());
    // media = 0xF8
    bpb[21] = 0xF8;
    // fat_size_16 = 0 (FAT32)
    bpb[22..24].copy_from_slice(&0u16.to_le_bytes());
    // sectors_per_track = 1
    bpb[24..26].copy_from_slice(&1u16.to_le_bytes());
    // num_heads = 1
    bpb[26..28].copy_from_slice(&1u16.to_le_bytes());
    // hidden_sectors = 0
    bpb[28..32].copy_from_slice(&0u32.to_le_bytes());
    // total_sectors_32
    bpb[32..36].copy_from_slice(&total_sectors.to_le_bytes());
    // fat_size_32
    bpb[36..40].copy_from_slice(&fat_size.to_le_bytes());
    // ext_flags = 0
    bpb[40..42].copy_from_slice(&0u16.to_le_bytes());
    // fs_version = 0
    bpb[42..44].copy_from_slice(&0u16.to_le_bytes());
    // root_cluster = 2
    bpb[44..48].copy_from_slice(&2u32.to_le_bytes());
    // fs_info = 1
    bpb[48..50].copy_from_slice(&1u16.to_le_bytes());
    // backup_boot_sector = 6
    bpb[50..52].copy_from_slice(&6u16.to_le_bytes());
    // drive_number = 0x80
    bpb[64] = 0x80;
    // boot_signature = 0x29
    bpb[66] = 0x29;
    // volume_serial
    bpb[67..71].copy_from_slice(&0x12345678u32.to_le_bytes());
    // volume_label
    bpb[71..82].copy_from_slice(b"NEKOS      ");
    // fs_type
    bpb[82..90].copy_from_slice(b"FAT32   ");
    // 引导扇区签名
    bpb[510] = 0x55;
    bpb[511] = 0xAA;

    // 写 BPB 到扇区 0
    if !disk_write(0, &bpb) {
        return false;
    }

    // 也写一份备份到扇区 6
    if !disk_write(6, &bpb) {
        return false;
    }

    // 清零 FSInfo 扇区 (扇区 1)
    let mut fsinfo = [0u8; BLOCK_SIZE];
    fsinfo[0..4].copy_from_slice(&[0x52, 0x52, 0x61, 0x41]); // "RRaA"
    fsinfo[484..488].copy_from_slice(&[0x72, 0x72, 0x41, 0x61]); // "rrAa"
    fsinfo[488..492].copy_from_slice(&((total_sectors / spc as u32 - 33) as u32).to_le_bytes()); // free count
    fsinfo[492..496].copy_from_slice(&3u32.to_le_bytes()); // next free cluster
    fsinfo[508..512].copy_from_slice(&[0x00, 0x00, 0x55, 0xAA]);
    disk_write(1, &fsinfo);

    // 初始化 FAT 表
    let fat_start = reserved as u64;
    let mut fat_sector = [0u8; BLOCK_SIZE];
    // 簇 0 和 1 的保留项
    fat_sector[0..4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
    fat_sector[4..8].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
    // 簇 2（根目录）标记为 EOC
    fat_sector[8..12].copy_from_slice(&FAT32_EOC.to_le_bytes());
    disk_write(fat_start, &fat_sector);

    // 清零根目录簇（簇 2）
    let data_start = fat_start + fat_size as u64;
    let root_sector = data_start; // 簇 2 扇区
    disk_write(root_sector, &[0u8; BLOCK_SIZE]);

    true
}

// ── 主函数 ──────────────────────────────────────────────────

fn main() -> ! {
    // 格式化 RAM Disk
    if !format_fat32() {
        syscall::exit(1);
    }

    // 加载 FAT32
    let mut bpb_buf = [0u8; BLOCK_SIZE];
    disk_read(0, &mut bpb_buf);
    let fs = Fat32Fs::open(&bpb_buf).expect("FAT32 open failed");

    // IPC 主循环：用 recv_buf 接收所有请求（OPEN 携带路径）
    // fs-server 是单线程进程，这两个静态缓冲区只在此主循环中访问。
    let recv_path_buf = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(RECV_PATH_BUF).cast::<u8>(),
            IPC_BUFFER_SIZE,
        )
    };
    let read_data_buf = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(READ_DATA_BUF).cast::<u8>(),
            IPC_BUFFER_SIZE,
        )
    };

    loop {
        let (client, words, buf_len) = ipc::recv_buf(
            FS_ENDPOINT,
            recv_path_buf,
        ).expect("fs-server ipc_recv_buf failed");

        match words[0] {
            FS_OPEN => {
                // 接收方的路径通过缓冲区 IPC 传递
                let path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                match fs.resolve_path(path) {
                    Some((entry, _name, _name_len)) => {
                        let cluster =
                            ((entry.first_cluster_hi as u32) << 16)
                            | entry.first_cluster_lo as u32;
                        let _ = ipc::reply(client, [
                            FS_OK,
                            cluster as usize,
                            entry.file_size as usize,
                            entry.attr as usize,
                        ]);
                    }
                    None => {
                        let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                    }
                }
            }
            FS_READ => {
                // words[1] = cluster, words[2] = offset, words[3] = size
                let cluster = words[1] as u32;
                let offset = words[2] as u32;
                let size = words[3] as u32;
                let read_size = size.min(read_data_buf.len() as u32);
                let bytes_read = fs.read_file(
                    cluster,
                    offset,
                    u32::MAX, // file_size 不限制，由 FAT 链决定
                    &mut read_data_buf[..read_size as usize],
                );
                let _ = ipc::reply_buf(
                    client,
                    [FS_OK, bytes_read, 0, 0],
                    &read_data_buf[..bytes_read],
                );
            }
            FS_READDIR => {
                // words[1] = cluster
                let cluster = words[1] as u32;
                let mut count = 0usize;
                fs.walk_dir(cluster, |_entry, _name| {
                    count += 1;
                });
                let _ = ipc::reply(client, [FS_OK, count, 0, 0]);
            }
            FS_CLOSE => {
                let _ = ipc::reply(client, [FS_OK, 0, 0, 0]);
            }
            FS_MKDIR => {
                // recv_path_buf 中包含目录名
                let path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                // 解析父目录
                if let Some(slash_pos) = path.rfind('/') {
                    let parent_path = if slash_pos == 0 { "/" } else { &path[..slash_pos] };
                    let dir_name = &path[slash_pos + 1..];
                    match fs.resolve_path(parent_path) {
                        Some((parent_entry, _, _)) => {
                            let parent_cluster = ((parent_entry.first_cluster_hi as u32) << 16)
                                | parent_entry.first_cluster_lo as u32;
                            match fs.create_empty_dir(parent_cluster) {
                                Some(new_cluster) => {
                                    let _ = fs.create_dir_entry(
                                        parent_cluster,
                                        dir_name,
                                        ATTR_DIRECTORY,
                                        new_cluster,
                                        0,
                                    );
                                    let _ = ipc::reply(client, [FS_OK, new_cluster as usize, 0, 0]);
                                }
                                None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                            }
                        }
                        None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                    }
                } else {
                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                }
            }
            FS_WRITE => {
                // words[1] = cluster, words[2] = offset, words[3] = unused
                // 缓冲区数据在 recv_path_buf 中
                let mut cluster = words[1] as u32;
                let offset = words[2] as u32;
                let size = buf_len;
                // 零长度文件首次写入：先分配首簇
                if cluster < 2 {
                    match fs.alloc_cluster() {
                        Some(c) => cluster = c,
                        None => {
                            let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                            continue; // 跳过本次 loop 迭代
                        }
                    }
                }
                let (bytes_written, last_cluster) = fs.write_file(
                    cluster,
                    offset,
                    u32::MAX,
                    &recv_path_buf[..size],
                );
                let _ = ipc::reply(client, [FS_OK, bytes_written, last_cluster as usize, 0]);
            }
            FS_CREATE_OPEN => {
                // O_CREAT：路径在 recv_path_buf 中
                let path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                // 先尝试打开
                match fs.resolve_path(path) {
                    Some((entry, _, _)) => {
                        // 已存在，直接返回
                        let cluster = ((entry.first_cluster_hi as u32) << 16)
                            | entry.first_cluster_lo as u32;
                        let _ = ipc::reply(client, [
                            FS_OK, cluster as usize, entry.file_size as usize, entry.attr as usize,
                        ]);
                    }
                    None => {
                        // 不存在，创建新文件
                        if let Some(slash_pos) = path.rfind('/') {
                            let parent_path = if slash_pos == 0 { "/" } else { &path[..slash_pos] };
                            let file_name = &path[slash_pos + 1..];
                            match fs.resolve_path(parent_path) {
                                Some((parent_entry, _, _)) => {
                                    let parent_cluster = ((parent_entry.first_cluster_hi as u32) << 16)
                                        | parent_entry.first_cluster_lo as u32;
                                    // 创建目录条目，文件大小为 0，簇号为 0（写入时分配）
                                    match fs.create_dir_entry(
                                        parent_cluster,
                                        file_name,
                                        ATTR_ARCHIVE,
                                        0, // 零长度文件，cluster = 0
                                        0,
                                    ) {
                                        Some(_) => {
                                            let _ = ipc::reply(client, [FS_OK, 0, 0, ATTR_ARCHIVE as usize]);
                                        }
                                        None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                                    }
                                }
                                None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                            }
                        } else {
                            let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                        }
                    }
                }
            }
            FS_UPDATE_SIZE => {
                // words[1] = unused, words[2] = file_size, words[3] = new_cluster
                // recv_path_buf = 完整路径
                let file_size = words[2] as u32;
                let new_cluster = words[3] as u32;
                let full_path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                // 解析父目录和文件名
                if let Some(slash_pos) = full_path.rfind('/') {
                    let parent_path = if slash_pos == 0 { "/" } else { &full_path[..slash_pos] };
                    let file_name = &full_path[slash_pos + 1..];
                    match fs.resolve_path(parent_path) {
                        Some((parent_entry, _, _)) => {
                            let parent_cluster = ((parent_entry.first_cluster_hi as u32) << 16)
                                | parent_entry.first_cluster_lo as u32;
                            let ok = fs.update_dir_entry_size(
                                parent_cluster, file_name, file_size, new_cluster,
                            );
                            let _ = ipc::reply(client, [if ok { FS_OK } else { FS_ERR }, 0, 0, 0]);
                        }
                        None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                    }
                } else {
                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                }
            }
            FS_DELETE => {
                // recv_path_buf = 完整路径
                let full_path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                if let Some(slash_pos) = full_path.rfind('/') {
                    let parent_path = if slash_pos == 0 { "/" } else { &full_path[..slash_pos] };
                    let file_name = &full_path[slash_pos + 1..];
                    match fs.resolve_path(parent_path) {
                        Some((parent_entry, _, _)) => {
                            let parent_cluster = ((parent_entry.first_cluster_hi as u32) << 16)
                                | parent_entry.first_cluster_lo as u32;
                            if let Some((entry, _, _)) = fs.find_entry(parent_cluster, file_name) {
                                let cluster = ((entry.first_cluster_hi as u32) << 16)
                                    | entry.first_cluster_lo as u32;
                                if fs.delete_dir_entry(parent_cluster, file_name) {
                                    if cluster >= 2 {
                                        fs.free_chain(cluster);
                                    }
                                    let _ = ipc::reply(client, [FS_OK, 0, 0, 0]);
                                } else {
                                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                                }
                            } else {
                                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                            }
                        }
                        None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                    }
                } else {
                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                }
            }
            FS_TRUNCATE => {
                // recv_path_buf = 完整路径
                let full_path = core::str::from_utf8(&recv_path_buf[..buf_len])
                    .unwrap_or("")
                    .trim_end_matches('\0');
                if let Some(slash_pos) = full_path.rfind('/') {
                    let parent_path = if slash_pos == 0 { "/" } else { &full_path[..slash_pos] };
                    let file_name = &full_path[slash_pos + 1..];
                    match fs.resolve_path(parent_path) {
                        Some((parent_entry, _, _)) => {
                            let parent_cluster = ((parent_entry.first_cluster_hi as u32) << 16)
                                | parent_entry.first_cluster_lo as u32;
                            if let Some((entry, _, _)) = fs.find_entry(parent_cluster, file_name) {
                                let cluster = ((entry.first_cluster_hi as u32) << 16)
                                    | entry.first_cluster_lo as u32;
                                if cluster >= 2 {
                                    let ok = fs.truncate_file(cluster, parent_cluster, file_name);
                                    let _ = ipc::reply(client, [if ok { FS_OK } else { FS_ERR }, 0, 0, 0]);
                                } else {
                                    let _ = ipc::reply(client, [FS_OK, 0, 0, 0]);
                                }
                            } else {
                                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                            }
                        }
                        None => { let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]); }
                    }
                } else {
                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                }
            }
            _ => {
                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
            }
        }
    }
}

entry!(main);
