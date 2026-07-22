#version 300 es

precision highp float;
precision highp int;

struct VsOut {
    vec4 position;
    vec2 uv;
};
const bool SRGB_TARGET = false;
const int SCALE_I = 2;
const float SCALE_F = 2.0;

smooth out vec2 _vs2fs_location0;

float srgb_to_linear(float c) {
    if ((c <= 0.04045)) {
        return (c / 12.92);
    }
    return pow(((c + 0.055) / 1.055), 2.4);
}

vec3 decode(uint raw) {
    vec3 c_1 = vec3(0.0);
    uint r = (((raw & 31u) * 255u) / 31u);
    uint g = ((((raw >> 5u) & 31u) * 255u) / 31u);
    uint b_2 = ((((raw >> 10u) & 31u) * 255u) / 31u);
    c_1 = (vec3(float(r), float(g), float(b_2)) / vec3(255.0));
    if (SRGB_TARGET) {
        float _e33 = c_1.x;
        float _e34 = srgb_to_linear(_e33);
        float _e36 = c_1.y;
        float _e37 = srgb_to_linear(_e36);
        float _e39 = c_1.z;
        float _e40 = srgb_to_linear(_e39);
        c_1 = vec3(_e34, _e37, _e40);
    }
    vec3 _e42 = c_1;
    return _e42;
}

bool yuv_diff(vec3 a, vec3 b) {
    bool local = false;
    bool local_1 = false;
    vec3 d = (a - b);
    float y = dot(d, vec3(0.299, 0.587, 0.114));
    float u = dot(d, vec3(-0.169, -0.331, 0.5));
    float v = dot(d, vec3(0.5, -0.419, -0.081));
    if (!((abs(y) > 0.1882353))) {
        local = (abs(u) > 0.02745098);
    } else {
        local = true;
    }
    bool _e28 = local;
    if (!(_e28)) {
        local_1 = (abs(v) > 0.023529412);
    } else {
        local_1 = true;
    }
    bool _e36 = local_1;
    return _e36;
}

bool diff(vec3 a_1, vec3 b_1) {
    bool _e2 = yuv_diff(a_1, b_1);
    return _e2;
}

vec3 interp1_(vec3 c1_, vec3 c2_) {
    return (((c1_ * 3.0) + c2_) / vec3(4.0));
}

vec3 interp2_(vec3 c1_1, vec3 c2_1, vec3 c3_) {
    return ((((c1_1 * 2.0) + c2_1) + c3_) / vec3(4.0));
}

vec3 interp3_(vec3 c1_2, vec3 c2_2) {
    return (((c1_2 * 7.0) + c2_2) / vec3(8.0));
}

vec3 interp4_(vec3 c1_3, vec3 c2_3, vec3 c3_1) {
    return (((c1_3 * 2.0) + ((c2_3 + c3_1) * 7.0)) / vec3(16.0));
}

vec3 interp5_(vec3 c1_4, vec3 c2_4) {
    return ((c1_4 + c2_4) / vec3(2.0));
}

vec3 interp6_(vec3 c1_5, vec3 c2_5, vec3 c3_2) {
    return ((((c1_5 * 5.0) + (c2_5 * 2.0)) + c3_2) / vec3(8.0));
}

vec3 interp7_(vec3 c1_6, vec3 c2_6, vec3 c3_3) {
    return ((((c1_6 * 6.0) + c2_6) + c3_3) / vec3(8.0));
}

vec3 interp8_(vec3 c1_7, vec3 c2_7) {
    return (((c1_7 * 5.0) + (c2_7 * 3.0)) / vec3(8.0));
}

vec3 interp9_(vec3 c1_8, vec3 c2_8, vec3 c3_4) {
    return (((c1_8 * 2.0) + ((c2_8 + c3_4) * 3.0)) / vec3(8.0));
}

vec3 interp10_(vec3 c1_9, vec3 c2_9, vec3 c3_5) {
    return ((((c1_9 * 14.0) + c2_9) + c3_5) / vec3(16.0));
}

void main() {
    uint index = uint(gl_VertexID);
    VsOut out_ = VsOut(vec4(0.0), vec2(0.0));
    vec2 uv = vec2(float(((index << 1u) & 2u)), float((index & 2u)));
    out_.position = vec4(((uv.x * 2.0) - 1.0), (1.0 - (uv.y * 2.0)), 0.0, 1.0);
    out_.uv = uv;
    VsOut _e26 = out_;
    gl_Position = _e26.position;
    _vs2fs_location0 = _e26.uv;
    return;
}

