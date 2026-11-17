/* Recall Fate in the map menu, for Fire Emblem Awakening USA (00040000000A0500) rev0.
 * See NAGA_TEAR_PLAN.md (§2, §9b; NT-06) in Awakening-Port.
 *
 * Pressing A on an empty tile opens the map menu (Units, Guide, Options, Save/Suspend, auto
 * battle, End). Once the campaign has Naga's Tear (save flag G_ナーガの涙), a "Recall Fate" entry
 * sits at the top. Choosing it opens a list drawn by the game's own map dialog (the one End uses
 * for "End your turn?"), newest first: "Turn N" for each turn's start and "<unit> - <action>" for
 * each player action. Choosing a row asks the runtime to go back there (libzakuro_android's
 * recall.rs keeps a whole-console checkpoint at each player phase start and after each action);
 * it does so between frames.
 *
 * How (USA rev0, from the disassembly, 2026-11-16):
 *   0x00175E48  the map menu builder: seven 0x38-byte items from the allocator into a
 *               NULL-terminated list on its stack (0x8C-byte frame, list at sp+8), then
 *   0x0030071C  (menu, list): counts the list and copies it into the menu (+0x38 array, +0x3C
 *               count). Overridden here: for the map menu's list (Units first, End seventh) one
 *               more item goes in first, the list moving down a word in the builder's frame
 *               (sp+0x28 free).
 *   0x0031521C  allocator (r0 size, default heap)
 *   Item vtable slots: 0 state (1 shown, 2 greyed, 4 hidden), 2 row height (0x20), 4 name (UTF-16
 *   text), 5 help text, 12 select (0x400 refused, 0x100 stay, 0x108 done), 21 delete, 22 icon
 *   (index into menu.tga of systems/icon_E.ctpk, 32x32 cells, 8 a row: Units 0x18, Guide 0x1B,
 *   Options 8, Save 9, Suspend 0x19, End 0x1A, auto battle 0x33; no cell is free: the bottom halves of 0x34-0x37 hold the 24x24
 *   portraits of indices 100+, so the clock gets its own index, RF_CLOCK_ICON), 23 the owning
 *   menu, 27 called as the menu opens. A map menu item's vtable runs to slot 32.
 *   Item fields: +0 vtable, +4/+8 0, +0xC the owning menu, +0x30 flags (2 greyed), +0x34 0xFF.
 *   The map menu entry is Guide's class (mapSystemMenu::TutorialList, vtable 0x49D6A8: slots 0
 *   0x1C92CC, 4 0x1C9360, 5 0x1C9340, 12 0x1C9310); the turn rows are End's dialog's Yes class
 *   (vtable 0x49D5E0: 4 0x1F8384, 12 0x1C925C, 13 0x1F837C (B)), made for a dialog owner. (Map menu items cast
 *   their owner to SeparateMenu, which the dialog is not: rows of Guide's class crashed at
 *   0x1EDD44 on the Thor.)
 *   Each item made here carries a copy of its class's vtable (with the type-info word before it)
 *   in its own allocation and points at that copy, so the overrides of those slots know it by its
 *   vtable lying inside it; the game's own items point at the fixed vtables. After the item's 0x38
 *   bytes: name text, help text, the row's turn, the vtable copy.
 *   End's turn dialog (0x1CA714): owner = slot 23; menu = alloc(0x12C), 0x313EDC(menu, 0x12C),
 *   0x2E7C68(menu, -1), +0x78 = 1, +0x4C |= 0x10, vtable 0x48FCDC; 0x3124C4(menu, 0x5078F8,
 *   "MapDialogYesNo" at 0x1CA898, 0, [sp] owner); 0x30071C(that, list); [[+0x2C]+0x44]++;
 *   +0x4C |= 0x4000000; +0x110 = -4.0f; 0x2E7C08(that, title text); returns 0x100.
 *   (Rows of 0x1C instead of 0x20, to fit eight, hid the last row and set the icons too low; the
 *   game's height is kept.)
 *   Global flags (ev::FlagGet 0x3B6A94 -> 0x2F663C): table [[0x4D5E2C]+0x2C] { +4 names, +8 count,
 *   +0xC bits }, flag i is bit (i & 7) of byte i >> 3.
 */
#include <stdio.h>
#include <string.h>
#include "overrides.h"

#define RF_MENU_SET_ITEMS 0x0030071Cu
#define RF_ALLOC 0x0031521Cu
#define RF_UNITS_VTABLE 0x0049DC70u
#define RF_END_VTABLE 0x0049DBE4u
#define RF_GUIDE_VTABLE 0x0049D6A8u
#define RF_YES_VTABLE 0x0049D5E0u
#define RF_MENU_INIT 0x00313EDCu
#define RF_MENU_CTOR 0x002E7C68u
#define RF_DIALOG_VTABLE 0x0048FCDCu
#define RF_PROC_START 0x003124C4u
#define RF_DIALOG_KIND 0x005078F8u
#define RF_DIALOG_NAME 0x001CA898u /* "MapDialogYesNo" */
#define RF_SET_TITLE 0x002E7C08u
#define RF_SCRIPT_GLOBALS 0x004D5E2Cu
#define RF_MAP_MENU_VTABLE 0x0049D734u
#define RF_MENU_LAYOUT 0x003D8460u /* menu slot 9: places the visible rows (+0x30 array, +0x34
                                      count) in a grid, +0x68 rows a column, +0x6C columns; slot 10
                                      sets each row's place. The window shown is +0x70 rows by +0x74
                                      columns; slots 6/7 (0x3D7CE4, 0x3D7BE8) size the box from it
                                      into +0x60/+0x64 while those are negative, and slot 8
                                      (0x3D8070) centres a box whose +0x58/+0x5C is negative */
#define RF_UNIT_NAME 0x002F19D0u /* (unit) -> its name, UTF-16: its own (+0x74, the avatar's) or its
                                    person's (0x2C6904: the person's label, +0x14, as text) */
