#ifndef NEKOS_STDLIB_H
#define NEKOS_STDLIB_H

#include <stddef.h>

__attribute__((noreturn)) void exit(int status);
__attribute__((noreturn)) void _Exit(int status);

void *malloc(size_t size);
void free(void *ptr);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);

#endif
