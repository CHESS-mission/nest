/*
 * fault_inject.c QEMU TCG Plugin: SEU (Single Event Upset) Fault Injection
 *
 * Injects bit-flip faults into guest virtual memory at a configurable
 * instruction-count offset. Because the plugin callback runs *inside* the
 * vCPU thread (not halting it), the guest clock never pauses so FPrime's
 * Svc::Health time-drift watchdog is not triggered. This is the key
 * difference from a GDB breakpoint approach.
 *
 * ── Requirements ────────────────────────────────────────────────────────────
 *   QEMU 8.1+  (plugin API v2, adds qemu_plugin_read_memory_vaddr)
 *   GLib 2.x   (headers bundled with QEMU)
 *
 * ── Memory write strategy ───────────────────────────────────────────────────
 *   qemu_plugin_write_memory_vaddr is NOT exported by QEMU 10.x.  The plugin
 *   therefore only performs the READ here; it records the injection as
 *   "needs_gdb_write" in last_result so that the Python controller can write
 *   the flipped byte via QEMU's GDB Remote Serial Protocol stub (port 1234).
 *   The GDB halt is <1 ms below FPrime's health watchdog threshold.
 *
 * ── Plugin arguments (comma-separated after the .so path) ───────────────────
 *   socket=<path>   Unix socket for runtime control     (REQUIRED)
 *   log=<path>      Append-mode log file                (optional)
 *
 *   Example QEMU invocation fragment:
 *     -plugin /path/to/fault_inject.so,socket=/tmp/fi.sock,log=/tmp/fi.log
 *
 * ── Control socket protocol (line-delimited JSON, one request per connect) ──
 *
 *   Schedule a memory bit-flip:
 *     -> {"cmd":"inject_mem","addr":<uint64>,"bit":<0-7>,"delay":<uint64>}
 *        addr  : guest virtual address (decimal or 0x-prefixed hex)
 *        bit   : bit index within the byte at addr (0 = LSB)
 *        delay : instruction count from *now* after which to inject
 *     ← {"status":"ok","trigger_insn":<uint64>}
 *
 *   Query current instruction count:
 *     -> {"cmd":"get_count"}
 *     ← {"status":"ok","insn_count":<uint64>,"pending":<bool>}
 *
 *   Cancel pending injection:
 *     -> {"cmd":"clear"}
 *     ← {"status":"ok"}
 *
 *   Get last injection result:
 *     -> {"cmd":"last_result"}
 *     ← {"status":"ok","injected":<bool>,"addr":<uint64>,"bit":<int>,
 *         "original":<uint8>,"flipped":<uint8>,"insn_count":<uint64>}
 *
 * ── Determinism note ────────────────────────────────────────────────────────
 *   For reproducible tests add  -icount shift=0,sleep=off  to QEMU's options.
 *   This makes the guest wall-clock advance by exactly 1 ns per instruction,
 *   decoupling it from host real-time. Combine with fixed I2C responses from
 *   the NEST simulation to get fully deterministic instruction-count triggers.
 */

#include <dlfcn.h>
#include <elf.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <glib.h>
#include <qemu-plugin.h>

/* ── QEMU internal write function, resolved at load time via ELF scan ───── */
/*
 * cpu_memory_rw_debug(cpu, addr, buf, len, is_write) is present in QEMU's
 * static symbol table (confirmed in QEMU 10.0.3) but NOT exported in the
 * dynamic symbol table, so dlsym() cannot find it.  We resolve it by:
 *   1. Mapping /proc/self/exe (the QEMU binary) and scanning .symtab.
 *   2. Adding the ASLR load bias (first LOAD-segment base from /proc/self/maps).
 *
 * When called from the vCPU thread (our plugin callback), the CPU's MMU
 * maps the CHESS process pages, so BOTH text and BSS addresses are writable.
 *
 * Signature (from QEMU include/exec/cpu-common.h):
 *   int cpu_memory_rw_debug(CPUState *cpu, vaddr addr,
 *                           void *buf, size_t len, bool is_write);
 * CPUState* is obtained via qemu_get_cpu(index).
 */
typedef int (*cpu_memory_rw_debug_fn)(void *cpu, uint64_t addr,
                                      void *buf, size_t len, bool is_write);