#define RF_UNITS 0x004D1100u     /* the units: unit i (from 1) at [RF_UNITS] + (i - 1) * 0x110 */
#define RF_UNIT_SIZE 0x110u
#define RF_DIALOG_LAYOUT 0x0015C5C0u /* the dialog's slot 9: lays its rows out (grid as the map menu's) */
#define RF_GUIDE_ICON 0x001C9358u  /* Guide's slot 22: returns 0x1B */
#define RF_DRAW_ICON 0x002BF4D4u   /* (sheet, index, colour; s0-s4 place and size) */
#define RF_DRAW_BEGIN 0x002F50E4u  /* what 0x2BF4D4 calls before its quad, in this order */
#define RF_DRAW_STATE 0x002F501Cu
#define RF_DRAW_TEXTURE 0x002F4F9Cu /* r0 = the sheet's texture, [sheet + 0x18] */
#define RF_DRAW_MODE 0x002F4F4Cu
#define RF_DRAW_QUAD 0x002E23B8u   /* r0 colour; s0-s4 as given, s5/s6 the texel the cell starts at */
#define RF_DRAW_END 0x002F4E70u
/* the clock: an index no game icon uses, drawn from the cell tools/naga-tear adds to menu.tga
   (256x512 in the layer, the clock at x 0, y 256; icons.rs) */
#define RF_CLOCK_ICON 0x7C10u
#define RF_CLOCK_X 0.0f
#define RF_CLOCK_Y 256.0f

#define RF_NAME 0x38u        /* UTF-16, 31 characters */
#define RF_HELP 0x78u        /* UTF-16, 43 characters */
#define RF_TURN 0xD0u        /* u32, a row's history entry id */
#define RF_UNIT_ROW 0xD4u    /* u32, the acting unit (guest) for an action row, else 0: its sprite is drawn */
#define RF_ACTION_ROW 0xD8u  /* u32, 1 for an action row, 0 for a turn's start */
#define RF_SHIFT 0xDCu       /* s32, how far a turn row's +0x14 is moved while it is drawn (0 otherwise) */
#define RF_SPRITE_DESC 0xE0u /* 0x10 bytes, the sprite description (heap: no stack below sp in use) */
#define RF_SPRITE_COLOUR 0xF0u /* u32, white */
#define RF_VTABLE_COPY 0xF4u /* type-info word, then the vtable */
/* the type-info word and slots 0-32: a map menu item's vtable runs that far (the menu calls slot
   27, +0x6C, as it opens; a shorter copy crashed there), the Yes class's is shorter */
#define RF_VTABLE_BYTES 0x88u
#define RF_TITLE (RF_VTABLE_COPY + RF_VTABLE_BYTES) /* UTF-16, the list's title "Recall Fate (N left)" (the
                                                       map menu entry, which outlives the list, keeps it) */
#define RF_TITLE_BYTES 0x40u
#define RF_ALLOCATION (RF_TITLE + RF_TITLE_BYTES)
#define RF_MAX_ROWS 24
#define RF_NAME_CHARS 31
/* the list sits on the left, as tall as the screen allows (rows beyond that scroll), its rows
   RF_ROW_WIDTH wide (the Yes class's slot 1, 0x1CE2C0, gives 80) with their text from the left */
#define RF_LIST_X 8
#define RF_LIST_TOP 8
#define RF_LIST_BOTTOM 232
#define RF_ROW_WIDTH 168
#define RF_CENTRED 0x10u /* dialog +0x4C: the Yes rows' draw (0x1CE024) centres their text */
/* an action row shows its unit's map sprite (as the game's unit list does) between the indent and
   the text: turn rows RF_TURN_HEIGHT tall and action rows RF_ACTION_HEIGHT (the Yes class's slot 2,
   0x192730, gives 20; the layout stacks each row's own height), action text after RF_INDENT spaces,
   the sprite RF_SPRITE_X from the row's left and clipped to the row, so its top half shows */
#define RF_TURN_HEIGHT 28
#define RF_ACTION_HEIGHT 22
#define RF_DRAW_CLIP 0x002EE944u /* (x, y, width, height): where drawing lands (0x1CE024 sets 0,0,400,240) */
#define RF_INDENT 6
#define RF_SPRITE_X (-8) /* the box starts 8 px left of the row (0x1CE024) */
#define RF_SPRITE_Y (-5)
#define RF_SPRITE_DESCRIBE 0x002C785Cu /* (description, unit): the unit's sprite (class, army, name) */
#define RF_SPRITE_DRAW 0x002E2150u     /* (manager, description, 0, 0; s0 x, s1 y, s2 0; [sp] colour *) */
#define RF_SPRITE_MANAGER 0x004D56ACu  /* the unit sprite manager, as the unit list's 0x2E0DA8 passes it */
#define RF_ROW_DRAW 0x001CE024u
/* a return point (after the bl at 0x2EE960) whose next instruction is "pop {r3, pc}": a guest call
   made with sp lowered by 8 ([sp] its stack argument, [sp + 4] where to go) gives this as its
   return, so that if the host stops inside it the guest goes on from here, sp back where it was */
#define RF_POP_R3_PC 0x002EE964u

#define RF_SHOWN 1u
#define RF_GREYED 2u
#define RF_REFUSED 0x400u
#define RF_STAY 0x100u
#define RF_BACK 0x201u /* what map menu items return for B (0x192704) */

/* "G_ナーガの涙" in Shift-JIS, as tools/naga-tear registers it */
static const char rf_flag[] = "G_\x83\x69\x81\x5B\x83\x4B\x82\xCC\x97\xDC";

/* ---- the runtime (libzakuro_android, recall.rs) ---- */

