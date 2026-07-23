#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static void puts_literal(const char *text, size_t length) {
    write(1, text, length);
}

int main(void) {
    static const char start[] = "POSIX wrapper test\n";
    static const char success[] = "fork/execve/waitpid passed\n";
    static const char failure[] = "POSIX wrapper test failed\n";

    puts_literal(start, sizeof(start) - 1);
    pid_t child = fork();
    if (child < 0) {
        puts_literal(failure, sizeof(failure) - 1);
        return 1;
    }
    if (child == 0) {
        execve("hello-c", 0, 0);
        _exit(2);
    }

    int status = 0;
    if (waitpid(child, &status, 0) != child
        || !WIFEXITED(status)
        || WEXITSTATUS(status) != 0) {
        puts_literal(failure, sizeof(failure) - 1);
        return 1;
    }

    puts_literal(success, sizeof(success) - 1);
    return 0;
}