static cpu_memory_rw_debug_fn g_cpu_rw_debug = NULL;

/* qemu_get_cpu(index) -> CPUState* used to get CPU object from vCPU index */
typedef void *(*qemu_get_cpu_fn)(int index);
static qemu_get_cpu_fn g_qemu_get_cpu = NULL;

/*
 * find_qemu_sym scan the static ELF symbol table of /proc/self/exe and
 * return the runtime address of 'sym_name', or 0 on failure.
 */
static uintptr_t find_qemu_sym(const char *sym_name)
{
    /* ── Step 1: resolve /proc/self/exe ─────────────────────────────────── */
    char exe_path[512];
    ssize_t plen = readlink("/proc/self/exe", exe_path, sizeof(exe_path) - 1);
    if (plen <= 0) return 0;
    exe_path[plen] = '\0';

    /* ── Step 2: ASLR base from /proc/self/maps (first mapping, offset 0) ─ */
    uintptr_t load_base = 0;
    {
        FILE *f = fopen("/proc/self/maps", "r");
        if (!f) return 0;
        char line[512];
        while (fgets(line, sizeof(line), f)) {
            if (!strstr(line, exe_path)) continue;
            unsigned long long start = 0, end = 0, offset = 0;
            if (sscanf(line, "%llx-%llx %*s %llx", &start, &end, &offset) == 3
                    && offset == 0) {
                (void)end;
                load_base = (uintptr_t)start;
                break;
            }
        }
        fclose(f);
    }
    if (!load_base) return 0;

    /* ── Step 3: mmap the binary and scan .symtab ───────────────────────── */
    int fd = open(exe_path, O_RDONLY);
    if (fd < 0) return 0;

    struct stat st;
    if (fstat(fd, &st) != 0) { close(fd); return 0; }

    void *map = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    if (map == MAP_FAILED) return 0;

    uintptr_t result = 0;
    const Elf64_Ehdr *eh = (const Elf64_Ehdr *)map;

    /* Basic ELF sanity checks */
    if (st.st_size < (off_t)sizeof(Elf64_Ehdr)
            || memcmp(eh->e_ident, ELFMAG, SELFMAG) != 0
            || eh->e_ident[EI_CLASS] != ELFCLASS64
            || eh->e_shoff == 0
            || eh->e_shstrndx == SHN_UNDEF)
        goto out;

    {
        const Elf64_Shdr *shdrs =
            (const Elf64_Shdr *)((const char *)map + eh->e_shoff);
        const char *shstrtab =
            (const char *)map + shdrs[eh->e_shstrndx].sh_offset;

        const Elf64_Shdr *symtab_sh = NULL, *strtab_sh = NULL;
        for (uint16_t i = 0; i < eh->e_shnum; i++) {
            const char *sname = shstrtab + shdrs[i].sh_name;
            if (shdrs[i].sh_type == SHT_SYMTAB
                    && strcmp(sname, ".symtab") == 0)
                symtab_sh = &shdrs[i];
            else if (shdrs[i].sh_type == SHT_STRTAB
                    && strcmp(sname, ".strtab") == 0)
                strtab_sh = &shdrs[i];
        }
        if (!symtab_sh || !strtab_sh) goto out;

        const char     *strtab =
            (const char *)map + strtab_sh->sh_offset;
        const Elf64_Sym *syms =
            (const Elf64_Sym *)((const char *)map + symtab_sh->sh_offset);
        size_t nsyms = symtab_sh->sh_size / sizeof(Elf64_Sym);

        for (size_t i = 0; i < nsyms; i++) {
            if (syms[i].st_name == 0 || syms[i].st_value == 0) continue;
            if (strcmp(strtab + syms[i].st_name, sym_name) == 0) {
                result = load_base + (uintptr_t)syms[i].st_value;
                break;
            }
        }
    }
out:
    munmap(map, (size_t)st.st_size);
    return result;
}

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

/* ── Compile-time guard ─────────────────────────────────────────────────── */
#if QEMU_PLUGIN_VERSION < 2
#  error "fault_inject requires QEMU plugin API v2 (QEMU 8.1+). " \
         "Upgrade QEMU or see the README for the shm-fallback approach."
#endif

/* ── Types ──────────────────────────────────────────────────────────────── */

