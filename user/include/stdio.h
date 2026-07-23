#ifndef NEKOS_STDIO_H
#define NEKOS_STDIO_H

#define EOF (-1)

/*
 * nekos 尚未实现文件系统和 FILE，因此当前只提供面向标准输出的
 * 最小 stdio 子集。
 */
int putchar(int character);
int puts(const char *text);
int printf(const char *format, ...);

#endif
