/* Feet on the ground, for Fire Emblem Awakening USA (00040000000A0500) rev0.
 *
 * A C port of fe-awakening-recomp 1.0.17's feet hook (zakuro-core/src/feet.rs, MIT), as a
 * 3dsrecomp override instead of a runtime hook: 3dsrecomp 730fbac compiles direct calls to
 * direct C calls, which a runtime lookup hook never sees, while an override replaces the
 * function for every caller.
 *
 * The feet mod's models (tools/feet-usa) give each foot bone inverse bind data that starts with
 * three points (ankle, toe, heel, 9 floats) followed by the tag "FEET". When the skinning loop
 * multiplies such a bone, this override uses the shin's skinning matrix instead and turns it
 * about the ankle until the toe and the heel no longer sink into the floor. Every other call
 * runs the game's own function, so without the mod's models nothing changes.
 *
 * Addresses, EUR -> USA (checked by disassembly of the USA code, 2026-11-16):
 *   skinning product  0x306964 -> 0x306300  (vpush {d8-d10}; r0 out, r1 world, r2 inverse bind)
 *   loop return       0x2607CC -> 0x2602A0  (bl 0x306300 at 0x26029C, r2 = bone record + 0xA4)
 */
#include "overrides.h"

#define FEET_SKINNING_PRODUCT 0x00306300u
#define FEET_LOOP_RETURN 0x002602A0u
#define FEET_TAG 0x54454546u /* "FEET", little-endian */
#define FEET_TAG_OFFSET 36u
#define FEET_INVERSE_BIND_OFFSET 0xA4u
#define FEET_BONE_INDEX_OFFSET 0x8u
#define FEET_PARENT_INDEX_OFFSET 0xCu
#define FEET_MATRIX_BYTES 48u
#define FEET_BISECTION_STEPS 24

typedef struct { float m[3][4]; } FeetMatrix;
typedef struct { float v[3]; } FeetVector;

static float feet_read_float(Context *ctx, uint32_t address) {
    uint32_t bits = mem_read32(ctx, address);
    float value;
    memcpy(&value, &bits, 4);
    return value;
}

static void feet_read_matrix(Context *ctx, uint32_t address, FeetMatrix *out) {
    for (int i = 0; i < 12; i++) out->m[i / 4][i % 4] = feet_read_float(ctx, address + 4u * (uint32_t)i);
}

static void feet_write_matrix(Context *ctx, uint32_t address, const FeetMatrix *matrix) {
    for (int i = 0; i < 12; i++) {
        uint32_t bits;
        memcpy(&bits, &matrix->m[i / 4][i % 4], 4);
        mem_write32(ctx, address + 4u * (uint32_t)i, bits);
    }
}

static FeetVector feet_vec(float x, float y, float z) { FeetVector r = {{x, y, z}}; return r; }
static FeetVector feet_add(FeetVector a, FeetVector b) { return feet_vec(a.v[0] + b.v[0], a.v[1] + b.v[1], a.v[2] + b.v[2]); }
static FeetVector feet_sub(FeetVector a, FeetVector b) { return feet_vec(a.v[0] - b.v[0], a.v[1] - b.v[1], a.v[2] - b.v[2]); }
static float feet_dot(FeetVector a, FeetVector b) { return a.v[0] * b.v[0] + a.v[1] * b.v[1] + a.v[2] * b.v[2]; }
static FeetVector feet_cross(FeetVector a, FeetVector b) {
    return feet_vec(a.v[1] * b.v[2] - a.v[2] * b.v[1], a.v[2] * b.v[0] - a.v[0] * b.v[2], a.v[0] * b.v[1] - a.v[1] * b.v[0]);
}
static FeetVector feet_normalize(FeetVector a) {
    float length = sqrtf(feet_dot(a, a));
    if (length == 0.0f) return a;
    return feet_vec(a.v[0] / length, a.v[1] / length, a.v[2] / length);
}
static FeetVector feet_column(const FeetMatrix *matrix, int index) {
    return feet_vec(matrix->m[0][index], matrix->m[1][index], matrix->m[2][index]);
}
static FeetVector feet_apply(const FeetMatrix *matrix, FeetVector p) {
    FeetVector r;
    for (int i = 0; i < 3; i++) r.v[i] = matrix->m[i][0] * p.v[0] + matrix->m[i][1] * p.v[1] + matrix->m[i][2] * p.v[2] + matrix->m[i][3];
    return r;
}
/* Rodrigues' formula, axis of length one. */
static FeetVector feet_rotate(FeetVector vector, FeetVector axis, float angle) {
    float s = sinf(angle), c = cosf(angle);
    FeetVector across = feet_cross(axis, vector);
    float along = feet_dot(axis, vector) * (1.0f - c);
    FeetVector r;
    for (int i = 0; i < 3; i++) r.v[i] = vector.v[i] * c + across.v[i] * s + axis.v[i] * along;
    return r;
}