typedef struct {
    uint64_t trigger_insn;    /* absolute insn count at which to inject       */
    uint64_t target_addr;     /* guest virtual address (byte granularity)     */
    int      bit;             /* bit index 0-7 within the target byte         */
    bool     active;          /* an injection is scheduled                    */
    bool     done;            /* injection was performed                      */
    bool     deferred_logged; /* have we logged the first kernel-mode deferral */
    int      retry_count;     /* times we rescheduled due to wrong MMU ctx    */
} InjectionSpec;

/* Reschedule delay and maximum retries when we fire in the wrong process. */
#define RETRY_DELAY    100000ULL   /* instructions between context retries    */
#define MAX_RETRIES    200         /* ~20 s at 1 M insns/s, well within 30 s  */

typedef struct {
    bool     valid;        /* a result is available                        */
    uint64_t addr;
    int      bit;
    uint8_t  original;
    uint8_t  flipped;
    uint64_t insn_count;
    bool     success;      /* memory write succeeded                       */
} InjectionResult;

/* ── Global state ───────────────────────────────────────────────────────── */

static qemu_plugin_id_t  plugin_id;
static _Atomic uint64_t  insn_count  = 0;
static _Atomic bool      inj_pending = false; /* fast path flag            */

static pthread_mutex_t   inj_mutex   = PTHREAD_MUTEX_INITIALIZER;
static InjectionSpec     pending;
static InjectionResult   last_result;

static char   socket_path[256];
static char   log_path[256];
static FILE  *log_file = NULL;

/* ── Logging ────────────────────────────────────────────────────────────── */

static void plugin_log(const char *fmt, ...) G_GNUC_PRINTF(1, 2);
static void plugin_log(const char *fmt, ...)
{
    va_list ap;
    char    buf[512];

    va_start(ap, fmt);
    vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);

    fprintf(stderr, "[fault_inject] %s\n", buf);

    if (log_file) {
        fprintf(log_file, "%s\n", buf);
        fflush(log_file);
    }
}

/* ── Fault injection (called from within vCPU thread) ──────────────────── */

static void perform_injection(unsigned vcpu_idx, uint64_t addr, int bit)
{
    GByteArray *buf = g_byte_array_new();

    pthread_mutex_lock(&inj_mutex);
    last_result.valid    = false;
    last_result.addr     = addr;
    last_result.bit      = bit;
    last_result.insn_count = atomic_load(&insn_count);
    pthread_mutex_unlock(&inj_mutex);

    /* ── Read the target byte ─────────────────────────────────────────────
     * qemu_plugin_read_memory_vaddr can fail for user-space BSS pages
     * (physical page not yet faulted in / MMU context mismatch).
     * Fall back to cpu_memory_rw_debug which uses the vCPU's full MMU.   */
    uint8_t orig     = 0;
    bool    read_ok  = false;

    if (qemu_plugin_read_memory_vaddr(addr, buf, 1)) {
        orig     = buf->data[0];
        read_ok  = true;
    } else if (g_cpu_rw_debug && g_qemu_get_cpu) {
        void *cpu = g_qemu_get_cpu((int)vcpu_idx);
        int rrc = cpu ? g_cpu_rw_debug(cpu, addr, &orig, 1, false) : -99;
        if (rrc == 0)
            read_ok = true;
    }

    if (!read_ok) {
        pthread_mutex_lock(&inj_mutex);
        if (pending.retry_count < MAX_RETRIES) {
            plugin_log("INJECT ERROR: cannot read vaddr 0x%" PRIx64 ". Retrying...", addr);
            pending.retry_count++;
            pending.trigger_insn = atomic_load(&insn_count) + RETRY_DELAY;
            pending.done = false;
            pending.active = true;
            atomic_store(&inj_pending, true);
        } else {
            plugin_log("INJECT ERROR: max retries reached for vaddr 0x%" PRIx64, addr);
            last_result.valid   = true;
            last_result.success = false;
        }
        pthread_mutex_unlock(&inj_mutex);
        g_byte_array_free(buf, TRUE);
        return;
    }

    uint8_t flipped  = orig ^ (uint8_t)(1u << (bit & 7));
    bool    write_ok = false;

    /* Write the flipped byte via cpu_memory_rw_debug 
     * Runs in the vCPU thread with the CHESS process's MMU context active,
     * so both .text and .bss addresses are accessible no GDB halt needed. */
    if (g_cpu_rw_debug && g_qemu_get_cpu) {
        void *cpu = g_qemu_get_cpu((int)vcpu_idx);
        if (cpu) {
            int rc = g_cpu_rw_debug(cpu, addr, &flipped, 1, /*is_write=*/true);
            write_ok = (rc == 0);
        }
    }

    plugin_log("TRIGGER: vaddr=0x%" PRIx64
               " bit=%d orig=0x%02x flipped=0x%02x insn=%" PRIu64
               " write=%s",
               addr, bit, orig, flipped, atomic_load(&insn_count),
               write_ok ? "OK" : (g_cpu_rw_debug ? "FAIL" : "GDB-pending"));

    pthread_mutex_lock(&inj_mutex);
    last_result.valid    = true;
    last_result.original = orig;
    last_result.flipped  = flipped;
    last_result.success  = write_ok;
    pthread_mutex_unlock(&inj_mutex);

    g_byte_array_free(buf, TRUE);
}