#if defined(__ANDROID__)
#include <dlfcn.h>
#include <stdarg.h>
/* logcat, tag RecallFate */
static void rf_log(const char *format, ...) {
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
    print(4, "RecallFate", "%s", line);
}
static void *rf_runtime(const char *name) {
    static void *library;
    static int tried;
    if (!tried) {
        tried = 1;
        library = dlopen("libzakuro_android.so", RTLD_NOW | RTLD_NOLOAD);
    }
    return library ? dlsym(library, name) : NULL;
}
/* recall.rs's Entry: one line of the history, newest first */
typedef struct {
    uint32_t id;     /* what zakuro_recall_go takes */
    uint16_t turn;
    uint16_t unit;   /* the acting unit's index, 0 for a turn's start */
    uint8_t kind;    /* 0 turn start, 1 attack, 2 heal, 3 move, 4 wait, 5 dance, 6 staff, 7 item,
                        8 pair up, 9 separate, 10 rally, 11 talk, 12 visit (RF_KIND_NAMES) */
    uint8_t pad[3];
    uint32_t person; /* the acting unit's person (guest), 0 for a turn's start */
} RfEntry;
static size_t rf_entries(RfEntry *out, size_t length) {
    static size_t (*entries)(RfEntry *, size_t);
    if (!entries) entries = (size_t (*)(RfEntry *, size_t))rf_runtime("zakuro_recall_entries");
    return entries ? entries(out, length) : 0;
}
static void rf_go(uint32_t id) {
    static void (*go)(uint32_t);
    if (!go) go = (void (*)(uint32_t))rf_runtime("zakuro_recall_go");
    if (go) go(id);
}
/* the unit command just chosen (an entry kind), which names the action once it ends */
static void rf_command(uint8_t kind) {
    static void (*command)(uint8_t);
    if (!command) command = (void (*)(uint8_t))rf_runtime("zakuro_recall_command");
    if (command) command(kind);
}
/* Naga's Tear's uses left in this battle (recall.rs: 10 a battle, one per rewind) */
static uint32_t rf_uses(void) {
    static uint32_t (*uses)(void);
    if (!uses) uses = (uint32_t (*)(void))rf_runtime("zakuro_recall_uses");
    return uses ? uses() : 10;
}
#else
#define rf_log(...) do { } while (0)
static uint32_t rf_uses(void) { return 10; }
typedef struct { uint32_t id; uint16_t turn, unit; uint8_t kind, pad[3]; uint32_t person; } RfEntry;
static size_t rf_entries(RfEntry *out, size_t length) { (void)out; (void)length; return 0; }
static void rf_go(uint32_t id) { (void)id; }
static void rf_command(uint8_t kind) { (void)kind; }
#endif

/* the history list being shown (its layout is one column, RF_DIALOG_LAYOUT), and the map menu it
   was opened from, which is not drawn while the list is open (the list stands alone over the map,
   as Engage's Time Crystal list does); 0 once the list closes */
static uint32_t rf_list;
static uint32_t rf_owner;

/* ---- guest helpers ---- */

static void rf_write_text(Context *ctx, uint32_t address, const char *text, uint32_t bytes) {
    uint32_t i = 0;
    for (; text[i] && 2 * (i + 1) < bytes; i++) mem_write16(ctx, address + 2 * i, (uint8_t)text[i]);
    mem_write16(ctx, address + 2 * i, 0);
}

/* an item of the class whose vtable is given, pointing at its own copy of that vtable */
static void rf_init_item(Context *ctx, uint32_t item, uint32_t vtable) {
    for (uint32_t i = 0; i < RF_VTABLE_BYTES; i += 4)
        mem_write32(ctx, item + RF_VTABLE_COPY + i, mem_read32(ctx, vtable - 4 + i));
    mem_write32(ctx, item, item + RF_VTABLE_COPY + 4);
    mem_write32(ctx, item + 4, 0);
    mem_write32(ctx, item + 8, 0);
    mem_write32(ctx, item + 0x30, 0);
    mem_write8(ctx, item + 0x34, 0xFF);
    mem_write32(ctx, item + RF_TURN, 0);
    mem_write32(ctx, item + RF_UNIT_ROW, 0);
    mem_write32(ctx, item + RF_ACTION_ROW, 0);
    mem_write32(ctx, item + RF_SHIFT, 0);
}

/* whether this is an item made here */
static int rf_mine(Context *ctx, uint32_t self) {
    return self && mem_read32(ctx, self) == self + RF_VTABLE_COPY + 4;
}

static int rf_guest_equals(Context *ctx, uint32_t address, const char *text) {
    for (uint32_t i = 0;; i++) {
        uint8_t c = mem_read8(ctx, address + i);
        if (c != (uint8_t)text[i]) return 0;
        if (!c) return 1;
    }
}

/* whether this campaign has Naga's Tear: the save flag, read as ev::FlagGet does */
static int rf_has_naga_tear(Context *ctx) {
    uint32_t globals = mem_read32(ctx, RF_SCRIPT_GLOBALS);
    uint32_t table = globals ? mem_read32(ctx, globals + 0x2C) : 0;
    if (!table) return 0;
    uint32_t names = mem_read32(ctx, table + 4), count = mem_read32(ctx, table + 8), bits = mem_read32(ctx, table + 0xC);
    if (!names || !bits || count > 4096) return 0;
    for (uint32_t i = 0; i < count; i++) {
        uint32_t name = mem_read32(ctx, names + 4 * i);
        if (name && rf_guest_equals(ctx, name, rf_flag)) return (mem_read8(ctx, bits + (i >> 3)) >> (i & 7)) & 1;
    }
    return 0;
}

/* a guest call from here: the host may stop inside it (its work then finishes, returning to the
   given address); otherwise this goes on after it with the result in r0 */
#define RF_GUEST_CALL(function, back) do { \
    ctx->r[14] = (back); \
    ctx->r[15] = (function); \
    ctx->thumb = 0; \
    CALL(recomp_call); \
} while (0)

/* ---- the map menu: one more item before End ---- */

