//! hqx GPU upscalers. All three scales use the same compact evaluator shader
//! with private packed lookup tables, keeping the hqx rule data out of both
//! [`Effect`] and the GPU compiler's control-flow graph.

use crate::platform::video::framebuffer::{compile_render_pipeline, CompiledEffect, Effect, EffectRenderer};

mod rules;

const SHADER: &str = include_str!("hqx.wgsl");
const PARTS: &[&str] = &[super::COMMON, SHADER];

#[derive(Debug)]
struct HqxRenderer {
    table: rules::Table,
    constants: &'static [(&'static str, f64)],
}

impl EffectRenderer for HqxRenderer {
    fn compile(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
        effect_id: &'static str,
    ) -> Box<dyn CompiledEffect> {
        assert_eq!(self.table.pixels.len(), (self.table.width * self.table.height) as usize);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hqx bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = compile_render_pipeline(device, target, effect_id, &bind_group_layout, PARTS, self.constants);

        let table_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hqx rule table"),
            size: wgpu::Extent3d {
                width: self.table.width,
                height: self.table.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &table_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(self.table.pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.table.width * std::mem::size_of::<u32>() as u32),
                rows_per_image: Some(self.table.height),
            },
            wgpu::Extent3d {
                width: self.table.width,
                height: self.table.height,
                depth_or_array_layers: 1,
            },
        );
        let table_view = table_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Box::new(CompiledHqx {
            pipeline,
            bind_group_layout,
            bind_group: None,
            texture_generation: None,
            _table_texture: table_texture,
            table_view,
        })
    }
}

#[derive(Debug)]
struct CompiledHqx {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    texture_generation: Option<u64>,
    _table_texture: wgpu::Texture,
    table_view: wgpu::TextureView,
}

impl CompiledEffect for CompiledHqx {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        framebuffer: &wgpu::TextureView,
        texture_generation: u64,
    ) {
        if self.texture_generation == Some(texture_generation) {
            return;
        }
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hqx bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(framebuffer),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.table_view),
                },
            ],
        }));
        self.texture_generation = Some(texture_generation);
    }

    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        let Some(bind_group) = &self.bind_group else {
            return;
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

static HQ2X_RENDERER: HqxRenderer = HqxRenderer {
    table: rules::HQ2X_TABLE,
    constants: &[("SCALE", 2.0)],
};
static HQ3X_RENDERER: HqxRenderer = HqxRenderer {
    table: rules::HQ3X_TABLE,
    constants: &[("SCALE", 3.0)],
};
static HQ4X_RENDERER: HqxRenderer = HqxRenderer {
    table: rules::HQ4X_TABLE,
    constants: &[("SCALE", 4.0)],
};

pub const HQ2X: Effect = Effect::new("hq2x", "hq2x", 2, &HQ2X_RENDERER);
pub const HQ3X: Effect = Effect::new("hq3x", "hq3x", 3, &HQ3X_RENDERER);
pub const HQ4X: Effect = Effect::new("hq4x", "hq4x", 4, &HQ4X_RENDERER);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rule_tables_are_well_formed() {
        for (scale, table) in [(2, rules::HQ2X_TABLE), (3, rules::HQ3X_TABLE), (4, rules::HQ4X_TABLE)] {
            let rule_len = 256 * scale * scale;
            assert_eq!(table.width, 256);
            assert_eq!(table.height as usize, scale * scale + 1);
            assert_eq!(table.pixels.len(), rule_len + 256);

            let (rules, recipes) = table.pixels.split_at(rule_len);
            for &rule in rules {
                assert!(((rule >> 16) & 0x7) <= 4);
                assert_eq!(rule >> 19, 0);
                for shift in [0, 8] {
                    let recipe = recipes[((rule >> shift) & 0xff) as usize];
                    assert!((recipe & 0xf) <= 10);
                    assert!(((recipe >> 4) & 0xf) <= 9);
                    assert!(((recipe >> 8) & 0xf) <= 9);
                    assert!(((recipe >> 12) & 0xf) <= 9);
                    assert_eq!(recipe >> 16, 0);
                }
            }
        }
    }
}
