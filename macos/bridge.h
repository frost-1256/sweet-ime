#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct Context SweetContext;

bool sweet_macos_load(const char *dictionary);
SweetContext *sweet_macos_new(void);
void sweet_macos_free(SweetContext *context);
void sweet_macos_activate(SweetContext *context);
bool sweet_macos_key(SweetContext *context, uint16_t vk, uint32_t character, uint8_t modifiers);
bool sweet_macos_modifier(SweetContext *context, uint16_t vk, bool pressed, bool alone);
bool sweet_macos_select(SweetContext *context, size_t row);
void sweet_macos_finish(SweetContext *context, bool commit);
const char *sweet_macos_snapshot(SweetContext *context);
bool sweet_macos_live_pending(const SweetContext *context);
bool sweet_macos_apply_live(SweetContext *context);
