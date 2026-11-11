#pragma once
#include <stddef.h>
#include <stdint.h>
// All handle operations are main-thread-only; never share a handle concurrently.
typedef struct Host Host;
Host *kr_open(const char *directory, const char *save_directory);
void kr_close(Host *handle);
char *kr_error(void);
char *kr_snapshot(Host *handle);
void kr_string_free(char *value);
int32_t kr_action(Host *handle, const char *action);
int32_t kr_has_save(Host *handle);
char *kr_saves(Host *handle);
char *kr_timeline(Host *handle);
int32_t kr_draw(Host *handle, void *metal_layer, float width, float height);
uint8_t *kr_audio(Host *handle, const char *name, size_t *length);
uint8_t *kr_asset(Host *handle, const char *name, size_t *length);
// Pure decoder, no handle; may be used from background workers.
uint8_t *kr_decode_audio(const uint8_t *bytes, size_t length, size_t *output_length);
void kr_bytes_free(uint8_t *bytes, size_t length);
