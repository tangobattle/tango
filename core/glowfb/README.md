# glowfb

glowfb is a simple pixel framebuffer for use with [glow](https://github.com/grovesNL/glow). It's designed to support OpenGL 2.1 all the way up to 4.6, so you can use it with really old GPUs (e.g. really old Intel iGPUs) if you really need to.

## How to use

Just two steps!

### 1. Obtain a Glow GL context somehow

If you're using e.g. winit + glutin:

```rust
let event_loop = winit::event_loop::EventLoop::new();

let wb =  glutin::window::WindowBuilder::new();

let gl_window = unsafe {
    glutin::ContextBuilder::new()
        .build_windowed(wb, &event_loop)
        .unwrap()
        .make_current()
        .unwrap()
};

let gl = std::rc::Rc::new(unsafe {
    glow::Context::from_loader_function(|s| gl_window.get_proc_address(s))
});

let mut fb = glowfb::Framebuffer::new(
    gl.clone(),
    glutin::dpi::LogicalSize { width, height },
).unwrap();
```

### 2. Hook it up to the event loop

```rust
event_loop.run(move |event, _, control_flow| {
    match event {
        winit::event::Event::MainEventsCleared => {
            unsafe {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
            fb.draw(gl_window.window().inner_size(), &pixels);
            gl_window.swap_buffers().unwrap();
        }
        winit::event::Event::WindowEvent {
            event: ref window_event,
            ..
        } => {
            match window_event {
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = winit::event_loop::ControlFlow::Exit;
                }
                winit::event::WindowEvent::Resized(size) => {
                    gl_window.resize(*size);
                }
                _ => {}
            };
        }
        _ => {}
    }

});
```

## Limitations

glowfb was designed for use with a GBA emulator, so it doesn't have very many bells and whistles. In particular:

-   The internal buffer cannot be resized.

-   Only proportional integer scaling is performed.

## Alternatives

-   [mini_gl_fb](https://github.com/shivshank/mini_gl_fb): Another OpenGL framebuffer library! Similar to this library but requires OpenGL 3.3+ support due to use of GLSL 3.3 and direct use of VAOs.

-   [pixels](https://github.com/parasyte/pixels): A much more modern approach supporting Vulkan/DX12/GLES via wgpu.

-   [softbuffer](https://github.com/john01dav/softbuffer): A pixel buffer implemented completely in software rendering.

-   [minifb](https://github.com/emoon/rust_minifb): A similar framebuffer library but comes with window management built in.

## Acknowledgements

-   [shivshank](https://github.com/shivshank) for the original mini_gl_fb package.

-   [emilk](https://github.com/emilk) for the OpenGL shim code in egui.
