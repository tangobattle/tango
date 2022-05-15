#ifdef GL_ES
precision mediump float;
#endif

#if __VERSION__ >= 130
#define IN in
#define OUT out
#else
#define IN attribute
#define OUT varying
#endif

IN vec2 a_pos;
IN vec2 a_uv;
OUT vec2 v_uv;

void main() {
  gl_Position = vec4(a_pos, 0.0, 1.0);
  v_uv = a_uv;
}