/* Per-instruction callback.
 * userdata carries the instruction's virtual PC (cast to void*) so we can
 * check whether the guest CPU is in user space before firing the injection.
 * On ARM32, PC < 0xC0000000 = user space, PC >= 0xC0000000 = kernel.      */

static void vcpu_insn_exec(unsigned vcpu_index, void *userdata)
{
    uint64_t pc  = (uint64_t)(uintptr_t)userdata;
    uint64_t cnt = atomic_fetch_add(&insn_count, 1) + 1;

    /* Fast path: skip mutex if nothing is pending */
    if (!atomic_load_explicit(&inj_pending, memory_order_relaxed))
        return;

    pthread_mutex_lock(&inj_mutex);

    if (pending.active && !pending.done && cnt >= pending.trigger_insn) {
        /* We must inject when the target process's MMU context is active.
         * Since CHESS_Pathfinder0FS is compiled without PIE, its .text segment
         * is statically located between 0x00011E00 and 0x000E7134.
         * Waiting until the PC is inside this range guarantees that the active
         * page tables belong to FPrime and the target addresses are mapped. */
        if (pc < 0x00011E00ULL || pc > 0x000E7134ULL) {
            if (!pending.deferred_logged) {
                plugin_log("DEFER: PC=0x%" PRIx64
                           ", waiting for CHESS user context", pc);
                pending.deferred_logged = true;
            }
            pthread_mutex_unlock(&inj_mutex);
            return;
        }

        uint64_t addr = pending.target_addr;
        int      bit  = pending.bit;

        pending.done   = true;
        pending.active = false;
        atomic_store(&inj_pending, false);

        if (pending.deferred_logged)
            plugin_log("DEFERRED injection now firing at user PC=0x%" PRIx64, pc);

        pthread_mutex_unlock(&inj_mutex);

        perform_injection(vcpu_index, addr, bit);
        return;
    }

    pthread_mutex_unlock(&inj_mutex);
}

/* Translation-block callback */

static void vcpu_tb_trans(qemu_plugin_id_t id, struct qemu_plugin_tb *tb)
{
    (void)id;
    size_t n = qemu_plugin_tb_n_insns(tb);

    for (size_t i = 0; i < n; i++) {
        struct qemu_plugin_insn *insn = qemu_plugin_tb_get_insn(tb, i);
        uint64_t vaddr = qemu_plugin_insn_vaddr(insn);
        qemu_plugin_register_vcpu_insn_exec_cb(
            insn,
            vcpu_insn_exec,
            QEMU_PLUGIN_CB_NO_REGS,
            (void *)(uintptr_t)vaddr);
    }
}

/* Control socket: minimal JSON helpers */
/* We avoid a JSON library dependency; our Python client always sends
 * well-formed, compact single-line JSON, so simple string scanning suffices. */

#define MAX_MSG 512

static bool json_u64(const char *json, const char *key, uint64_t *out)
{
    char needle[80];
    snprintf(needle, sizeof(needle), "\"%s\":", key);
    const char *p = strstr(json, needle);
    if (!p) return false;
    p += strlen(needle);
    while (*p == ' ') p++;
    char *end;
    errno = 0;
    uint64_t v = strtoull(p, &end, 0); /* base 0 -> auto hex/dec */
    if (errno || end == p) return false;
    *out = v;
    return true;
}

