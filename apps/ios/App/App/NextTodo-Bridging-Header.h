// Local mode's server, `crates/ios` (docs/ios.md#本机模式). Those returning a string answer with JSON,
// handed back to `nexttodo_free`.

#include <stdint.h>

char *nexttodo_start(const char *dir, uint16_t port);
void nexttodo_stop(void);
char *nexttodo_session(void);
void nexttodo_pull(uint64_t wait_ms);
char *nexttodo_backup(const char *to);
char *nexttodo_prepare(const char *dir, const char *from);
char *nexttodo_commit(const char *dir);
char *nexttodo_file_name(const char *name);
void nexttodo_free(char *s);