typedef struct { FeetVector up, floor, ankle, toe, axis; } FeetGround;

static float feet_height(const FeetGround *g, FeetVector p) { return feet_dot(g->up, feet_sub(p, g->floor)); }
static float feet_toe_height_at(const FeetGround *g, float angle) {
    return feet_height(g, feet_add(g->ankle, feet_rotate(feet_sub(g->toe, g->ankle), g->axis, angle)));
}

/* The axis and angle that turn the foot about the ankle so that it stops sinking into the floor;
   0 when the toe and the heel are above it, or the sole is parallel to up. */
static int feet_turn_onto_floor(FeetGround *g, FeetVector heel, float *angle) {
    if (feet_height(g, g->toe) >= 0.0f && feet_height(g, heel) >= 0.0f) return 0;
    FeetVector sole = feet_sub(g->toe, heel);
    FeetVector across = feet_cross(g->up, sole);
    if (feet_dot(across, across) < 1e-12f) return 0;
    g->axis = feet_normalize(across);
    float rise = feet_dot(g->up, sole);
    float ratio = rise / sqrtf(feet_dot(sole, sole));
    if (ratio < -1.0f) ratio = -1.0f;
    if (ratio > 1.0f) ratio = 1.0f;
    float to_flat = asinf(ratio);
    if (rise >= 0.0f || feet_toe_height_at(g, to_flat) <= 0.0f) {
        *angle = to_flat;
        return 1;
    }
    /* the toe is the low point and is above the floor once flat: find where it touches */
    float under = 0.0f, over = to_flat;
    for (int i = 0; i < FEET_BISECTION_STEPS; i++) {
        float middle = (under + over) / 2.0f;
        if (feet_toe_height_at(g, middle) < 0.0f) under = middle; else over = middle;
    }
    *angle = over;
    return 1;
}

static void feet_turned_about(const FeetMatrix *matrix, FeetVector axis, float angle, FeetVector pivot, FeetMatrix *out) {
    for (int c = 0; c < 3; c++) {
        FeetVector turned = feet_rotate(feet_column(matrix, c), axis, angle);
        for (int r = 0; r < 3; r++) out->m[r][c] = turned.v[r];
    }
    FeetVector origin = feet_add(pivot, feet_rotate(feet_sub(feet_column(matrix, 3), pivot), axis, angle));
    for (int r = 0; r < 3; r++) out->m[r][3] = origin.v[r];
}

RECOMP_OVERRIDE(0x00306300) {
    uint32_t skinning = ctx->r[0], world = ctx->r[1], inverse_bind = ctx->r[2];
    if (LIKELY(ctx->r[14] != FEET_LOOP_RETURN) || LIKELY(mem_read32(ctx, inverse_bind + FEET_TAG_OFFSET) != FEET_TAG))
        TAIL_CALL(RECOMP_ORIGINAL(0x00306300));
    uint32_t record = inverse_bind - FEET_INVERSE_BIND_OFFSET;
    uint32_t bone = mem_read32(ctx, record + FEET_BONE_INDEX_OFFSET);
    uint32_t parent = mem_read32(ctx, record + FEET_PARENT_INDEX_OFFSET);
    if (parent >= bone)
        TAIL_CALL(RECOMP_ORIGINAL(0x00306300));

    /* bone matrices lie one after another: step back to the parent's skinning matrix and to the
       first bone's world matrix (the model's) */
    FeetMatrix shin, model, foot;
    feet_read_matrix(ctx, skinning - (bone - parent) * FEET_MATRIX_BYTES, &shin);
    feet_read_matrix(ctx, world - bone * FEET_MATRIX_BYTES, &model);
    FeetVector points[3];
    for (int p = 0; p < 3; p++) {
        FeetVector raw;
        for (int a = 0; a < 3; a++) raw.v[a] = feet_read_float(ctx, inverse_bind + 4u * (uint32_t)(3 * p + a));
        points[p] = feet_apply(&shin, raw);
    }
    FeetGround ground;
    ground.up = feet_normalize(feet_column(&model, 1));
    ground.floor = feet_column(&model, 3);
    ground.ankle = points[0];
    ground.toe = points[1];
    float angle;
    if (feet_turn_onto_floor(&ground, points[2], &angle)) feet_turned_about(&shin, ground.axis, angle, ground.ankle, &foot);
    else foot = shin;
    feet_write_matrix(ctx, skinning, &foot);
    RETURN_TO(ctx->r[14]);
}
