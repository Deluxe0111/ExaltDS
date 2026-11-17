/* The feet override's math against fe-awakening-recomp's feet.rs tests.
   Build: clang -I <3dsrecomp build>/generated crates/exalt-mods/overrides/tests/feet_test.c -o feet_test && ./feet_test */
#include <stdio.h>
#include "../feet.c"

/* the generated original, which the math tests never reach */
uint32_t f_00306300(Context *ctx) { (void)ctx; return 0; }

static int failures;
#define CHECK(cond, what) do { if (!(cond)) { printf("FAIL %s\n", what); failures++; } else printf("ok   %s\n", what); } while (0)

static void stand(float lean_degrees, float height, FeetVector *toe_out, FeetVector *heel_out) {
    float r = lean_degrees * 3.14159265358979f / 180.0f, s = sinf(r), c = cosf(r);
    FeetVector ankle = feet_vec(0, height, 0);
    FeetVector toe = feet_add(ankle, feet_vec(0, -0.8f * c - 1.5f * s, -0.8f * s + 1.5f * c));
    FeetVector heel = feet_add(ankle, feet_vec(0, -0.8f * c + 0.4f * s, -0.8f * s - 0.4f * c));
    FeetGround g; g.up = feet_vec(0, 1, 0); g.floor = feet_vec(0, 0, 0); g.ankle = ankle; g.toe = toe;
    float angle;
    if (feet_turn_onto_floor(&g, heel, &angle)) {
        *toe_out = feet_add(ankle, feet_rotate(feet_sub(toe, ankle), g.axis, angle));
        *heel_out = feet_add(ankle, feet_rotate(feet_sub(heel, ankle), g.axis, angle));
    } else { *toe_out = toe; *heel_out = heel; }
}

int main(void) {
    FeetVector toe, heel;
    stand(15, 2.0f, &toe, &heel);
    float r = 15 * 3.14159265358979f / 180.0f;
    CHECK(toe.v[1] > 0 && heel.v[1] > 0 && fabsf(toe.v[1] - (2.0f - 0.8f * cosf(r) - 1.5f * sinf(r))) < 1e-5f, "a foot off the floor keeps its angle to the shin");
    stand(15, 0.9f, &toe, &heel);
    CHECK(fabsf(toe.v[1]) < 1e-4f && heel.v[1] > toe.v[1], "a toe that would dig in comes up to the floor");
    stand(-15, 0.85f, &toe, &heel);
    CHECK(fabsf(toe.v[1] - heel.v[1]) < 1e-4f && heel.v[1] > -0.1f && heel.v[1] < 0.1f, "a heel that would dig in lays the foot flat");
    stand(15, 0.7f, &toe, &heel);
    CHECK(fabsf(toe.v[1] - heel.v[1]) < 1e-4f && toe.v[1] < 0, "a foot on the floor lies flat");
    FeetMatrix m = {{{1, 0, 0, 1}, {0, 1, 0, 2}, {0, 0, 1, 3}}}, out;
    FeetVector ankle = feet_vec(1, 2, 3), x = feet_vec(1, 0, 0);
    feet_turned_about(&m, x, 0.3f, ankle, &out);
    FeetVector a = feet_sub(feet_apply(&out, feet_vec(0, 0, 0)), ankle);
    FeetVector t = feet_sub(feet_apply(&out, feet_vec(0, 0, 1)), feet_add(ankle, feet_rotate(feet_vec(0, 0, 1), x, 0.3f)));
    CHECK(fabsf(a.v[0]) < 1e-6f && fabsf(a.v[1]) < 1e-6f && fabsf(a.v[2]) < 1e-6f && fabsf(t.v[0]) < 1e-6f && fabsf(t.v[1]) < 1e-6f && fabsf(t.v[2]) < 1e-6f,
          "the turned matrix moves the foot about the ankle");
    printf("%s\n", failures ? "FAILED" : "all passed");
    return failures != 0;
}