RECOMP_OVERRIDE(0x0030071C) {
    uint32_t menu = ctx->r[0], list = ctx->r[1], link = ctx->r[14];
    uint32_t first = list ? mem_read32(ctx, list) : 0, seventh = list ? mem_read32(ctx, list + 24) : 0;
    if (LIKELY(!first || mem_read32(ctx, first) != RF_UNITS_VTABLE || !seventh || mem_read32(ctx, seventh) != RF_END_VTABLE
               || mem_read32(ctx, list + 28) != 0 || !rf_has_naga_tear(ctx)))
        TAIL_CALL(RECOMP_ORIGINAL(0x0030071C));

    uint32_t r2 = ctx->r[2], r3 = ctx->r[3], r12 = ctx->r[12];
    ctx->r[0] = RF_ALLOCATION;
    RF_GUEST_CALL(RF_ALLOC, link);
    uint32_t item = ctx->r[0];
    if (item) {
        rf_init_item(ctx, item, RF_GUIDE_VTABLE);
        rf_write_text(ctx, item + RF_NAME, "Recall Fate", RF_HELP - RF_NAME);
        char help[48];
        uint32_t uses = rf_uses();
        if (uses) snprintf(help, sizeof help, "Rewind to an earlier moment. %u/10 left.", (unsigned)uses);
        else snprintf(help, sizeof help, "No uses left in this battle.");
        rf_write_text(ctx, item + RF_HELP, help, RF_TURN - RF_HELP);
        rf_log("map menu %08X: Recall Fate item %08X first", menu, item);
        /* the seven move down a word; the list ends one word later, still in the builder's frame */
        for (uint32_t i = 7; i > 0; i--) mem_write32(ctx, list + 4 * i, mem_read32(ctx, list + 4 * (i - 1)));
        mem_write32(ctx, list, item);
        mem_write32(ctx, list + 32, 0);
        /* End goes right after Guide: with two columns (4 + 3) it is the left column's last row and
           Options heads the right one */
        uint32_t end = mem_read32(ctx, list + 28);
        for (uint32_t g = 1; g < 6; g++) {
            uint32_t guide = mem_read32(ctx, list + 4 * g);
            if (!guide || mem_read32(ctx, guide) != RF_GUIDE_VTABLE) continue;
            if (mem_read32(ctx, end) == RF_END_VTABLE) {
                for (uint32_t i = 7; i > g + 1; i--) mem_write32(ctx, list + 4 * i, mem_read32(ctx, list + 4 * (i - 1)));
                mem_write32(ctx, list + 4 * (g + 1), end);
            }
            break;
        }
    }
    ctx->r[0] = menu;
    ctx->r[1] = list;
    ctx->r[2] = r2;
    ctx->r[3] = r3;
    ctx->r[12] = r12;
    ctx->r[14] = link;
    TAIL_CALL(RECOMP_ORIGINAL(0x0030071C));
}

/* ---- the map menu entry (Guide's class) ---- */

RECOMP_OVERRIDE(0x001C92CC) { /* slot 0: state */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C92CC));
    ctx->r[0] = rf_entries(NULL, 0) && rf_uses() ? RF_SHOWN : RF_GREYED;
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x001C9360) { /* slot 4: name */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C9360));
    ctx->r[0] = self + RF_NAME;
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x001C9340) { /* slot 5: help text */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C9340));
    ctx->r[0] = self + RF_HELP;
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x001C9310) { /* slot 12: select, opens the turn list */
    uint32_t self = ctx->r[0], link = ctx->r[14];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C9310));

    RfEntry entries[RF_MAX_ROWS];
    size_t count = rf_entries(entries, RF_MAX_ROWS);
    if (count > RF_MAX_ROWS) count = RF_MAX_ROWS;
    if (!count || !rf_uses() || (mem_read32(ctx, self + 0x30) & 2)) {
        ctx->r[0] = RF_REFUSED;
        RETURN_TO(link);
    }
    uint32_t r4 = ctx->r[4], sp = ctx->r[13];
    rf_log("select %08X: %u entries, owner %08X", self, (unsigned)count, mem_read32(ctx, self + 0xC));

    /* the rows, newest first: "Turn N" for a turn's start, "  <unit> — <what>" for each action
       under it (NT-05; recall.rs keeps a checkpoint before each) */
    static const char *const kinds[] = {"",      "Attack",   "Heal",  "Move", "Wait",  "Dance", "Staff",
                                        "Item",  "Pair Up", "Separate", "Rally", "Talk",  "Visit"};
    uint32_t rows[RF_MAX_ROWS];
    size_t row_count = 0;
    for (size_t i = 0; i < count; i++) {
        uint16_t text[RF_NAME_CHARS + 1];
        size_t n = 0;
        uint32_t row_unit = 0;
        if (entries[i].kind == 0) {
            char turn[16];
            snprintf(turn, sizeof turn, "Turn %u", (unsigned)entries[i].turn);
            for (const char *c = turn; *c && n < RF_NAME_CHARS; c++) text[n++] = (uint8_t)*c;
        } else {
            for (int k = 0; k < RF_INDENT; k++) text[n++] = ' ';
            uint32_t name = 0, units = mem_read32(ctx, RF_UNITS);
            if (units && entries[i].unit) {
                uint32_t unit = units + (entries[i].unit - 1u) * RF_UNIT_SIZE;
                /* the unit is still that person's (it may be gone, or its place taken) */
                if (mem_read32(ctx, unit + 0x64) == entries[i].person) {
                    row_unit = unit;
                    ctx->r[0] = unit;
                    RF_GUEST_CALL(RF_UNIT_NAME, link);
                    name = ctx->r[0];
                }
            }
            for (uint32_t k = 0; name && k < 14; k++) {
                uint16_t c = mem_read16(ctx, name + 2 * k);
                if (!c) break;
                text[n++] = c;
            }
            if (!name || !mem_read16(ctx, name)) {
                n = RF_INDENT;
                char unit[12];
                snprintf(unit, sizeof unit, "Unit %u", (unsigned)entries[i].unit);
                for (const char *c = unit; *c; c++) text[n++] = (uint8_t)*c;
            }
            const char *kind = entries[i].kind < sizeof kinds / sizeof *kinds ? kinds[entries[i].kind] : "Wait";
            /* (the game's font has no em dash: it drew "?") */
            text[n++] = ' ';
            text[n++] = '-';
            text[n++] = ' ';
            for (const char *c = kind; *c && n < RF_NAME_CHARS; c++) text[n++] = (uint8_t)*c;
        }
        text[n] = 0;
        ctx->r[0] = RF_ALLOCATION;
        RF_GUEST_CALL(RF_ALLOC, link);
        uint32_t item = ctx->r[0];
        if (!item) break;
        rf_init_item(ctx, item, RF_YES_VTABLE);
        for (size_t k = 0; k <= n; k++) mem_write16(ctx, item + RF_NAME + 2 * (uint32_t)k, text[k]);
        rf_write_text(ctx, item + RF_HELP,
                      entries[i].kind == 0 ? "Return to the start of this turn." : "Return to just before this action.",
                      RF_TURN - RF_HELP);
        mem_write32(ctx, item + RF_TURN, entries[i].id);
        mem_write32(ctx, item + RF_UNIT_ROW, row_unit);
        mem_write32(ctx, item + RF_ACTION_ROW, entries[i].kind != 0);
        rows[row_count++] = item;
    }
    if (!row_count) {
        ctx->r[0] = RF_REFUSED;
        RETURN_TO(link);
    }

    /* the dialog, as End's "End your turn?". Its owner is the map menu: End takes it from slot 23,
       a dynamic_cast<GameMenu *> of the item's +0xC (SeparateMenu *); that cast gave 0 for this
       item on the Thor (2026-11-16) and the dialog never ran. The map menu is a
       mapSystemMenu::MapSystemMenu : MapMenu : SeparateMenu : GameMenu, single inheritance all the
       way, so the cast leaves the pointer as it is: +0xC is the owner. */
    uint32_t owner = mem_read32(ctx, self + 0xC);
    ctx->r[0] = 0x12C;
    RF_GUEST_CALL(RF_ALLOC, link);
    uint32_t menu = ctx->r[0];
    if (!menu) {
        ctx->r[0] = RF_REFUSED;
        RETURN_TO(link);
    }
    ctx->r[0] = menu;
    ctx->r[1] = 0x12C;
    RF_GUEST_CALL(RF_MENU_INIT, link);
    ctx->r[0] = menu;
    ctx->r[1] = 0xFFFFFFFFu;
    RF_GUEST_CALL(RF_MENU_CTOR, link);
    menu = ctx->r[0];
    mem_write32(ctx, menu + 0x78, 1);
    /* (End's dialog sets RF_CENTRED here, as the constructor already has; it is cleared below) */
    mem_write32(ctx, menu, RF_DIALOG_VTABLE);

    /* the fifth argument and the rows' list on the guest stack, below the caller's frame */
    uint32_t list = (sp - 4 * (uint32_t)(row_count + 1)) & ~7u;
    for (size_t i = 0; i < row_count; i++) mem_write32(ctx, list + 4 * (uint32_t)i, rows[i]);
    mem_write32(ctx, list + 4 * (uint32_t)row_count, 0);
    ctx->r[13] = list - 8;
    mem_write32(ctx, ctx->r[13], owner);
    ctx->r[0] = menu;
    ctx->r[1] = RF_DIALOG_KIND;
    ctx->r[2] = RF_DIALOG_NAME;
    ctx->r[3] = 0;
    RF_GUEST_CALL(RF_PROC_START, link);
    ctx->r[1] = list;
    RF_GUEST_CALL(RF_MENU_SET_ITEMS, link);
    uint32_t dialog = ctx->r[0];
    rf_list = dialog; /* laid out in one column (RF_DIALOG_LAYOUT) */
    rf_owner = owner;
    uint32_t inner = mem_read32(ctx, dialog + 0x2C);
    if (inner) mem_write32(ctx, inner + 0x44, mem_read32(ctx, inner + 0x44) + 1);
    mem_write32(ctx, dialog + 0x4C, (mem_read32(ctx, dialog + 0x4C) | 0x4000000u) & ~RF_CENTRED);
    mem_write32(ctx, dialog + 0x110, 0xC0800000u); /* -4.0f */
    char title[32];
    snprintf(title, sizeof title, "Recall Fate (%u left)", (unsigned)rf_uses());
    rf_write_text(ctx, self + RF_TITLE, title, RF_TITLE_BYTES);
    ctx->r[0] = dialog;
    ctx->r[1] = self + RF_TITLE;
    RF_GUEST_CALL(RF_SET_TITLE, link);
    rf_log("turn list %08X open, %u rows, owner %08X, over %08X (%d open)", dialog, (unsigned)row_count, owner, inner,
           inner ? (int32_t)mem_read32(ctx, inner + 0x44) : 0);

    ctx->r[4] = r4;
    ctx->r[13] = sp;
    ctx->r[0] = RF_STAY;
    RETURN_TO(link);
}

