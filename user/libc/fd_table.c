#include <fcntl.h>
#include <nekos.h>
#include <errno.h>
#include <unistd.h>

#define NEKOS_FS_ENDPOINT 2UL

/* IPC 协议 — 与 fs-server/src/main.rs 保持一致 */
#define FS_OPEN         1
#define FS_CLOSE        2
#define FS_READ         3
#define FS_WRITE        4
#define FS_STAT         5
#define FS_MKDIR        6
#define FS_READDIR      7
#define FS_CREATE_OPEN  8
#define FS_UPDATE_SIZE  9
#define FS_DELETE       10
#define FS_TRUNCATE     11

#define FS_OK           0
#define FS_ERR          ((nekos_word_t)-1)

#define SYS_IPC_CALL_BUF 15

#define MAX_FD 16
#define MAX_PATH 256

/* 每个 fd 的句柄信息 */
struct fd_entry {
    int in_use;
    int flags;              /* 打开时的 flags */
    nekos_word_t cluster;   /* FAT32 簇号 */
    nekos_word_t file_size;
    nekos_word_t offset;    /* 当前读写偏移 */
    nekos_word_t attr;      /* 文件属性 */
    int dirty;              /* 是否有未同步的写入 */
    char path[MAX_PATH];    /* 打开时的路径（用于 FS_UPDATE_SIZE） */
};

static struct fd_entry fd_table[MAX_FD];
static int fd_table_initialized = 0;

static void fd_table_init(void) {
    for (int i = 0; i < MAX_FD; i++) {
        fd_table[i].in_use = 0;
    }
    fd_table[0].in_use = 1;
    fd_table[1].in_use = 1;
    fd_table[2].in_use = 1;
    fd_table_initialized = 1;
}

static int alloc_fd(void) {
    for (int i = 3; i < MAX_FD; i++) {
        if (!fd_table[i].in_use) {
            return i;
        }
    }
    return -1;
}

