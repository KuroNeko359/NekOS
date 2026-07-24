#include <errno.h>
#include <nekos.h>
#include <fcntl.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int errno;

/* fd_table.c 中的内部函数 */
extern int __fd_is_file(int fd);
extern long __fd_read(int fd, void *buffer, nekos_size_t length);
extern long __fd_write(int fd, const void *buffer, nekos_size_t length);

ssize_t write(int fd, const void *buffer, size_t length) {
    if (fd == 1 || fd == 2) {
        long result = nekos_write(fd, buffer, length);
        if (result < 0) {
            errno = EIO;
            return -1;
        }
        return (ssize_t)result;
    }
    if (__fd_is_file(fd)) {
        long result = __fd_write(fd, buffer, (nekos_size_t)length);
        if (result < 0) {
            errno = EIO;
            return -1;
        }
        return (ssize_t)result;
    }
    errno = EBADF;
    return -1;
}

ssize_t read(int fd, void *buffer, size_t length) {
    if (fd == 0) {
        long result = nekos_read(fd, buffer, length);
        if (result < 0) {
            errno = EIO;
            return -1;
        }
        return (ssize_t)result;
    }
    if (__fd_is_file(fd)) {
        long result = __fd_read(fd, buffer, (nekos_size_t)length);
        if (result < 0) {
            errno = EIO;
            return -1;
        }
        return (ssize_t)result;
    }
    errno = EBADF;
    return -1;
}

__attribute__((noreturn)) void _exit(int status) {
    nekos_exit(status);
}

__attribute__((noreturn)) void _Exit(int status) {
    nekos_exit(status);
}

__attribute__((noreturn)) void exit(int status) {
    /*
     * 尚未实现 stdio 缓冲、atexit 处理器和析构函数，因此目前与 _exit
     * 行为相同。
     */
    nekos_exit(status);
}

pid_t getpid(void) {
    return (pid_t)nekos_getpid();
}

pid_t fork(void) {
    long result = nekos_fork();
    if (result < 0) {
        errno = EIO;
        return -1;
    }
    return (pid_t)result;
}

void *sbrk(intptr_t increment) {
    void *result = nekos_sbrk((long)increment);
    if (result == (void *)-1) {
        errno = ENOMEM;
    }
    return result;
}

int execve(
    const char *path,
    char *const argv[],
    char *const envp[]
) {
    (void)argv;
    (void)envp;
    if (path == 0) {
        errno = EINVAL;
        return -1;
    }

    long result = nekos_exec(path);
    if (result < 0) {
        errno = ENOENT;
        return -1;
    }
    return (int)result;
}

pid_t waitpid(pid_t pid, int *status, int options) {
    /*
     * 内核目前只支持等待指定的正 PID，不支持 WNOHANG 或 pid <= 0
     * 所代表的进程组语义。
     */
    if (pid <= 0 || options != 0) {
        errno = EINVAL;
        return -1;
    }

    long exit_code = nekos_waitpid((unsigned int)pid);
    if (exit_code < 0) {
        errno = ECHILD;
        return -1;
    }
    if (status != 0) {
        *status = ((int)exit_code & 0xff) << 8;
    }
    return pid;
}