RECOMP_OVERRIDE(0x001C9358) { /* slot 22: icon */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C9358));
    ctx->r[0] = RF_CLOCK_ICON;
    RETURN_TO(ctx->r[14]);
}

/* ---- two columns ----
   With Recall Fate the map menu has seven rows in battle, which at the game's 32-px rows overflow
   the top screen (426x240). The menu class lays its rows out as a grid; for the map menu with more
   than six visible rows this asks for two columns (4 + 3), the game doing the rest. */
RECOMP_OVERRIDE(0x003D8460) {
    uint32_t menu = ctx->r[0];
    if (LIKELY(mem_read32(ctx, menu) != RF_MAP_MENU_VTABLE)) TAIL_CALL(RECOMP_ORIGINAL(0x003D8460));
    uint32_t count = mem_read32(ctx, menu + 0x34);
    static uint32_t logged;
    if (logged < 4) {
        logged++;
        rf_log("map menu %08X layout: %u rows, grid %u x %u, window %u x %u, size %d x %d at %d,%d", menu, count,
               mem_read32(ctx, menu + 0x68), mem_read32(ctx, menu + 0x6C), mem_read32(ctx, menu + 0x70),
               mem_read32(ctx, menu + 0x74), (int32_t)mem_read32(ctx, menu + 0x60), (int32_t)mem_read32(ctx, menu + 0x64),
               (int32_t)mem_read32(ctx, menu + 0x58), (int32_t)mem_read32(ctx, menu + 0x5C));
    }
    int32_t width = (int32_t)mem_read32(ctx, menu + 0x60), height = (int32_t)mem_read32(ctx, menu + 0x64);
    uint32_t window_rows = mem_read32(ctx, menu + 0x70);
    if (count > 6 && count <= 8 && mem_read32(ctx, menu + 0x74) == 1 && window_rows == count && width > 0 &&
        height > 0 && rf_has_naga_tear(ctx)) {
        /* the box was sized once, for one column of `count` rows (slots 6/7, which run only while the
           size is negative, at creation): width = item + 0xCC + 2 * 0xB8, height = rows * row + 0xC8 +
           2 * 0xBC. The same for two columns (+ 0xC0 between them), centred as slot 8 centres a new box
           (on 400 x 240) */
        int32_t pad_x = (int32_t)mem_read32(ctx, menu + 0xCC) + 2 * (int32_t)mem_read32(ctx, menu + 0xB8);
        int32_t pad_y = (int32_t)mem_read32(ctx, menu + 0xC8) + 2 * (int32_t)mem_read32(ctx, menu + 0xBC);
        int32_t item = width - pad_x, row = (height - pad_y) / (int32_t)count;
        uint32_t rows = (count + 1) / 2;
        int32_t new_width = 2 * item + (int32_t)mem_read32(ctx, menu + 0xC0) + pad_x;
        int32_t new_height = (int32_t)rows * row + pad_y;
        mem_write32(ctx, menu + 0x68, rows); /* the grid */
        mem_write32(ctx, menu + 0x6C, 2);
        mem_write32(ctx, menu + 0x70, rows); /* the window shown (picks the rows drawn) */
        mem_write32(ctx, menu + 0x74, 2);
        mem_write32(ctx, menu + 0x60, (uint32_t)new_width);
        mem_write32(ctx, menu + 0x64, (uint32_t)new_height);
        mem_write32(ctx, menu + 0x58, (uint32_t)((400 - new_width) / 2));
        mem_write32(ctx, menu + 0x5C, (uint32_t)((240 - new_height) / 2));
        rf_log("map menu %08X: two columns, %d x %d", menu, new_width, new_height);
    }
    TAIL_CALL(RECOMP_ORIGINAL(0x003D8460));
}

