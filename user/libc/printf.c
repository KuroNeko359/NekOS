#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

static int emit_char(char byte) {
    return write(1, &byte, 1) == 1 ? 1 : EOF;
}

static int emit_string(const char *text) {
    int count = 0;
    if (text == 0) {
        text = "(null)";
    }
    while (*text != '\0') {
        if (emit_char(*text++) == EOF) {
            return EOF;
        }
        ++count;
    }
    return count;
}

static int print_unsigned(unsigned long value, unsigned int base) {
    static const char digits[] = "0123456789abcdef";
    char buffer[sizeof(unsigned long) * 8];
    int length = 0;
    int count = 0;

    do {
        buffer[length++] = digits[value % base];
        value /= base;
    } while (value != 0);

    while (length > 0) {
        if (emit_char(buffer[--length]) == EOF) {
            return EOF;
        }
        ++count;
    }
    return count;
}

static int print_signed(long value) {
    int count = 0;
    unsigned long magnitude;

    if (value < 0) {
        if (emit_char('-') == EOF) {
            return EOF;
        }
        ++count;
        magnitude = 0UL - (unsigned long)value;
    } else {
        magnitude = (unsigned long)value;
    }

    int digits = print_unsigned(magnitude, 10);
    return digits == EOF ? EOF : count + digits;
}

int putchar(int character) {
    unsigned char byte = (unsigned char)character;
    return emit_char((char)byte) == EOF ? EOF : (int)byte;
}

int puts(const char *text) {
    if (emit_string(text) == EOF || emit_char('\n') == EOF) {
        return EOF;
    }
    return 0;
}

int printf(const char *format, ...) {
    if (format == 0) {
        return EOF;
    }

    va_list args;
    int total = 0;
    va_start(args, format);

    while (*format != '\0') {
        int written;
        if (*format != '%') {
            written = emit_char(*format++);
            if (written == EOF) {
                total = EOF;
                break;
            }
            total += written;
            continue;
        }

        ++format;
        switch (*format) {
        case '\0':
            written = emit_char('%');
            break;
        case '%':
            written = emit_char('%');
            break;
        case 'c':
            written = emit_char((char)va_arg(args, int));
            break;
        case 's':
            written = emit_string(va_arg(args, const char *));
            break;
        case 'd':
            written = print_signed((long)va_arg(args, int));
            break;
        case 'u':
            written = print_unsigned(
                (unsigned long)va_arg(args, unsigned int),
                10
            );
            break;
        case 'x':
            written = print_unsigned(
                (unsigned long)va_arg(args, unsigned int),
                16
            );
            break;
        case 'p':
            written = emit_string("0x");
            if (written != EOF) {
                int digits = print_unsigned(
                    (unsigned long)(uintptr_t)va_arg(args, void *),
                    16
                );
                written = digits == EOF ? EOF : written + digits;
            }
            break;
        case 'l':
            ++format;
            switch (*format) {
            case 'd':
                written = print_signed(va_arg(args, long));
                break;
            case 'u':
                written = print_unsigned(va_arg(args, unsigned long), 10);
                break;
            case 'x':
                written = print_unsigned(va_arg(args, unsigned long), 16);
                break;
            default:
                written = emit_string("%l");
                if (written != EOF && *format != '\0') {
                    int suffix = emit_char(*format);
                    written = suffix == EOF ? EOF : written + suffix;
                }
                break;
            }
            break;
        default:
            written = emit_char('%');
            if (written != EOF) {
                int suffix = emit_char(*format);
                written = suffix == EOF ? EOF : written + suffix;
            }
            break;
        }

        if (written == EOF) {
            total = EOF;
            break;
        }
        total += written;
        if (*format == '\0') {
            break;
        }
        ++format;
    }

    va_end(args);
    return total;
}