/* 内部辅助：通过 ipc_call_buf 发送路径，返回 reply words */
static long call_buf_with_path(nekos_word_t op, const char *path,
                               nekos_word_t *r0, nekos_word_t *r1,
                               nekos_word_t *r2, nekos_word_t *r3) {
    nekos_size_t path_len = 0;
    const char *p = path;
    while (*p) { path_len++; p++; }
    path_len++;

    register nekos_word_t a0 asm("a0") = NEKOS_FS_ENDPOINT;
    register nekos_word_t a1 asm("a1") = op;
    register nekos_word_t a2 asm("a2") = 0;
    register nekos_word_t a3 asm("a3") = 0;
    register nekos_word_t a4 asm("a4") = 0;
    register nekos_word_t a5 asm("a5") = (nekos_word_t)path;
    register nekos_word_t a6 asm("a6") = path_len;
    register nekos_word_t a7 asm("a7") = SYS_IPC_CALL_BUF;
    asm volatile(
        "ecall"
        : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
        : "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );
    *r0 = a0; *r1 = a1; *r2 = a2; *r3 = a3;
    return a0;
}

/* 内部辅助：保存路径到 fd 表 */
static void save_path(int fd, const char *path) {
    int i;
    for (i = 0; i < MAX_PATH - 1 && path[i]; i++) {
        fd_table[fd].path[i] = path[i];
    }
    fd_table[fd].path[i] = '\0';
}

/* 提取文件名（最后一个 / 之后的部分） */
static const char *basename_of(const char *path) {
    const char *last = path;
    while (*path) {
        if (*path == '/') last = path + 1;
        path++;
    }
    return last;
}

/* 通过 FS_UPDATE_SIZE 更新目录条目 */
static void sync_dir_entry(int fd) {
    if (!fd_table[fd].dirty) return;

    const char *name = basename_of(fd_table[fd].path);
    nekos_size_t name_len = 0;
    const char *p = name;
    while (*p) { name_len++; p++; }
    name_len++;

    register nekos_word_t a0 asm("a0") = NEKOS_FS_ENDPOINT;
    register nekos_word_t a1 asm("a1") = FS_UPDATE_SIZE;
    register nekos_word_t a2 asm("a2") = 0; /* dir_cluster — fs-server 自行解析 */
    register nekos_word_t a3 asm("a3") = fd_table[fd].file_size;
    register nekos_word_t a4 asm("a4") = fd_table[fd].cluster;
    register nekos_word_t a5 asm("a5") = (nekos_word_t)fd_table[fd].path;
    register nekos_word_t a6 asm("a6") = MAX_PATH; /* 发送完整路径 */
    register nekos_word_t a7 asm("a7") = SYS_IPC_CALL_BUF;
    asm volatile(
        "ecall"
        : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
        : "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );
    (void)name;
    (void)name_len;
    fd_table[fd].dirty = 0;
}

int open(const char *path, int flags, ...) {
    if (!fd_table_initialized) fd_table_init();

    int fd = alloc_fd();
    if (fd < 0) {
        errno = EMFILE;
        return -1;
    }

    nekos_word_t r0, r1, r2, r3;
    int is_creat = (flags & O_CREAT) != 0;

    if (is_creat) {
        call_buf_with_path(FS_CREATE_OPEN, path, &r0, &r1, &r2, &r3);
    } else {
        call_buf_with_path(FS_OPEN, path, &r0, &r1, &r2, &r3);
    }

    if (r0 != FS_OK) {
        errno = ENOENT;
        return -1;
    }

    fd_table[fd].in_use = 1;
    fd_table[fd].flags = flags;
    fd_table[fd].cluster = r1;
    fd_table[fd].file_size = r2;
    fd_table[fd].attr = r3;
    fd_table[fd].offset = 0;
    fd_table[fd].dirty = 0;
    save_path(fd, path);

    /* O_TRUNC：截断已有文件 */
    if ((flags & O_TRUNC) && fd_table[fd].file_size > 0) {
        if (fd_table[fd].cluster >= 2) {
            nekos_size_t path_len = 0;
            const char *pp = path;
            while (*pp) { path_len++; pp++; }
            path_len++;
            register nekos_word_t a0 asm("a0") = NEKOS_FS_ENDPOINT;
            register nekos_word_t a1 asm("a1") = FS_TRUNCATE;
            register nekos_word_t a2 asm("a2") = 0;
            register nekos_word_t a3 asm("a3") = 0;
            register nekos_word_t a4 asm("a4") = 0;
            register nekos_word_t a5 asm("a5") = (nekos_word_t)path;
            register nekos_word_t a6 asm("a6") = path_len;
            register nekos_word_t a7 asm("a7") = SYS_IPC_CALL_BUF;
            asm volatile(
                "ecall"
                : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
                : "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                : "memory"
            );
        }
        fd_table[fd].file_size = 0;
    }

    /* O_APPEND：定位到文件末尾 */
    if (flags & O_APPEND) {
        fd_table[fd].offset = fd_table[fd].file_size;
    }

    return fd;
}

int close(int fd) {
    if (!fd_table_initialized) fd_table_init();
    if (fd < 0 || fd >= MAX_FD || !fd_table[fd].in_use) {
        errno = EBADF;
        return -1;
    }
    if (fd <= 2) {
        return 0;
    }

    /* 如果有未同步的写入，先更新目录条目 */
    sync_dir_entry(fd);

    nekos_word_t request[4] = { FS_CLOSE, fd_table[fd].cluster, 0, 0 };
    nekos_word_t reply[4];
    nekos_ipc_call(NEKOS_FS_ENDPOINT, request, reply);

    fd_table[fd].in_use = 0;
    fd_table[fd].cluster = 0;
    fd_table[fd].file_size = 0;
    fd_table[fd].offset = 0;
    fd_table[fd].flags = 0;
    fd_table[fd].dirty = 0;
    fd_table[fd].path[0] = '\0';
    return 0;
}

/* 供 posix.c 调用的内部函数 */

int __fd_is_file(int fd) {
    if (!fd_table_initialized) fd_table_init();
    return fd >= 3 && fd < MAX_FD && fd_table[fd].in_use;
}

long __fd_read(int fd, void *buffer, nekos_size_t length) {
    if (!fd_table_initialized) fd_table_init();
    if (fd < 3 || fd >= MAX_FD || !fd_table[fd].in_use) {
        return NEKOS_ERROR;
    }

    if (fd_table[fd].offset >= fd_table[fd].file_size) {
        return 0;
    }

    nekos_size_t remaining = fd_table[fd].file_size - fd_table[fd].offset;
    if (length > remaining) {
        length = remaining;
    }

    register nekos_word_t a0 asm("a0") = NEKOS_FS_ENDPOINT;
    register nekos_word_t a1 asm("a1") = FS_READ;
    register nekos_word_t a2 asm("a2") = fd_table[fd].cluster;
    register nekos_word_t a3 asm("a3") = fd_table[fd].offset;
    register nekos_word_t a4 asm("a4") = length;
    register nekos_word_t a5 asm("a5") = (nekos_word_t)buffer;
    register nekos_word_t a6 asm("a6") = length;
    register nekos_word_t a7 asm("a7") = SYS_IPC_CALL_BUF;
    asm volatile(
        "ecall"
        : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
        : "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );

    if (a0 == FS_OK) {
        nekos_size_t bytes_read = a1;
        fd_table[fd].offset += bytes_read;
        return (long)bytes_read;
    }
    return NEKOS_ERROR;
}

long __fd_write(int fd, const void *buffer, nekos_size_t length) {
    if (!fd_table_initialized) fd_table_init();
    if (fd < 3 || fd >= MAX_FD || !fd_table[fd].in_use) {
        return NEKOS_ERROR;
    }

    nekos_word_t cluster = fd_table[fd].cluster;

    register nekos_word_t a0 asm("a0") = NEKOS_FS_ENDPOINT;
    register nekos_word_t a1 asm("a1") = FS_WRITE;
    register nekos_word_t a2 asm("a2") = cluster;
    register nekos_word_t a3 asm("a3") = fd_table[fd].offset;
    register nekos_word_t a4 asm("a4") = length;
    register nekos_word_t a5 asm("a5") = (nekos_word_t)buffer;
    register nekos_word_t a6 asm("a6") = length;
    register nekos_word_t a7 asm("a7") = SYS_IPC_CALL_BUF;
    asm volatile(
        "ecall"
        : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
        : "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );

    if (a0 != FS_OK) {
        return NEKOS_ERROR;
    }

    nekos_size_t bytes_written = a1;
    nekos_word_t last_cluster = a2;

    /* 首次写入分配了首簇，更新本地记录 */
    if (cluster == 0 && last_cluster >= 2) {
        fd_table[fd].cluster = last_cluster;
    }

    fd_table[fd].offset += bytes_written;
    if (fd_table[fd].offset > fd_table[fd].file_size) {
        fd_table[fd].file_size = fd_table[fd].offset;
    }
    fd_table[fd].dirty = 1;

    return (long)bytes_written;
}
