#include <nekos.h>

enum {
    SYS_EXIT = 2,
    SYS_YIELD = 4,
    SYS_GETPID = 5,
    SYS_FORK = 6,
    SYS_PS = 7,
    SYS_EXEC = 8,
    SYS_WAITPID = 9,
    SYS_IPC_CALL = 10,
    SYS_IPC_RECV = 11,
    SYS_IPC_REPLY = 12,
    SYS_IRQ_WAIT = 13,
};

static nekos_size_t string_length(const char *text) {
    nekos_size_t length = 0;
    while (text[length] != '\0') {
        ++length;
    }
    return length;
}

__attribute__((noreturn)) void nekos_exit(int code) {
    register nekos_word_t a0 asm("a0") = (nekos_word_t)code;
    register nekos_word_t a7 asm("a7") = SYS_EXIT;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    for (;;) {
        asm volatile("" ::: "memory");
    }
}

void nekos_yield(void) {
    register nekos_word_t a0 asm("a0") = 0;
    register nekos_word_t a7 asm("a7") = SYS_YIELD;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
}

unsigned int nekos_getpid(void) {
    register nekos_word_t a0 asm("a0") = 0;
    register nekos_word_t a7 asm("a7") = SYS_GETPID;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return (unsigned int)a0;
}

long nekos_fork(void) {
    register nekos_word_t a0 asm("a0") = 0;
    register nekos_word_t a7 asm("a7") = SYS_FORK;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return (long)a0;
}

void nekos_ps(void) {
    register nekos_word_t a0 asm("a0") = 0;
    register nekos_word_t a7 asm("a7") = SYS_PS;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
}

long nekos_exec(const char *name) {
    register nekos_word_t a0 asm("a0") = (nekos_word_t)name;
    register nekos_word_t a1 asm("a1") = string_length(name);
    register nekos_word_t a7 asm("a7") = SYS_EXEC;
    asm volatile(
        "ecall"
        : "+r"(a0)
        : "r"(a1), "r"(a7)
        : "memory"
    );
    return (long)a0;
}

long nekos_waitpid(unsigned int pid) {
    register nekos_word_t a0 asm("a0") = pid;
    register nekos_word_t a7 asm("a7") = SYS_WAITPID;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return (long)a0;
}

long nekos_irq_wait(nekos_word_t irq) {
    register nekos_word_t a0 asm("a0") = irq;
    register nekos_word_t a7 asm("a7") = SYS_IRQ_WAIT;
    asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return (long)a0;
}

long nekos_ipc_call(
    nekos_word_t endpoint,
    const nekos_word_t request[4],
    nekos_word_t reply[4]
) {
    register nekos_word_t a0 asm("a0") = endpoint;
    register nekos_word_t a1 asm("a1") = request[0];
    register nekos_word_t a2 asm("a2") = request[1];
    register nekos_word_t a3 asm("a3") = request[2];
    register nekos_word_t a4 asm("a4") = request[3];
    register nekos_word_t a7 asm("a7") = SYS_IPC_CALL;
    asm volatile(
        "ecall"
        : "+r"(a0), "+r"(a1), "+r"(a2), "+r"(a3)
        : "r"(a4), "r"(a7)
        : "memory"
    );
    if (a0 == (nekos_word_t)-1) {
        return NEKOS_ERROR;
    }
    reply[0] = a0;
    reply[1] = a1;
    reply[2] = a2;
    reply[3] = a3;
    return 0;
}

long nekos_ipc_recv(
    nekos_word_t endpoint,
    unsigned int *client,
    nekos_word_t words[4]
) {
    register nekos_word_t a0 asm("a0") = endpoint;
    register nekos_word_t a1 asm("a1");
    register nekos_word_t a2 asm("a2");
    register nekos_word_t a3 asm("a3");
    register nekos_word_t a4 asm("a4");
    register nekos_word_t a7 asm("a7") = SYS_IPC_RECV;
    asm volatile(
        "ecall"
        : "+r"(a0), "=r"(a1), "=r"(a2), "=r"(a3), "=r"(a4)
        : "r"(a7)
        : "memory"
    );
    if (a0 == (nekos_word_t)-1) {
        return NEKOS_ERROR;
    }
    *client = (unsigned int)a0;
    words[0] = a1;
    words[1] = a2;
    words[2] = a3;
    words[3] = a4;
    return 0;
}

long nekos_ipc_reply(unsigned int client, const nekos_word_t words[4]) {
    register nekos_word_t a0 asm("a0") = client;
    register nekos_word_t a1 asm("a1") = words[0];
    register nekos_word_t a2 asm("a2") = words[1];
    register nekos_word_t a3 asm("a3") = words[2];
    register nekos_word_t a4 asm("a4") = words[3];
    register nekos_word_t a7 asm("a7") = SYS_IPC_REPLY;
    asm volatile(
        "ecall"
        : "+r"(a0)
        : "r"(a1), "r"(a2), "r"(a3), "r"(a4), "r"(a7)
        : "memory"
    );
    return a0 == 0 ? 0 : NEKOS_ERROR;
}

long nekos_write(int fd, const void *buffer, nekos_size_t length) {
    const unsigned char *bytes = (const unsigned char *)buffer;
    if (fd != 1 && fd != 2) {
        return NEKOS_ERROR;
    }
    for (nekos_size_t index = 0; index < length; ++index) {
        nekos_word_t request[4] = {
            NEKOS_CONSOLE_WRITE,
            bytes[index],
            0,
            0,
        };
        nekos_word_t reply[4];
        if (nekos_ipc_call(NEKOS_CONSOLE_ENDPOINT, request, reply) < 0) {
            return NEKOS_ERROR;
        }
    }
    return (long)length;
}

long nekos_read(int fd, void *buffer, nekos_size_t length) {
    unsigned char *bytes = (unsigned char *)buffer;
    if (fd != 0) {
        return NEKOS_ERROR;
    }
    for (nekos_size_t index = 0; index < length; ++index) {
        const nekos_word_t request[4] = {
            NEKOS_CONSOLE_READ,
            0,
            0,
            0,
        };
        nekos_word_t reply[4];
        if (nekos_ipc_call(NEKOS_CONSOLE_ENDPOINT, request, reply) < 0) {
            return NEKOS_ERROR;
        }
        bytes[index] = (unsigned char)reply[0];
        if (bytes[index] == '\n') {
            return (long)(index + 1);
        }
    }
    return (long)length;
}