/* ---- the clock icon ----
   0x2BF4D4 (sheet r0, index r1, colour r2; s0/s1 where, s2-s4 passed on) draws index < 56 from
   the 32x32 cell (index % 8 * 32, index / 8 * 32) of menu.tga. For RF_CLOCK_ICON this does the
   same steps with the clock's cell, below the game's 256 rows. Without the layer's taller sheet the
   cell is empty and the row shows no icon. */
static uint32_t rf_float_bits(float value) {
    uint32_t bits;
    memcpy(&bits, &value, 4);
    return bits;
}

RECOMP_OVERRIDE(0x002BF4D4) {
    if (LIKELY(ctx->r[1] != RF_CLOCK_ICON)) TAIL_CALL(RECOMP_ORIGINAL(0x002BF4D4));
    uint32_t sheet = ctx->r[0], colour = ctx->r[2], link = ctx->r[14];
    uint32_t place[5];
    memcpy(place, ctx->vfp, sizeof place);
    RF_GUEST_CALL(RF_DRAW_BEGIN, link);
    RF_GUEST_CALL(RF_DRAW_STATE, link);
    ctx->r[0] = mem_read32(ctx, sheet + 0x18);
    RF_GUEST_CALL(RF_DRAW_TEXTURE, link);
    RF_GUEST_CALL(RF_DRAW_MODE, link);
    memcpy(ctx->vfp, place, sizeof place);
    ctx->vfp[5] = rf_float_bits(RF_CLOCK_X);
    ctx->vfp[6] = rf_float_bits(RF_CLOCK_Y);
    ctx->r[0] = colour;
    RF_GUEST_CALL(RF_DRAW_QUAD, link);
    RF_GUEST_CALL(RF_DRAW_END, link);
    RETURN_TO(link);
}

/* ---- the history list's layout ----
   End's dialog puts its rows side by side, and sets its grid itself after the list is made. For
   the history list (rf_list) its layout (slot 9, the same grid as the map menu's: +0x68 rows a
   column, +0x6C columns, window +0x70 x +0x74) gets one column; the box is sized by slots 6 and 7
   (0x3D7CE4, 0x3D7BE8: from the window, while the size at +0x60/+0x64 is negative). Sized once for
   one row to learn the row's height and the box's padding, then for as many rows as fit between
   RF_LIST_TOP and RF_LIST_BOTTOM. Slot 8 (0x15C500) keeps a place at +0x58/+0x5C that is not
   negative: the box goes to the left edge, at the top, like a side panel beside the map. */

/* (a macro: a guest call can stop the override it is in) */
#define RF_SIZE_LIST(dialog, window_rows, link) do { \
    mem_write32(ctx, (dialog) + 0x70, (window_rows)); \
    mem_write32(ctx, (dialog) + 0x60, 0xFFFFFFFFu); \
    mem_write32(ctx, (dialog) + 0x64, 0xFFFFFFFFu); \
    for (uint32_t slot = 6; slot <= 7; slot++) { \
        ctx->r[0] = (dialog); \
        RF_GUEST_CALL(mem_read32(ctx, mem_read32(ctx, (dialog)) + 4 * slot), (link)); \
    } \
} while (0)

RECOMP_OVERRIDE(0x0015C5C0) {
    uint32_t dialog = ctx->r[0], link = ctx->r[14];
    if (LIKELY(dialog != rf_list || !rf_list)) TAIL_CALL(RECOMP_ORIGINAL(0x0015C5C0));
    uint32_t rows = mem_read32(ctx, dialog + 0x34);
    if (rows && (mem_read32(ctx, dialog + 0x6C) != 1 || mem_read32(ctx, dialog + 0x74) != 1)) {
        rf_log("list %08X: %u rows, grid was %u x %u, window %u x %u, size %d x %d", dialog, rows,
               mem_read32(ctx, dialog + 0x68), mem_read32(ctx, dialog + 0x6C), mem_read32(ctx, dialog + 0x70),
               mem_read32(ctx, dialog + 0x74), (int32_t)mem_read32(ctx, dialog + 0x60), (int32_t)mem_read32(ctx, dialog + 0x64));
        mem_write32(ctx, dialog + 0x68, rows);
        mem_write32(ctx, dialog + 0x6C, 1);
        mem_write32(ctx, dialog + 0x74, 1);
        /* the box is the rows' heights (each its own) plus the padding: as many rows as fit */
        uint32_t items = mem_read32(ctx, dialog + 0x30);
        RF_SIZE_LIST(dialog, 1, link);
        int32_t first = mem_read32(ctx, mem_read32(ctx, items) + RF_ACTION_ROW) ? RF_ACTION_HEIGHT : RF_TURN_HEIGHT;
        int32_t pad = (int32_t)mem_read32(ctx, dialog + 0x64) - first, used = pad, row = first;
        uint32_t fit = 0;
        while (fit < rows) {
            uint32_t item = mem_read32(ctx, items + 4 * fit);
            int32_t height = mem_read32(ctx, item + RF_ACTION_ROW) ? RF_ACTION_HEIGHT : RF_TURN_HEIGHT;
            if (fit && used + height > RF_LIST_BOTTOM - RF_LIST_TOP) break;
            used += height;
            fit++;
        }
        RF_SIZE_LIST(dialog, fit, link);
        mem_write32(ctx, dialog + 0x58, RF_LIST_X);
        mem_write32(ctx, dialog + 0x5C, RF_LIST_TOP);
        ctx->r[0] = dialog;
        RF_GUEST_CALL(mem_read32(ctx, mem_read32(ctx, dialog) + 4 * 8), link);
        rf_log("list %08X: row %d, padding %d, %u of %u rows shown, %d x %d at %d,%d", dialog, row, pad,
               mem_read32(ctx, dialog + 0x70), rows, (int32_t)mem_read32(ctx, dialog + 0x60),
               (int32_t)mem_read32(ctx, dialog + 0x64), (int32_t)mem_read32(ctx, dialog + 0x58),
               (int32_t)mem_read32(ctx, dialog + 0x5C));
        ctx->r[0] = dialog;
        ctx->r[14] = link;
    }
    TAIL_CALL(RECOMP_ORIGINAL(0x0015C5C0));
}

