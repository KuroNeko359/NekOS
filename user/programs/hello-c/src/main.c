#include <nekos.h>

int main(void) {
    static const char message[] = "Hello from C!\n";
    nekos_write(1, message, sizeof(message) - 1);
    return 0;
}
