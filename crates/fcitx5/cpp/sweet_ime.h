// C ABI of the sweet-fcitx5 static library (see ../src/lib.rs).
#pragma once

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SweetContext SweetContext;
typedef void (*SweetNotifyFn)(void *user);

enum {
    SWEET_MOD_SHIFT = 1,
    SWEET_MOD_CTRL = 2,
    SWEET_MOD_ALT = 4,
};

enum {
    SWEET_RESULT_CONSUMED = 1,
    SWEET_RESULT_MODE_CHANGED = 2,
};

enum {
    SWEET_SPAN_INPUT = 0,
    SWEET_SPAN_CONVERTED = 1,
    SWEET_SPAN_FOCUSED = 2,
};

bool sweet_init(void);
SweetContext *sweet_context_new(void);
void sweet_context_free(SweetContext *ctx);
void sweet_context_set_notify(SweetContext *ctx, SweetNotifyFn fn, void *user);

uint32_t sweet_process_key(SweetContext *ctx, uint32_t sym, uint32_t unicode, uint32_t mods, bool allow_cloud);
bool sweet_apply_live(SweetContext *ctx, bool allow_cloud);
bool sweet_select_candidate(SweetContext *ctx, uint32_t index);
void sweet_commit_all(SweetContext *ctx);
void sweet_reset(SweetContext *ctx);
bool sweet_is_active(SweetContext *ctx);
bool sweet_is_open(SweetContext *ctx);
void sweet_set_open(SweetContext *ctx, bool open);
void sweet_set_left_context(SweetContext *ctx, const char *text);

const char *sweet_commit_text(SweetContext *ctx);
const char *sweet_preedit_text(SweetContext *ctx);
uint32_t sweet_preedit_caret(SweetContext *ctx);
uint32_t sweet_preedit_span_count(SweetContext *ctx);
void sweet_preedit_span(SweetContext *ctx, uint32_t i, uint32_t *start, uint32_t *end, uint32_t *attr);

uint32_t sweet_candidate_count(SweetContext *ctx);
const char *sweet_candidate(SweetContext *ctx, uint32_t i);
int32_t sweet_candidate_cursor(SweetContext *ctx);
uint32_t sweet_candidate_page_start(SweetContext *ctx);
uint32_t sweet_candidate_total(SweetContext *ctx);
uint32_t sweet_candidate_kind(SweetContext *ctx);

#ifdef __cplusplus
}
#endif
