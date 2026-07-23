#ifndef NEKOS_UNISTD_H
#define NEKOS_UNISTD_H

#include <sys/types.h>

ssize_t read(int fd, void *buffer, size_t length);
ssize_t write(int fd, const void *buffer, size_t length);

__attribute__((noreturn)) void _exit(int status);
pid_t getpid(void);
pid_t fork(void);

/*
 * nekos 暂时没有 argv/envp 支持。execve 会加载 path 指定的 initrd 程序，
 * 但当前忽略 argv 和 envp。
 */
int execve(
    const char *path,
    char *const argv[],
    char *const envp[]
);

#endif
