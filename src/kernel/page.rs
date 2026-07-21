//! 页面分配器

use spin::Mutex;
use crate::arch::riscv::PAGE_SIZE;
use crate::println;

/// 内核结束地址 (从链接脚本获取)
extern "C" {
    static __kernel_end: u8;
}

/// 页面分配器
pub struct PageAllocator {
    /// 页面位图 (每bit代表一个页面)
    bitmap: [u64; BITMAP_SIZE],
    /// 总页面数
    total_pages: usize,
    /// 空闲页面数
    free_pages: usize,
    /// 起始物理地址
    start_addr: usize,
}

/// 位图大小 (支持最多4096个页面 = 16MB内存)
const BITMAP_SIZE: usize = 64;
const MAX_PAGES: usize = BITMAP_SIZE * 64;

/// 全局页面分配器
static PAGE_ALLOCATOR: Mutex<Option<PageAllocator>> = Mutex::new(None);

impl PageAllocator {
    /// 创建新的页面分配器
    pub fn new(start_addr: usize, total_pages: usize) -> Self {
        Self {
            bitmap: [0; BITMAP_SIZE],
            total_pages: total_pages.min(MAX_PAGES),
            free_pages: total_pages.min(MAX_PAGES),
            start_addr,
        }
    }
    
    /// 分配一个页面
    pub fn alloc(&mut self) -> Option<usize> {
        if self.free_pages == 0 {
            return None;
        }
        
        // 查找空闲页面
        for i in 0..self.total_pages {
            let word_idx = i / 64;
            let bit_idx = i % 64;
            
            if (self.bitmap[word_idx] & (1 << bit_idx)) == 0 {
                // 标记为已分配
                self.bitmap[word_idx] |= 1 << bit_idx;
                self.free_pages -= 1;
                
                // 返回物理地址
                return Some(self.start_addr + i * PAGE_SIZE);
            }
        }
        
        None
    }
    
    /// 释放一个页面
    pub fn free(&mut self, addr: usize) {
        // 检查地址是否在范围内
        if addr < self.start_addr || addr >= self.start_addr + self.total_pages * PAGE_SIZE {
            return;
        }
        
        // 计算页面索引
        let page_idx = (addr - self.start_addr) / PAGE_SIZE;
        let word_idx = page_idx / 64;
        let bit_idx = page_idx % 64;
        
        // 检查是否已分配
        if (self.bitmap[word_idx] & (1 << bit_idx)) != 0 {
            // 标记为空闲
            self.bitmap[word_idx] &= !(1 << bit_idx);
            self.free_pages += 1;
        }
    }
    
    /// 获取总页面数
    pub fn total_pages(&self) -> usize {
        self.total_pages
    }
    
    /// 获取空闲页面数
    pub fn free_pages(&self) -> usize {
        self.free_pages
    }
}

/// 初始化页面分配器
pub fn init() {
    let kernel_end = unsafe { &__kernel_end as *const u8 as usize };
    let aligned_end = (kernel_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    
    // 假设总内存为128MB (从0x80000000开始)
    let total_mem = 128 * 1024 * 1024;
    let available_mem = total_mem - (aligned_end - 0x80000000);
    let total_pages = available_mem / PAGE_SIZE;
    
    let mut allocator = PageAllocator::new(aligned_end, total_pages);
    
    println!("page allocator: total={} free={}", total_pages, allocator.free_pages());
    
    *PAGE_ALLOCATOR.lock() = Some(allocator);
}

/// 分配一个页面
pub fn alloc() -> Option<usize> {
    PAGE_ALLOCATOR.lock().as_mut()?.alloc()
}

/// 释放一个页面
pub fn free(addr: usize) {
    if let Some(ref mut allocator) = *PAGE_ALLOCATOR.lock() {
        allocator.free(addr);
    }
}