static bool json_str(const char *json, const char *key, char *out, size_t sz)
{
    char needle[80];
    snprintf(needle, sizeof(needle), "\"%s\":\"", key);
    const char *p = strstr(json, needle);
    if (!p) return false;
    p += strlen(needle);
    size_t i = 0;
    while (*p && *p != '"' && i < sz - 1) out[i++] = *p++;
    out[i] = '\0';
    return (i > 0);
}

/* Control socket: request handler */

static void handle_client(int fd)
{
    char buf[MAX_MSG];
    ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
    if (n <= 0) return;
    buf[n] = '\0';
    // strip trailing newline
    for (ssize_t i = 0; i < n; i++) {
        if (buf[i] == '\n' || buf[i] == '\r') { buf[i] = '\0'; break; }
    }

    char cmd[64] = "";
    json_str(buf, "cmd", cmd, sizeof(cmd));

    char resp[512];

    // get_count
    if (strcmp(cmd, "get_count") == 0) {
        pthread_mutex_lock(&inj_mutex);
        bool pen = pending.active;
        pthread_mutex_unlock(&inj_mutex);
        snprintf(resp, sizeof(resp),
                 "{\"status\":\"ok\",\"insn_count\":%" PRIu64 ",\"pending\":%s}\n",
                 atomic_load(&insn_count),
                 pen ? "true" : "false");

    // inject_mem
    } else if (strcmp(cmd, "inject_mem") == 0) {
        uint64_t addr = 0, delay = 0, bit_u = 0;
        bool ok = json_u64(buf, "addr",  &addr)
               && json_u64(buf, "bit",   &bit_u)
               && json_u64(buf, "delay", &delay);

        if (!ok || bit_u > 7) {
            snprintf(resp, sizeof(resp),
                     "{\"status\":\"error\",\"msg\":\"need addr, bit(0-7), delay\"}\n");
        } else {
            uint64_t trigger = atomic_load(&insn_count) + delay;
            pthread_mutex_lock(&inj_mutex);
            pending.trigger_insn    = trigger;
            pending.target_addr     = addr;
            pending.bit             = (int)bit_u;
            pending.active          = true;
            pending.done            = false;
            pending.deferred_logged = false;
            pending.retry_count     = 0;
            pthread_mutex_unlock(&inj_mutex);
            atomic_store(&inj_pending, true);

            plugin_log("Scheduled: addr=0x%" PRIx64 " bit=%d trigger=%" PRIu64,
                       addr, (int)bit_u, trigger);
            snprintf(resp, sizeof(resp),
                     "{\"status\":\"ok\",\"trigger_insn\":%" PRIu64 "}\n",
                     trigger);
        }

    // clear
    } else if (strcmp(cmd, "clear") == 0) {
        pthread_mutex_lock(&inj_mutex);
        pending.active = false;
        pthread_mutex_unlock(&inj_mutex);
        atomic_store(&inj_pending, false);
        snprintf(resp, sizeof(resp), "{\"status\":\"ok\"}\n");

    // last_result
    } else if (strcmp(cmd, "last_result") == 0) {
        pthread_mutex_lock(&inj_mutex);
        InjectionResult r = last_result;
        pthread_mutex_unlock(&inj_mutex);

        if (!r.valid) {
            snprintf(resp, sizeof(resp),
                     "{\"status\":\"ok\",\"injected\":false}\n");
        } else {
            snprintf(resp, sizeof(resp),
                     "{\"status\":\"ok\",\"injected\":true,"
                     "\"success\":%s,\"addr\":%" PRIu64 ",\"bit\":%d,"
                     "\"original\":%u,\"flipped\":%u,\"insn_count\":%" PRIu64 "}\n",
                     r.success ? "true" : "false",
                     r.addr, r.bit,
                     (unsigned)r.original, (unsigned)r.flipped,
                     r.insn_count);
        }

    // unknown command
    } else {
        snprintf(resp, sizeof(resp),
                 "{\"status\":\"error\",\"msg\":\"unknown cmd '%s'\"}\n", cmd);
    }

    send(fd, resp, strlen(resp), MSG_NOSIGNAL);
}

/* Control socket server thread */

