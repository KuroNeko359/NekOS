#ifndef NEKOS_SYS_WAIT_H
#define NEKOS_SYS_WAIT_H

#include <sys/types.h>

#define WNOHANG 1
#define WIFEXITED(status) (1)
#define WEXITSTATUS(status) (((status) >> 8) & 0xff)

pid_t waitpid(pid_t pid, int *status, int options);

#endif
