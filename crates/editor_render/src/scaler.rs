use wgpu::util::DeviceExt;
use crate::offscreen::{OFFSCREEN_FORMAT, OffscreenTarget, SceneTarget};

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ScaleUniform {
    source_size: [u32; 2],
    output_size: [u32; 2],
    source_pixel_scale: f32,
    mode: u32,
    _padding: [u32; 2],
}

pub struct ScaleResources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

impl ScaleResources {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_scaler_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scaler.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_scaler_bgl"),
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ScaleUniform>() as u64),
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_scaler_uniform"),
            contents: bytemuck::bytes_of(&ScaleUniform {
                source_size: [1, 1], output_size: [1, 1], source_pixel_scale: 1.0, mode: 0, _padding: [0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene_scaler_pipeline_layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene_scaler_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_main", buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState { format: OFFSCREEN_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(), depth_stencil: None,
            multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
        });
        Self { pipeline, layout, uniform }
    }

    /// Resolve a cena completa em uma única imagem. Nenhum filtro toca um
    /// sprite isolado; transparências e sobreposições já foram compostas.
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, source: &SceneTarget, target: &OffscreenTarget, mode: u32, source_pixel_scale: f32) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&ScaleUniform {
            source_size: [source.width, source.height], output_size: [target.width, target.height],
            source_pixel_scale, mode, _padding: [0; 2],
        }));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_bg"), layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform.as_entire_binding() },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scene_scaler_encoder") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene_scaler_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