/* ---- the turn rows (the Yes class of End's dialog) ---- */

RECOMP_OVERRIDE(0x001CE2C0) { /* slot 1: width (80 for the game's Yes items) */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001CE2C0));
    ctx->r[0] = RF_ROW_WIDTH;
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x001F8384) { /* slot 4: name, shared by the game's Yes items */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001F8384));
    ctx->r[0] = self + RF_NAME;
    RETURN_TO(ctx->r[14]);
}

/* slot 13: B on a row. The Yes class returns 0, and the Yes/No dialog's cancel (slot 25, 0x1DE408)
   then selects item 1 (End's "No"), which here is a turn: B rewound the battle (Thor, 2026-11-16).
   Rows return what map menu items return for B (0x201: back, with the cancel sound), which closes
   the list and leaves the map menu open. */
RECOMP_OVERRIDE(0x001F837C) {
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001F837C));
    rf_owner = 0; /* the list closes; the map menu shows again */
    ctx->r[0] = RF_BACK;
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x001C925C) { /* slot 12: select */
    uint32_t self = ctx->r[0];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001C925C));
    uint32_t id = mem_read32(ctx, self + RF_TURN);
    rf_log("row %08X: back to history entry %u", self, (unsigned)id);
    /* the runtime restores it after this frame, menus and all */
    rf_go(id);
    rf_owner = 0;
    ctx->r[0] = RF_STAY;
    RETURN_TO(ctx->r[14]);
}

/* ---- the map menu hidden behind the list ----
   The map menu draws itself in its slot 39 (0x1C93CC: the box and rows through the menus' common
   0x2E1CF8, then its own extras) and its cursor in slot 40 (0x3D81A0, shared by all menus). While
   the history list opened from it is up (rf_owner; the menu's +0x44 counts the dialogs open over
   it) neither runs for it. */
static int rf_hidden(Context *ctx, uint32_t menu) {
    return menu && menu == rf_owner && (int32_t)mem_read32(ctx, menu + 0x44) > 0;
}

RECOMP_OVERRIDE(0x001C93CC) {
    if (LIKELY(!rf_hidden(ctx, ctx->r[0]))) TAIL_CALL(RECOMP_ORIGINAL(0x001C93CC));
    RETURN_TO(ctx->r[14]);
}

RECOMP_OVERRIDE(0x003D81A0) {
    if (LIKELY(!rf_hidden(ctx, ctx->r[0]))) TAIL_CALL(RECOMP_ORIGINAL(0x003D81A0));
    RETURN_TO(ctx->r[14]);
}

/* ---- the acting unit's sprite on its row ----
   The rows are of the Yes class, whose slot 2 (0x192730, shared by many classes) gives their height
   and slot 8 (0x1CE024) draws them. For an action row the game's draw runs first, then the unit's
   map sprite goes on top the way the unit list's rows draw theirs (0x2E0DA8): a description from the
   unit (0x2C785C into 0xC bytes, the name string at +8 starting empty) drawn by 0x2E2150 through the
   sprite manager, clipped to the row's box (0x2EE944 between 0x2F501C and 0x2F4F4C, as the menus do)
   and the clip put back to the whole screen after. */
RECOMP_OVERRIDE(0x00192730) {
    if (LIKELY(!rf_mine(ctx, ctx->r[0]))) TAIL_CALL(RECOMP_ORIGINAL(0x00192730));
    ctx->r[0] = mem_read32(ctx, ctx->r[0] + RF_ACTION_ROW) ? RF_ACTION_HEIGHT : RF_TURN_HEIGHT;
    RETURN_TO(ctx->r[14]);
}

/* 0x1CE024 draws a row's text at its +0x14 - 3, whatever its height, and centres its box (24 px,
   fixed) on +0x14 - 3 + (height - 24) / 2. A turn row (taller) is drawn with +0x14 moved down by
   half the difference from an action row and the height less twice that: the box stays where it
   was, and the text is centred in it as an action row's is. The shift is noted in the row
   (RF_SHIFT) while it is in place: if the host stops inside the draw, the guest goes on to the
   caller and the next draw takes it back first (it ran away before: the row's height went down 6
   at a time and the game froze, Thor 2026-11-16).
   The sprite: every guest call here is made with sp as it came in and the caller's return as its
   return, except the sprite draw, which needs a stack argument: see RF_POP_R3_PC. */
static void rf_unshift(Context *ctx, uint32_t self) {
    int32_t shift = (int32_t)mem_read32(ctx, self + RF_SHIFT);
    if (!shift) return;
    mem_write32(ctx, self + 0x14, mem_read32(ctx, self + 0x14) - (uint32_t)shift);
    mem_write32(ctx, self + 0x24, mem_read32(ctx, self + 0x24) + 2u * (uint32_t)shift);
    mem_write32(ctx, self + RF_SHIFT, 0);
}

