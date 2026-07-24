#ifndef NEKOS_USER_H
#define NEKOS_USER_H

typedef unsigned long nekos_word_t;
typedef unsigned long nekos_size_t;

#define NEKOS_ERROR (-1L)
#define NEKOS_CONSOLE_ENDPOINT 1UL
#define NEKOS_CONSOLE_WRITE 1UL
#define NEKOS_CONSOLE_READ 2UL
#define NEKOS_UART0_IRQ 10UL

__attribute__((noreturn)) void nekos_exit(int code);
void nekos_yield(void);
unsigned int nekos_getpid(void);
long nekos_fork(void);
void nekos_ps(void);
long nekos_exec(const char *name);
long nekos_waitpid(unsigned int pid);
long nekos_irq_wait(nekos_word_t irq);
void *nekos_sbrk(long increment);

long nekos_ipc_call(
    nekos_word_t endpoint,
    const nekos_word_t request[4],
    nekos_word_t reply[4]
);
long nekos_ipc_recv(
    nekos_word_t endpoint,
    unsigned int *client,
    nekos_word_t words[4]
);
long nekos_ipc_reply(unsigned int client, const nekos_word_t words[4]);

long nekos_ipc_call_buf(
    nekos_word_t endpoint,
    const nekos_word_t request[4],
    const void *buf,
    nekos_size_t buf_len
);
long nekos_ipc_recv_buf(
    nekos_word_t endpoint,
    unsigned int *client,
    nekos_word_t words[4],
    void *buf,
    nekos_size_t capacity,
    nekos_size_t *out_len
);
long nekos_ipc_reply_buf(unsigned int client, const nekos_word_t words[4],
                         const void *buf, nekos_size_t buf_len);

long nekos_write(int fd, const void *buffer, nekos_size_t length);
long nekos_read(int fd, void *buffer, nekos_size_t length);

#endif
