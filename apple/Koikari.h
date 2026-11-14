#pragma once
#include <stddef.h>
#include <stdint.h>
// All Host operations are main-thread-only; never share a Host concurrently.
// Explicit immutable-lease worker exceptions are documented below.
typedef struct Host Host;
Host *kr_open(const char *directory, const char *save_directory);
void kr_close(Host *handle);
char *kr_error(void);
char *kr_snapshot(Host *handle);
void kr_string_free(char *value);
int32_t kr_action(Host *handle, const char *action);
int32_t kr_has_save(Host *handle);
// SaveInfo JSON includes optional preview_revision: lowercase SHA256 of exact
// bounded validated save bytes; null for unsupported/unsafe preview states.
char *kr_saves(Host *handle);
typedef struct SavePreview SavePreview;
// Main-thread-only capture from the persisted slot, NOT current live scene.
// Exact-check revision from kr_saves; reject changed/corrupt/incompatible/legacy
// migration saves and native video/credits/interlude overlays. Readonly: no
// restore, tick, action, migration, writes or game/progress/gallery mutations.
// Owns immutable Scene + resource lease; survives Host close and slot overwrite.
// Failure: NULL / kr_error on the calling main thread.
SavePreview *kr_save_preview_request(Host *handle, uint32_t slot, const char *revision);
// Transfer lease to ONE worker with exclusive access. BORROWS, DOES NOT CONSUME.
// Owned fixed 384x216 PNG, <= 1 MiB: kr_bytes_free(bytes, ORIGINAL length) once.
// Failure: NULL, zero length (if nonnull), kr_error on this same worker. length
// must be writable. Dedicated headless renderer; all preview GPU allocations
// globally serialized through teardown. No Host or live Renderer is referenced.
uint8_t *kr_save_preview_png(SavePreview *request, size_t *length);
// Always free exactly once after all png calls finish, including failure/cancel.
// May free an unused lease on main, or transferred lease on its owning worker.
// Null-safe; must not race png/free or any other access. png never frees lease.
void kr_save_preview_free(SavePreview *request);
char *kr_timeline(Host *handle);
// Tick: 0 = failure (kr_error), 1 = unchanged, 2 = refresh snapshot.
// Elapsed time is capped at 1000ms per call; host pauses clock in background/modals.
int32_t kr_tick(Host *handle, uint64_t milliseconds);
// Returns owned strings (kr_string_free); video path is app-owned cache storage.
char *kr_video(Host *handle);
char *kr_message_glyph(Host *handle);
typedef struct VideoExport VideoExport;
// Capture on main thread; consume lease exactly once on a serial background worker.
VideoExport *kr_video_request(Host *handle);
char *kr_video_export(VideoExport *request);
char *kr_gallery(Host *handle);
// Exclusive main-thread Host; id is a gallery ENTRY ID, never an asset filename.
// Rechecks the current unlocked entry; returns encoded bytes, at most 32 MiB
// decompressed, without decoding or changing preview, story, progress, or saves.
// Free success exactly once with kr_bytes_free(bytes, original length).
// Failure: NULL, zero length (if nonnull), kr_error. length must be writable.
uint8_t *kr_gallery_image(Host *handle, const char *id, size_t *length);
char *kr_replays(Host *handle);
// Preview only an unlocked CG, composed by the Rust Metal renderer.
int32_t kr_gallery_show(Host *handle, const char *id);
int32_t kr_gallery_hide(Host *handle);
int32_t kr_draw(Host *handle, void *metal_layer, float width, float height);
uint8_t *kr_audio(Host *handle, const char *name, size_t *length);
uint8_t *kr_asset(Host *handle, const char *name, size_t *length);
// Main-thread-only optional decorations: exactly ui/title/TI_base_0.webp or
// ui/title/TI_タイトルロゴ.webp, at most 8 MiB decompressed. No gameplay changes.
// Owned bytes: kr_bytes_free(bytes, length). Failure: NULL, zero length, kr_error.
uint8_t *kr_ui_asset(Host *handle, const char *name, size_t *length);
// Pure decoder, no handle; may be used from background workers.
uint8_t *kr_decode_audio(const uint8_t *bytes, size_t length, size_t *output_length);
void kr_bytes_free(uint8_t *bytes, size_t length);
