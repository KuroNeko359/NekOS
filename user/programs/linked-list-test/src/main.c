#include <stdio.h>
#include <stdlib.h>

typedef struct node{
    int data;
    struct node *next;
} node_t;

int main() {
    node_t *head = malloc(sizeof(node_t));
    head->data = 10;
    head->next = NULL;

    node_t *tail = head;
    tail->next = malloc(sizeof(node_t));
    tail->next->data = 20;
    tail->next->next = NULL;

    printf("head->data: %d\n", head->data);
    printf("tail->next->data: %d\n", tail->next->data);
    return 0;
}