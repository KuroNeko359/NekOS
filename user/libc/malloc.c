#include <stddef.h>
#include <stdint.h>
#include <unistd.h>

static void mem_set(void *dst, int c, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    while (n--) {
        *d++ = (unsigned char)c;
    }
}

static void mem_cpy(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    while (n--) {
        *d++ = *s++;
    }
}

/*
 * 基于 sbrk 的简单 first-fit 空闲链表内存分配器。
 *
 * 每个内存块布局:
 *   +--------+---------------------+
 *   | header | payload ...         |
 *   +--------+---------------------+
 *
 * header 中保存块大小（包含 header 本身）和空闲标志。
 * payload 区域的地址对齐到 16 字节（header 大小也是 16 字节）。
 */

#define ALIGNMENT 16
#define ALIGN_UP(x) (((x) + (ALIGNMENT - 1)) & ~(ALIGNMENT - 1))

typedef struct block_header {
    size_t size;             /* 整个块大小（含 header），最低位为 free 标志 */
    struct block_header *next; /* 空闲链表指针 */
} block_header_t;

/* 最小可分配块大小：header + 最小 payload(16) = 32 */
#define MIN_BLOCK_SIZE 32

#define HEADER_SIZE ALIGNMENT /* sizeof(block_header_t) 对齐后为 16 */

/* 从 header 中读取/写入标志位 */
#define GET_SIZE(h)    ((h)->size & ~0x1UL)
#define IS_FREE(h)     ((h)->size & 0x1UL)
#define SET_USED(h)    ((h)->size &= ~0x1UL)
#define SET_FREE(h)    ((h)->size |= 0x1UL)

/* 从 payload 指针获取 header */
#define HDR_FROM_PTR(ptr) ((block_header_t *)((char *)(ptr) - HEADER_SIZE))
/* 从 header 获取 payload 指针 */
#define PTR_FROM_HDR(h)   ((void *)((char *)(h) + HEADER_SIZE))

static block_header_t *free_list = NULL;

/*
 * 从系统申请新的内存块并加入空闲链表。
 */
static block_header_t *request_more(size_t total) {
    block_header_t *blk = (block_header_t *)sbrk((intptr_t)total);
    if (blk == (void *)-1) {
        return NULL;
    }
    blk->size = total;
    SET_FREE(blk);
    blk->next = NULL;
    return blk;
}

/*
 * 将空闲块插入空闲链表头部。
 */
static void add_to_free_list(block_header_t *blk) {
    SET_FREE(blk);
    blk->next = free_list;
    free_list = blk;
}

/*
 * 从空闲链表中移除指定块。
 */
static void remove_from_free_list(block_header_t *blk) {
    if (free_list == blk) {
        free_list = blk->next;
        return;
    }
    block_header_t *prev = free_list;
    while (prev && prev->next != blk) {
        prev = prev->next;
    }
    if (prev) {
        prev->next = blk->next;
    }
    blk->next = NULL;
}

void *malloc(size_t size) {
    if (size == 0) {
        return NULL;
    }

    size_t total = ALIGN_UP(size + HEADER_SIZE);
    if (total < MIN_BLOCK_SIZE) {
        total = MIN_BLOCK_SIZE;
    }

    /* first-fit: 遍历空闲链表 */
    block_header_t *blk = free_list;
    block_header_t *prev = NULL;
    while (blk) {
        if (GET_SIZE(blk) >= total) {
            /* 如果剩余空间足够大，拆分块 */
            if (GET_SIZE(blk) >= total + MIN_BLOCK_SIZE) {
                block_header_t *split = (block_header_t *)((char *)blk + total);
                split->size = GET_SIZE(blk) - total;
                SET_FREE(split);
                split->next = blk->next;

                blk->size = total;

                /* 用 split 替换 blk 在空闲链表中的位置 */
                if (prev) {
                    prev->next = split;
                } else {
                    free_list = split;
                }
            } else {
                /* 不拆分，直接从空闲链表移除 */
                if (prev) {
                    prev->next = blk->next;
                } else {
                    free_list = blk->next;
                }
            }
            SET_USED(blk);
            blk->next = NULL;
            return PTR_FROM_HDR(blk);
        }
        prev = blk;
        blk = blk->next;
    }

    /* 空闲链表中没有合适的块，向系统申请 */
    blk = request_more(total);
    if (!blk) {
        return NULL;
    }
    /* request_more 返回的块已经是空闲的，直接标记使用即可 */
    remove_from_free_list(blk);
    SET_USED(blk);
    return PTR_FROM_HDR(blk);
}

void free(void *ptr) {
    if (!ptr) {
        return;
    }

    block_header_t *blk = HDR_FROM_PTR(ptr);
    SET_FREE(blk);

    add_to_free_list(blk);
}

void *calloc(size_t nmemb, size_t size) {
    /* 检查乘法溢出 */
    if (nmemb != 0 && size > (size_t)-1 / nmemb) {
        return NULL;
    }

    size_t total = nmemb * size;
    void *ptr = malloc(total);
    if (ptr) {
        mem_set(ptr, 0, total);
    }
    return ptr;
}

void *realloc(void *ptr, size_t size) {
    if (!ptr) {
        return malloc(size);
    }
    if (size == 0) {
        free(ptr);
        return NULL;
    }

    block_header_t *blk = HDR_FROM_PTR(ptr);
    size_t old_payload = GET_SIZE(blk) - HEADER_SIZE;

    /* 新的 payload 大小不超过旧的，直接返回 */
    if (size <= old_payload) {
        return ptr;
    }

    /* 分配新块并拷贝数据 */
    void *new_ptr = malloc(size);
    if (!new_ptr) {
        return NULL;
    }
    mem_cpy(new_ptr, ptr, old_payload);
    free(ptr);
    return new_ptr;
}