RECOMP_OVERRIDE(0x001CE024) {
    uint32_t self = ctx->r[0], link = ctx->r[14];
    if (LIKELY(!rf_mine(ctx, self))) TAIL_CALL(RECOMP_ORIGINAL(0x001CE024));
    rf_unshift(ctx, self);
    uint32_t r4 = ctx->r[4], r5 = ctx->r[5], r6 = ctx->r[6], r7 = ctx->r[7], sp = ctx->r[13];
    if (!mem_read32(ctx, self + RF_ACTION_ROW)) {
        int32_t height = (int32_t)mem_read32(ctx, self + 0x24);
        int32_t shift = (height - RF_ACTION_HEIGHT) / 2;
        if (shift > 0 && shift < 16) {
            mem_write32(ctx, self + 0x14, mem_read32(ctx, self + 0x14) + (uint32_t)shift);
            mem_write32(ctx, self + 0x24, (uint32_t)(height - 2 * shift));
            mem_write32(ctx, self + RF_SHIFT, (uint32_t)shift);
        }
        CALL(RECOMP_ORIGINAL(0x001CE024));
        rf_unshift(ctx, self);
        ctx->r[13] = sp;
        RETURN_TO(link);
    }
    uint32_t unit = mem_read32(ctx, self + RF_UNIT_ROW);
    if (!unit) TAIL_CALL(RECOMP_ORIGINAL(0x001CE024));
    CALL(RECOMP_ORIGINAL(0x001CE024));
    ctx->r[13] = sp;
    /* still that unit's row (a rewind may have changed what is there) */
    uint32_t manager = mem_read32(ctx, RF_SPRITE_MANAGER);
    if (manager && mem_read32(ctx, self + RF_UNIT_ROW) == unit && mem_read32(ctx, unit + 0x64)) {
        float x = (float)(int32_t)mem_read32(ctx, self + 0x10) + RF_SPRITE_X;
        float y = (float)(int32_t)mem_read32(ctx, self + 0x14) + RF_SPRITE_Y;
        uint32_t description = self + RF_SPRITE_DESC, colour = self + RF_SPRITE_COLOUR;
        mem_write32(ctx, colour, 0xFFFFFFFFu);
        for (uint32_t i = 0; i < 0x10; i += 4) mem_write32(ctx, description + i, 0);
        ctx->r[0] = description;
        ctx->r[1] = unit;
        RF_GUEST_CALL(RF_SPRITE_DESCRIBE, link);
        int32_t box_x = (int32_t)mem_read32(ctx, self + 0x10) - 8, box_y = (int32_t)mem_read32(ctx, self + 0x14) - 3;
        int32_t box_w = (int32_t)mem_read32(ctx, self + 0x20) + 16, box_h = (int32_t)mem_read32(ctx, self + 0x24);
        RF_GUEST_CALL(RF_DRAW_BEGIN, link);
        RF_GUEST_CALL(RF_DRAW_STATE, link);
        ctx->r[0] = (uint32_t)box_x;
        ctx->r[1] = (uint32_t)(box_y + 1);
        ctx->r[2] = (uint32_t)box_w;
        ctx->r[3] = (uint32_t)(box_h - 2);
        RF_GUEST_CALL(RF_DRAW_CLIP, link);
        RF_GUEST_CALL(RF_DRAW_MODE, link);
        uint32_t frame = sp - 8;
        mem_write32(ctx, frame, colour);
        mem_write32(ctx, frame + 4, link);
        ctx->r[13] = frame;
        ctx->r[0] = manager;
        ctx->r[1] = description;
        ctx->r[2] = 0;
        ctx->r[3] = 0;
        ctx->vfp[0] = rf_float_bits(x);
        ctx->vfp[1] = rf_float_bits(y);
        ctx->vfp[2] = 0;
        RF_GUEST_CALL(RF_SPRITE_DRAW, RF_POP_R3_PC);
        ctx->r[13] = sp;
        RF_GUEST_CALL(RF_DRAW_STATE, link);
        ctx->r[0] = 0;
        ctx->r[1] = 0;
        ctx->r[2] = 400;
        ctx->r[3] = 240;
        RF_GUEST_CALL(RF_DRAW_CLIP, link);
        RF_GUEST_CALL(RF_DRAW_MODE, link);
        RF_GUEST_CALL(RF_DRAW_END, link);
    }
    ctx->r[4] = r4;
    ctx->r[5] = r5;
    ctx->r[6] = r6;
    ctx->r[7] = r7;
    ctx->r[13] = sp;
    RETURN_TO(link);
}

/* ---- the unit command menu: which command ended an action ----
 * Each command is its own item class (vtables 0x49FD48 Items .. 0x4A060C Separate, 0x8C apart, name
 * labels MID_MENU_持ち物/杖/踊る/待機/攻撃/共闘/降ろす/応援/会話/訪問 in m/E/Menu.bin); slot 12 is the
 * select, refused (0x400) while the item's +0x30 has 2. Choosing one tells the runtime, which names
 * the action by the last command chosen before the unit's "has acted" bit is set (a command backed
 * out of is replaced by the next one; Wait and no command leave it to what the action changed). */
/* the select of a command item: tells the runtime unless the item is greyed */
static void rf_chosen(Context *ctx, uint8_t kind) {
    if (!(mem_read32(ctx, ctx->r[0] + 0x30) & 2)) rf_command(kind);
}
RECOMP_OVERRIDE(0x001DC234) { /* Attack */
    rf_chosen(ctx, 1);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DC234));
}
RECOMP_OVERRIDE(0x001DBE10) { /* Wait */
    rf_chosen(ctx, 4);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DBE10));
}
RECOMP_OVERRIDE(0x001DBD6C) { /* Dance */
    rf_chosen(ctx, 5);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DBD6C));
}
RECOMP_OVERRIDE(0x001DB954) { /* Staff */
    rf_chosen(ctx, 6);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DB954));
}
RECOMP_OVERRIDE(0x001DB668) { /* Items */
    rf_chosen(ctx, 7);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DB668));
}
RECOMP_OVERRIDE(0x001DC39C) { /* Pair Up */
    rf_chosen(ctx, 8);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DC39C));
}
RECOMP_OVERRIDE(0x001DC6F4) { /* Separate */
    rf_chosen(ctx, 9);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DC6F4));
}
RECOMP_OVERRIDE(0x001DBC68) { /* Rally */
    rf_chosen(ctx, 10);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DBC68));
}
RECOMP_OVERRIDE(0x001DBB6C) { /* Talk */
    rf_chosen(ctx, 11);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DBB6C));
}
RECOMP_OVERRIDE(0x001DC0D0) { /* Visit */
    rf_chosen(ctx, 12);
    TAIL_CALL(RECOMP_ORIGINAL(0x001DC0D0));
}
