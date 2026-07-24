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

// 返回码
const FS_OK: usize = 0;
const FS_ERR: usize = usize::MAX;

// ── 块设备抽象 ──────────────────────────────────────────────

const BLOCK_SIZE: usize = 512;

// RAM Disk：4MB，共 8192 个扇区
const RAMDISK_SIZE: usize = 4 * 1024 * 1024;
const RAMDISK_SECTORS: usize = RAMDISK_SIZE / BLOCK_SIZE;

static mut RAMDISK: [u8; RAMDISK_SIZE] = [0u8; RAMDISK_SIZE];

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

    // IPC 主循环
    loop {
        let (client, words) = ipc::recv(FS_ENDPOINT).expect("fs-server ipc_recv failed");

        match words[0] {
            FS_OPEN => {
                // words[1..4] 未使用（路径通过缓冲区 IPC 传递）
                // 简单实现：返回目录条目信息
                // 目前暂不支持缓冲区 IPC 路径，先用固定路径测试
                let entry = fs.resolve_path("/");
                if entry.is_some() {
                    let _ = ipc::reply(client, [FS_OK, fs.root_cluster as usize, 0, 0]);
                } else {
                    let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
                }
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
            FS_READ => {
                // words[1] = cluster, words[2] = offset, words[3] = size
                let cluster = words[1] as u32;
                let offset = words[2] as u32;
                let size = words[3] as u32;
                let _ = (cluster, offset, size);
                // TODO: 通过缓冲区 IPC 返回数据
                let _ = ipc::reply(client, [FS_OK, 0, 0, 0]);
            }
            FS_MKDIR => {
                // TODO: 创建目录
                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
            }
            FS_WRITE => {
                // TODO: 写文件
                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
            }
            _ => {
                let _ = ipc::reply(client, [FS_ERR, 0, 0, 0]);
            }
        }
    }
}

entry!(main);
