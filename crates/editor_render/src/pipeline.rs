use wgpu::util::DeviceExt;
use crate::instance::TileInstance;

/// Máximo de andares desenhados num frame. A regra do RME nunca produz mais
/// que 8 (rooftop stack completo ou subsolo+2), 16 dá folga sem desperdício.
pub const MAX_FLOOR_LAYERS: usize = 16;

pub struct TileRenderResources {
    pub pipeline: wgpu::RenderPipeline,
    pub quad_vbuf: wgpu::Buffer,
    pub camera_buf: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    /// Stride (bytes) entre entradas consecutivas no camera_buf, já alinhado
    /// a `min_uniform_buffer_offset_alignment` do device.
    pub camera_stride: u32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub offset: [f32; 2],
    pub zoom: [f32; 2],
    pub atlas_columns: u32,
    pub _align_pad: u32,
    pub viewport_size: [f32; 2],
    pub floor_alpha: f32,
    /// 0 nearest, 1 sharp bilinear, 2 Super 2xSaI, 3 xBRZ.
    pub sampling_mode: u32,
}

impl TileRenderResources {
    /// `target_format` precisa ser o formato da textura OFFSCREEN, não o da swapchain.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat, atlas_bgl: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // Uma entrada de CameraUniform por andar desenhado no frame, selecionada
        // via dynamic offset — necessário porque vários draw calls no mesmo
        // encoder não podem depender de reescritas sequenciais do MESMO byte
        // range (só a última escrita sobreviveria até o submit).
        let align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let unpadded = std::mem::size_of::<CameraUniform>() as u64;
        let camera_stride = unpadded.div_ceil(align) * align;

        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_uniform"),
            size: camera_stride * MAX_FLOOR_LAYERS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(unpadded),
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bg"),
            layout: &camera_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &camera_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(unpadded),
                }),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tile_pipeline_layout"),
            bind_group_layouts: &[&camera_bgl, atlas_bgl],
            push_constant_ranges: &[],
        });

        let quad_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_vbuf"),
            contents: bytemuck::cast_slice(&[[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_layout = wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[quad_layout, TileInstance::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { pipeline, quad_vbuf, camera_buf, camera_bind_group, camera_stride: camera_stride as u32 }
    }
}
