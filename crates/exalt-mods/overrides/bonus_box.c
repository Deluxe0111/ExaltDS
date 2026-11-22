/* SpotPass bonuses on by default, for Fire Emblem Awakening USA (00040000000A0500) rev0.
 * See SPOTPASS_PLAN.md in Awakening-Port.
 *
 * Wireless -> Bonus Box lists what SpotPass delivered: bonus teams, items, maps and Double Duel
 * challengers. Everything they offer is on the cartridge already (data/download.bin holds the
 * teams, data/BossData.bin the four lists, the maps are X018-X023); a delivery only set bits in
 * two bitmaps the save keeps. Nintendo's servers are gone, so this marks every entry of the four
 * lists as delivered the way a delivery does, each time the game loads those lists, while the
 * sidebar's "Unlock bonus box content" is on (the default). What a save has received stays in it
 * when the switch goes off.
 *
 * How (USA rev0, from the disassembly, 2026-11-21):
 *   0x002EE550  loads data/BossData.bin (the four lists: 0 teams, 1 items, 2 challengers,
 *               3 maps), from the Bonus Box opening (0x001E1800), the game's start (0x002EE3E4)
 *               and a delivery (0x0015CB5C, 0x0015CD40). Overridden here: the game's own, then
 *               every entry delivered. (0x00392C20, the Bonus Box items' "anything new?", is no
 *               place for it: it only runs to refresh them after a SpotPass update, 0x00392B18.)
 *   0x00391754  the bonus team list (one of the lists' builders): shows the teams whose
 *               received bit is set.
 *   0x004D5E10  the game object (a pointer); per list, 36 bytes at +0x9B68 + list * 36 hold the
 *               received bits, at +0x9BF8 + list * 36 the not yet claimed bits: 32 bytes of bits
 *               by entry id, then a CRC32 of them at +0x20.
 *   0x002EE4D8  (list) -> the list's entries in BossData, 12 bytes each: u16 id, u16 type,
 *               u32 name, u16 / u16 delivery week
 *   0x002EE514  (list) -> how many
 *   0x0018B7D4  (bitmap) clears the bits when their CRC is wrong
 *   0x002EE3B4  (bitmap) writes the CRC
 *   0x0015CB48  a delivery (game, list, hours): for each entry it delivers whose received bit is
 *               clear, sets it and the not yet claimed bit. On the cartridge the teams, items and
 *               maps are type 0 (never delivered) and 10 of the 22 challengers type 2, which the
 *               game delivers itself on every start (0x002EE3DC).
 */
#include "overrides.h"

#define BB_GAME 0x004D5E10u
#define BB_RECEIVED 0x9B68u
#define BB_UNCLAIMED 0x9BF8u
#define BB_SLICE 36u
#define BB_BITS 256u
#define BB_LISTS 4u
#define BB_ENTRY 12u
#define BB_ENTRIES 0x002EE4D8u
#define BB_COUNT 0x002EE514u
#define BB_CHECK 0x0018B7D4u
#define BB_SEAL 0x002EE3B4u

#if defined(__ANDROID__)
#include <dlfcn.h>
#include <stdarg.h>
#include <stdio.h>
/* logcat, tag BonusBox */
static void bb_log(const char *format, ...) {
    static int (*print)(int, const char *, const char *, ...);
    static int tried;
    if (!tried) {
        tried = 1;
        void *log = dlopen("liblog.so", RTLD_NOW);
        if (log) print = (int (*)(int, const char *, const char *, ...))dlsym(log, "__android_log_print");
    }
    if (!print) return;
    char line[256];
    va_list args;
    va_start(args, format);
    vsnprintf(line, sizeof line, format, args);
    va_end(args);
    print(4 /* ANDROID_LOG_INFO */, "BonusBox", "%s", line);
}
/* the sidebar's "Unlock bonus box content" (libzakuro_android's zakuro_bonus_box); on when the
   runtime has no such switch */
