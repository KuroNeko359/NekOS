//! Sv39 页表管理

use crate::arch::riscv::*;
use crate::kernel::page;

/// 页表项类型
pub type PageTableEntry = usize;

/// 页表 (512个页表项 = 4KB)
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    /// 创建新的空页表
    pub fn new() -> Self {
        Self {
            entries: [0; 512],
        }
    }
    
    /// 清空页表
    pub fn clear(&mut self) {
        self.entries = [0; 512];
    }
}

/// 创建新的页表
pub fn create() -> Option<&'static mut PageTable> {
    let addr = page::alloc()?;
    let pt = unsafe { &mut *(addr as *mut PageTable) };
    pt.clear();
    Some(pt)
}

/// 释放页表及其所有子页表
pub fn free_walk(pt: &mut PageTable) {
    for i in 0..512 {
        let pte = pt.entries[i];
        if (pte & PTE_V) != 0 {
            let pa = pte_to_pa(pte);
            
            if (pte & (PTE_R | PTE_W | PTE_X)) == 0 {
                // 这是一个子页表，递归释放
                let child = unsafe { &mut *(pa as *mut PageTable) };
                free_walk(child);
                page::free(pa);
            }
        }
    }
}

/// 释放页表
pub fn free(pt: &mut PageTable) {
    let addr = pt as *mut PageTable as usize;
    free_walk(pt);
    page::free(addr);
}

/// 获取PTE的物理地址
pub fn pte_to_pa(pte: PageTableEntry) -> usize {
    ((pte >> 10) & 0x0FFF_FFFF_FFFF) << PAGE_SHIFT
}

/// 设置PTE的物理地址
pub fn pa_to_pte(pa: usize, flags: usize) -> PageTableEntry {
    ((pa >> PAGE_SHIFT) << 10) | flags
}

/// 页表遍历
pub fn walk(pt: &mut PageTable, va: usize, alloc: bool) -> Option<&mut PageTableEntry> {
    let mut current_pt = pt;
    
    // 提取VPN
    let vpn = [
        (va >> 12) & 0x1FF,  // VPN[0]
        (va >> 21) & 0x1FF,  // VPN[1]
        (va >> 30) & 0x1FF,  // VPN[2]
    ];
    
    for level in (1..3).rev() {
        let idx = vpn[level];
        let pte = current_pt.entries[idx];
        
        if (pte & PTE_V) == 0 {
            // 页表项无效
            if alloc {
                // 分配新的页表
                let new_pt = create()?;
                let pa = new_pt as *mut PageTable as usize;
                current_pt.entries[idx] = pa_to_pte(pa, PTE_V);
                
                // 递归到下一级
                current_pt = new_pt;
            } else {
                return None;
            }
        } else if (pte & (PTE_R | PTE_W | PTE_X)) != 0 {
            // 这是一个叶子页表项
            return None;
        } else {
            // 这是一个子页表
            let pa = pte_to_pa(pte);
            current_pt = unsafe { &mut *(pa as *mut PageTable) };
        }
    }
    
    // 返回最后一级的页表项
    Some(&mut current_pt.entries[vpn[0]])
}

/// 映射页面
pub fn map(pt: &mut PageTable, va: usize, pa: usize, size: usize, flags: usize) -> Result<(), ()> {
    if size == 0 || (va % PAGE_SIZE) != 0 || (pa % PAGE_SIZE) != 0 {
        return Err(());
    }
    
    let num_pages = size / PAGE_SIZE;
    
    for i in 0..num_pages {
        let current_va = va + i * PAGE_SIZE;
        let current_pa = pa + i * PAGE_SIZE;
        
        // 查找或创建页表项
        let pte = walk(pt, current_va, true).ok_or(())?;
        
        if (*pte & PTE_V) != 0 {
            // 已经映射
            return Err(());
        }
        
        // 设置页表项
        *pte = pa_to_pte(current_pa, flags | PTE_V);
    }
    
    Ok(())
}

/// 修改一段已有映射的权限。
pub fn set_flags(pt: &mut PageTable, va: usize, size: usize, flags: usize) -> Result<(), ()> {
    if size == 0 || (va % PAGE_SIZE) != 0 || (size % PAGE_SIZE) != 0 {
        return Err(());
    }

    for offset in (0..size).step_by(PAGE_SIZE) {
        let pte = walk(pt, va + offset, false).ok_or(())?;
        if (*pte & PTE_V) == 0 {
            return Err(());
        }
        let pa = pte_to_pa(*pte);
        *pte = pa_to_pte(pa, flags | PTE_V);
    }
    Ok(())
}

/// 取消映射
pub fn unmap(pt: &mut PageTable, va: usize, size: usize, free_page: bool) -> Result<(), ()> {
    if size == 0 || (va % PAGE_SIZE) != 0 {
        return Err(());
    }
    
    let num_pages = size / PAGE_SIZE;
    
    for i in 0..num_pages {
        let current_va = va + i * PAGE_SIZE;
        
        // 查找页表项
        let pte = walk(pt, current_va, false).ok_or(())?;
        
        if (*pte & PTE_V) == 0 {
            // 未映射
            continue;
        }
        
        if free_page {
            let pa = pte_to_pa(*pte);
            page::free(pa);
        }
        
        // 清除页表项
        *pte = 0;
    }
    
    // 刷新TLB
    crate::arch::riscv::csr::sfence_vma();
    
    Ok(())
}

/// 虚拟地址转物理地址
pub fn virt_to_phys(pt: &mut PageTable, va: usize) -> Option<usize> {
    let pte = walk(pt, va, false)?;
    
    if (*pte & PTE_V) == 0 {
        return None;
    }
    
    let pa = pte_to_pa(*pte);
    let offset = va & (PAGE_SIZE - 1);
    
    Some(pa + offset)
}
