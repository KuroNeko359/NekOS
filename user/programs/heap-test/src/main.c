#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static int fail(const char *reason) {
    printf("heap test failed: %s\n", reason);
    return 1;
}

int main(void) {
    const intptr_t heap_size = 5000;
    unsigned char *base = sbrk(0);
    if (base == (void *)-1) {
        return fail("sbrk(0)");
    }

    unsigned char *memory = sbrk(heap_size);
    if (memory != base) {
        return fail("grow");
    }
    memory[0] = 0x11;
    memory[4095] = 0x22;
    memory[4096] = 0x33;
    memory[heap_size - 1] = 0x44;

    pid_t child = fork();
    if (child < 0) {
        return fail("fork");
    }
    if (child == 0) {
        if (memory[0] != 0x11
            || memory[4095] != 0x22
            || memory[4096] != 0x33
            || memory[heap_size - 1] != 0x44) {
            _exit(2);
        }
        memory[0] = 0xaa;
        _exit(7);
    }

    int status = 0;
    if (waitpid(child, &status, 0) != child
        || !WIFEXITED(status)
        || WEXITSTATUS(status) != 7) {
        return fail("waitpid");
    }
    if (memory[0] != 0x11) {
        return fail("fork isolation");
    }

    if (sbrk(-heap_size) == (void *)-1 || sbrk(0) != base) {
        return fail("shrink");
    }
    errno = 0;
    if (sbrk(-1) != (void *)-1 || errno != ENOMEM) {
        return fail("lower bound");
    }

    printf("heap test passed: base=%p bytes=%ld\n", base, (long)heap_size);
    return 0;
}