static int bb_wanted(void) {
    static uint32_t (*wanted)(void);
    static int tried;
    if (!tried) {
        tried = 1;
        void *library = dlopen("libzakuro_android.so", RTLD_NOW | RTLD_NOLOAD);
        if (library) wanted = (uint32_t (*)(void))dlsym(library, "zakuro_bonus_box");
    }
    return wanted ? wanted() != 0 : 1;
}
#else
#define bb_log(...) do { } while (0)
static int bb_wanted(void) { return 1; }
#endif

/* a guest call to one of the game's helpers above, none of which waits on the host; should the
   host stop inside one anyway, the guest goes on at back */
#define BB_GUEST_CALL(function, argument, back) do { \
    ctx->r[0] = (argument); \
    ctx->r[14] = (back); \
    ctx->r[15] = (function); \
    ctx->thumb = 0; \
    CALL(recomp_call); \
} while (0)

static int bb_has(Context *ctx, uint32_t bits, uint32_t id) {
    return (mem_read8(ctx, bits + (id >> 3)) >> (id & 7)) & 1;
}

static void bb_set(Context *ctx, uint32_t bits, uint32_t id) {
    mem_write8(ctx, bits + (id >> 3), (uint8_t)(mem_read8(ctx, bits + (id >> 3)) | (1u << (id & 7))));
}

/* every entry of the four lists delivered, as a delivery does: bits with a wrong CRC go first,
   then each entry not yet received becomes received and not yet claimed, then the CRCs. Entries
   already received stay as they are, so a claimed item is not offered again. */
static uint32_t bb_deliver_all(Context *ctx, uint32_t link) {
    uint32_t game = mem_read32(ctx, BB_GAME);
    if (!game) return 0;
    for (uint32_t list = 0; list < BB_LISTS; list++) {
        BB_GUEST_CALL(BB_ENTRIES, list, link);
        uint32_t entries = ctx->r[0];
        BB_GUEST_CALL(BB_COUNT, list, link);
        uint32_t count = ctx->r[0];
        if (!entries || count > BB_BITS) {
            bb_log("list %u: no entries (%08X, %u)", (unsigned)list, entries, (unsigned)count);
            continue;
        }
        uint32_t received = game + BB_RECEIVED + list * BB_SLICE;
        uint32_t unclaimed = game + BB_UNCLAIMED + list * BB_SLICE;
        uint32_t missing = 0;
        for (uint32_t i = 0; i < count; i++) {
            uint32_t id = mem_read16(ctx, entries + i * BB_ENTRY);
            if (id < BB_BITS && !bb_has(ctx, received, id)) missing++;
        }
        if (!missing) continue;
        BB_GUEST_CALL(BB_CHECK, received, link);
        BB_GUEST_CALL(BB_CHECK, unclaimed, link);
        uint32_t delivered = 0;
        for (uint32_t i = 0; i < count; i++) {
            uint32_t id = mem_read16(ctx, entries + i * BB_ENTRY);
            if (id >= BB_BITS || bb_has(ctx, received, id)) continue;
            bb_set(ctx, received, id);
            bb_set(ctx, unclaimed, id);
            delivered++;
        }
        BB_GUEST_CALL(BB_SEAL, received, link);
        BB_GUEST_CALL(BB_SEAL, unclaimed, link);
        bb_log("list %u: %u of %u entries delivered", (unsigned)list, (unsigned)delivered, (unsigned)count);
    }
    return 1;
}

RECOMP_OVERRIDE(0x002EE550) {
    uint32_t link = ctx->r[14];
    CALL(RECOMP_ORIGINAL(0x002EE550));
    if (!bb_wanted()) RETURN_TO(link);
    if (UNLIKELY(bb_deliver_all(ctx, link) == RECOMP_STOPPED)) return RECOMP_STOPPED;
    RETURN_TO(link);
}