static void *socket_thread(void *arg)
{
    (void)arg;

    int server_fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (server_fd < 0) {
        plugin_log("socket() failed: %s", strerror(errno));
        return NULL;
    }

    unlink(socket_path);

    struct sockaddr_un sa;
    memset(&sa, 0, sizeof(sa));
    sa.sun_family = AF_UNIX;
    memcpy(sa.sun_path, socket_path,
           strnlen(socket_path, sizeof(sa.sun_path) - 1) + 1);

    if (bind(server_fd, (struct sockaddr *)&sa, sizeof(sa)) < 0 ||
        listen(server_fd, 8) < 0) {
        plugin_log("bind/listen failed: %s", strerror(errno));
        close(server_fd);
        return NULL;
    }

    plugin_log("Control socket ready: %s", socket_path);

    while (1) {
        int cfd = accept(server_fd, NULL, NULL);
        if (cfd < 0) continue;
        handle_client(cfd);
        close(cfd);
    }

    return NULL; // unreachable
}

/* Cleanup callback */

static void plugin_exit(qemu_plugin_id_t id, void *p)
{
    (void)id; (void)p;
    plugin_log("Unloading. Total instructions executed: %" PRIu64,
               atomic_load(&insn_count));
    unlink(socket_path);
    if (log_file) fclose(log_file);
}

/* Plugin entry point */

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id,
                                            const qemu_info_t *info,
                                            int argc, char **argv)
{
    plugin_id = id;

    // Parse plugin arguments
    for (int i = 0; i < argc; i++) {
        if (strncmp(argv[i], "socket=", 7) == 0)
            strncpy(socket_path, argv[i] + 7, sizeof(socket_path) - 1);
        else if (strncmp(argv[i], "log=", 4) == 0)
            strncpy(log_path, argv[i] + 4, sizeof(log_path) - 1);
        else
            fprintf(stderr, "[fault_inject] WARNING: unknown arg: %s\n", argv[i]);
    }

    if (socket_path[0] == '\0') {
        fprintf(stderr, "[fault_inject] ERROR: 'socket=<path>' is required\n");
        return -1;
    }

    if (log_path[0] != '\0') {
        log_file = fopen(log_path, "a");
        if (!log_file)
            fprintf(stderr, "[fault_inject] WARNING: cannot open log '%s': %s\n",
                    log_path, strerror(errno));
    }

    /* Resolve QEMU-internal write helper via ELF static symbol scan.
     * dlsym() cannot find these because they are in .symtab (not .dynsym).
     * find_qemu_sym() maps /proc/self/exe, scans .symtab, and adds the
     * ASLR load bias from /proc/self/maps.                               */
    g_cpu_rw_debug = (cpu_memory_rw_debug_fn)
                     find_qemu_sym("cpu_memory_rw_debug");
    g_qemu_get_cpu = (qemu_get_cpu_fn)
                     find_qemu_sym("qemu_get_cpu");
    if (!g_cpu_rw_debug || !g_qemu_get_cpu) {
        fprintf(stderr,
                "[fault_inject] WARNING: cpu_memory_rw_debug=0x%" PRIxPTR
                " qemu_get_cpu=0x%" PRIxPTR
                " direct write unavailable, will fall back to GDB\n",
                (uintptr_t)g_cpu_rw_debug, (uintptr_t)g_qemu_get_cpu);
    }

    plugin_log("Loaded | QEMU plugin API v%d | target=%s | socket=%s"
               " | direct_write=%s",
               info->version.cur, info->target_name, socket_path,
               (g_cpu_rw_debug && g_qemu_get_cpu) ? "YES" : "NO");

    // Warn if not running under TCG (plugins are TCG-only)
    if (!info->system_emulation)
        plugin_log("WARNING: not in system-emulation mode unexpected");

    // Start Unix socket server thread
    pthread_t t;
    if (pthread_create(&t, NULL, socket_thread, NULL) != 0) {
        plugin_log("pthread_create failed: %s", strerror(errno));
        return -1;
    }
    pthread_detach(t);

    // Register TB translation hook (instruments every instruction)
    qemu_plugin_register_vcpu_tb_trans_cb(id, vcpu_tb_trans);

    // Register cleanup
    qemu_plugin_register_atexit_cb(id, plugin_exit, NULL);

    return 0;
}
