#ifdef GL_ES
precision mediump float;
#endif

#if __VERSION__ >= 130
#define IN in
out vec4 f_color;
#else
#define IN varying
#define f_color gl_FragColor
#define texture texture2D
#endif

uniform sampler2D u_sampler;
IN vec2 v_uv;

void main() { f_color = texture(u_sampler, v_uv); }
