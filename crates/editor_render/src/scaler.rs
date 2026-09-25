use wgpu::util::DeviceExt;
use crate::offscreen::{OFFSCREEN_FORMAT, OffscreenTarget, SceneTarget};
use crate::pipeline::CameraUniform;
use crate::scene::TileLight;

/// State das passadas pixel-art do RME de referência.
pub const MODE_OFF: u32 = 0;
pub const MODE_SMOOTH: u32 = 1;
pub const MODE_SUPER_2XSAI: u32 = 2;
pub const MODE_XBRZ: u32 = 3;

/// Uniform da passada "plain" (blit nearest + bilinear), mantido byte a byte
/// com o struct do scaler.wgsl.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ScaleUniform {
    source_size: [u32; 2],
    output_size: [u32; 2],
    source_cell_size: [f32; 2],
    output_cell_size: [f32; 2],
    mode: u32,
    _align_padding: u32,
    _padding: [u32; 2],
}

/// Uniform do fragPixelArtSrc (xBRZ) compilado com naga. Bloco std140:
/// `int uSourceCellSize; float uOutputCellSize; vec2 uTexSize;` = 16 bytes.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct XbrzUniform {
    source_cell_size: i32,
    output_cell_size: f32,
    tex_size: [f32; 2],
}

/// Uniform do compositeSuper2xSaiSrc. Bloco std140 com três vec2 = 32 bytes
/// (padding final para alinhamento de buffer uniform).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Super2xSaiUniform {
    texture_size: [f32; 2],
    output_size: [f32; 2],
    input_size: [f32; 2],
    _padding: [f32; 2],
}

/// Uniform dos shaders MDAPT e CRT bloom: tamanho da textura de origem +
/// parâmetros. Bloco std140 `vec2 TextureSize; vec2 Params;` = 16 bytes.
/// MDAPT só lê TextureSize; o bloom lê Params.x = WorldLight (0..1).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TexSizeUniform {
    texture_size: [f32; 2],
    params: [f32; 2],
}

pub struct ScaleResources {
    pipeline_plain: wgpu::RenderPipeline,
    pipeline_xbrz: wgpu::RenderPipeline,
    pipeline_super_2xsai: wgpu::RenderPipeline,
    pipeline_mdapt: [wgpu::RenderPipeline; 5],
    pipeline_crt_color: wgpu::RenderPipeline,
    pipeline_crt_bloom: wgpu::RenderPipeline,
    pipeline_linear_to_srgb: wgpu::RenderPipeline,
    pipeline_lights: wgpu::RenderPipeline,
    pipeline_apply_light: wgpu::RenderPipeline,
    layout_plain: wgpu::BindGroupLayout,
    layout_xbrz: wgpu::BindGroupLayout,
    layout_super_2xsai: wgpu::BindGroupLayout,
    layout_mdapt: wgpu::BindGroupLayout,
    layout_mdapt_dual: wgpu::BindGroupLayout,
    layout_crt_color: wgpu::BindGroupLayout,
    layout_crt_bloom: wgpu::BindGroupLayout,
    layout_linear_to_srgb: wgpu::BindGroupLayout,
    layout_lights: wgpu::BindGroupLayout,
    layout_apply_light: wgpu::BindGroupLayout,
    uniform_plain: wgpu::Buffer,
    uniform_xbrz: wgpu::Buffer,
    uniform_super_2xsai: wgpu::Buffer,
    uniform_tex_size: wgpu::Buffer,
    sampler: wgpu::Sampler,
    sampler_linear: wgpu::Sampler,
    /// Buffer de instâncias de luz (storage buffer, atualizado por frame)
    light_buffer: wgpu::Buffer,
    light_buffer_capacity: usize,
    /// Câmera dedicada do pass de luz (CameraUniform de 48 bytes reescrito por frame)
    light_camera_uniform: wgpu::Buffer,
    /// Textura do light buffer (Rgba8Unorm linear, resolução nativa da cena)
    light_texture: wgpu::Texture,
    light_texture_view: wgpu::TextureView,
    light_texture_size: (u32, u32),
}

impl ScaleResources {
    pub fn new(device: &wgpu::Device) -> Self {
        // Vertex comum à passada plain; o fragment vary só o filtro.
        let vertex_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_scaler_vertex"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scaler.wgsl").into()),
        });
        let plain_fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene_scaler_blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scaler.wgsl").into()),
        });
        // O frontend GLSL do naga marca as variáveis interpoladas com
        // sampling None; um vertex WGSL (sampling Center) queima a validação
        // de interface. As pipelines pixel-art usam um vertex também em GLSL
        // (triângulo em tela cheia), como na referência (compositeVertexSrc).
        let glsl_vertex = Self::glsl_shader(device, "scene_scaler_glsl_vertex", wgpu::naga::ShaderStage::Vertex, include_str!("../assets/fullscreen_triangle.vert"));
        // Modos 2/3: o GLSL do RME de referência compilado na hora pelo naga
        // (frontend glsl do próprio wgpu). O algoritmo permanece o texto da
        // referência; só o dialeto foi adaptado (330->450, texture2D+sampler
        // separados, uniforms em bloco std140).
        let xbrz_fragment = Self::glsl_shader(device, "xbrz", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/xbrz.frag"));
        let sai_fragment = Self::glsl_shader(device, "super2xsai", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/super_2xsai.frag"));
        // "Shaders" da aba: MDAPT (checkerboard dithering, 5 passadas) roda na
        // cena nativa antes do upscaling; CRT colour (P22) e CRT bloom rodam
        // depois, na imagem final. Todos compilados pelo frontend GLSL do naga.
        let mdapt0_fragment = Self::glsl_shader(device, "mdapt0", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/mdapt0.frag"));
        let mdapt1_fragment = Self::glsl_shader(device, "mdapt1", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/mdapt1.frag"));
        let mdapt2_fragment = Self::glsl_shader(device, "mdapt2", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/mdapt2.frag"));
        let mdapt3_fragment = Self::glsl_shader(device, "mdapt3", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/mdapt3.frag"));
        let mdapt4_fragment = Self::glsl_shader(device, "mdapt4", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/mdapt4.frag"));
        let crt_color_fragment = Self::glsl_shader(device, "crt_color", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/crt_color.frag"));
        let crt_bloom_fragment = Self::glsl_shader(device, "crt_bloom", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/crt_bloom.frag"));
        let linear_to_srgb_fragment = Self::glsl_shader(device, "linear_to_srgb", wgpu::naga::ShaderStage::Fragment, include_str!("../assets/linear_to_srgb.frag"));
        // Shader WGSL da iluminação: light_buffer (quads por fonte, blend Max)
        // e apply_light (multiplicação cena × light buffer em linear).
        let light_vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("light_vertex"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../assets/light_vertex.wgsl").into()),
        });
        let light_fragment = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("light_fragment"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../assets/light_fragment.wgsl").into()),
        });
        let apply_light_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("apply_light"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../assets/apply_light.wgsl").into()),
        });

        let layout_plain = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        // Interface dos shaders GLSL compilados: (0) textura, (1) sampler
        // separado, (2) bloco de uniforms. Cada shader tem seu próprio layout
        // porque o bloco tem tamanho diferente (xBRZ 16, 2xSaI 24 bytes).
        let layout_xbrz = Self::glsl_layout(device, "scene_scaler_glsl_xbrz_bgl", std::mem::size_of::<XbrzUniform>() as u64);
        let layout_super_2xsai = Self::glsl_layout(device, "scene_scaler_glsl_2xsai_bgl", std::mem::size_of::<Super2xSaiUniform>() as u64);
        // MDAPT (1 textura) e pós MRAPT (1 textura + 1 alvo).
        let layout_mdapt = Self::glsl_layout(device, "scene_scaler_mdapt_bgl", std::mem::size_of::<TexSizeUniform>() as u64);
        // MDAPT passos 3/4: duas texturas (Source + Original) + sampler cada.
        let layout_mdapt_dual = Self::glsl_layout_dual(device, "scene_scaler_mdapt_dual_bgl", std::mem::size_of::<TexSizeUniform>() as u64);
        // Pós na imagem final: amostragem linear (window do libretro usa GL
        // linear no bloom; o CRT colour é um shift por texel, tanto faz).
        let layout_crt_color = Self::glsl_layout_filter(device, "scene_scaler_crt_color_bgl", None);
        let layout_crt_bloom = Self::glsl_layout_filter(device, "scene_scaler_crt_bloom_bgl", Some(std::mem::size_of::<TexSizeUniform>() as u64));
        // Layout para conversão final linear→sRGB: textura + sampler linear, sem uniforms.
        let layout_linear_to_srgb = Self::glsl_layout_filter(device, "scene_scaler_linear_to_srgb_bgl", None);

        let uniform_plain = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_scaler_uniform"),
            contents: bytemuck::bytes_of(&ScaleUniform {
                source_size: [1, 1], output_size: [1, 1], source_cell_size: [1.0; 2], output_cell_size: [1.0; 2], mode: 0, _align_padding: 0, _padding: [0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_xbrz = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_scaler_xbrz_uniform"),
            contents: bytemuck::bytes_of(&XbrzUniform { source_cell_size: 1, output_cell_size: 1.0, tex_size: [1.0, 1.0] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_super_2xsai = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_scaler_2xsai_uniform"),
            contents: bytemuck::bytes_of(&Super2xSaiUniform { texture_size: [1.0, 1.0], output_size: [1.0, 1.0], input_size: [1.0, 1.0], _padding: [0.0; 2] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_tex_size = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_scaler_tex_size_uniform"),
            contents: bytemuck::bytes_of(&TexSizeUniform { texture_size: [1.0, 1.0], params: [0.0; 2] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Nearest: a cena composta tem a densidade do pixel-art; o 2xSaI amos-
        // tra com coordenadas deslocadas, mas quer o texel mais próximo.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene_scaler_nearest_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Linear para o halo do CRT bloom (radios fracionárias em texel, sem
        // aliasy em "leque"). O sampler precisa de texture filterable (o layout
        // correspondente é criado com filterable: true).
        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene_scaler_linear_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_plain = Self::pipeline(device, "scene_scaler_pipeline", &layout_plain, &vertex_module, "vs_main", &plain_fragment, "fs_main");
        let pipeline_xbrz = Self::pipeline(device, "scene_scaler_xbrz", &layout_xbrz, &glsl_vertex, "main", &xbrz_fragment, "main");
        let pipeline_super_2xsai = Self::pipeline(device, "scene_scaler_2xsai", &layout_super_2xsai, &glsl_vertex, "main", &sai_fragment, "main");
        let pipeline_mdapt = [
            Self::pipeline(device, "scene_scaler_mdapt0", &layout_mdapt, &glsl_vertex, "main", &mdapt0_fragment, "main"),
            Self::pipeline(device, "scene_scaler_mdapt1", &layout_mdapt, &glsl_vertex, "main", &mdapt1_fragment, "main"),
            Self::pipeline(device, "scene_scaler_mdapt2", &layout_mdapt, &glsl_vertex, "main", &mdapt2_fragment, "main"),
            Self::pipeline(device, "scene_scaler_mdapt3", &layout_mdapt_dual, &glsl_vertex, "main", &mdapt3_fragment, "main"),
            Self::pipeline(device, "scene_scaler_mdapt4", &layout_mdapt_dual, &glsl_vertex, "main", &mdapt4_fragment, "main"),
        ];
        let pipeline_crt_color = Self::pipeline(device, "scene_scaler_crt_color", &layout_crt_color, &glsl_vertex, "main", &crt_color_fragment, "main");
        let pipeline_crt_bloom = Self::pipeline(device, "scene_scaler_crt_bloom", &layout_crt_bloom, &glsl_vertex, "main", &crt_bloom_fragment, "main");
        let pipeline_linear_to_srgb = Self::pipeline(device, "scene_scaler_linear_to_srgb", &layout_linear_to_srgb, &glsl_vertex, "main", &linear_to_srgb_fragment, "main");

        // Layout da iluminação: (0) CameraUniform estático. As fontes de luz
        // entram como vertex buffer de instância (TileLight) — o backend GL
        // aqui tem limite 0 de storage buffers por shader.
        let layout_lights = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_light_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<CameraUniform>() as u64),
                    },
                    count: None,
                },
            ],
        });
        // pipeline_apply_light: (0) textura da cena, (1) sampler, (2) textura
        // do light buffer, (3) sampler — fullscreen triangle, sem uniforms.
        let layout_apply_light = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_apply_light_bgl"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        // pipeline_lights: alvo é o light buffer Rgba8Unorm (linear, sem
        // codificação sRGB); blend Max por canal soma o máximo das luzes.
        // Vertex = instância TileLight (TriangleStrip, 4 vértices por fonte).
        let light_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TileLight>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            // Offsets explícitos: TileLight tem um pad (align 16 do vec3 WGSL)
            // entre `intensity` (8) e `color` (16). vertex_attr_array empacota
            // offsets consecutivos e apontaria color para o pad errado.
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 8, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
            ],
        };
        let pipeline_lights_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene_light_pipeline_layout"),
            bind_group_layouts: &[&layout_lights],
            push_constant_ranges: &[],
        });
        let pipeline_lights = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene_light_pipeline"),
            layout: Some(&pipeline_lights_layout),
            vertex: wgpu::VertexState {
                module: &light_vertex,
                entry_point: "vs_main",
                buffers: &[light_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &light_fragment,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Max,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Max,
                        },
                    }),
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

        // pipeline_apply_light: multiplica cena × light buffer (mesma resolução).
        let apply_light_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene_apply_light_pipeline_layout"),
            bind_group_layouts: &[&layout_apply_light],
            push_constant_ranges: &[],
        });
        let pipeline_apply_light = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene_apply_light_pipeline"),
            layout: Some(&apply_light_layout),
            vertex: wgpu::VertexState {
                module: &apply_light_module,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &apply_light_module,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: OFFSCREEN_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Buffers e textura da iluminação. O vertex buffer de instâncias cresce
        // conforme a cena visível (capacidade em nº de luzes).
        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene_light_buffer"),
            size: (64 * std::mem::size_of::<TileLight>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let light_camera_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene_light_camera_uniform"),
            contents: bytemuck::bytes_of(&CameraUniform {
                offset: [0.0; 2], zoom: [1.0; 2], atlas_columns: 1, _align_pad: 0,
                viewport_size: [1.0, 1.0], floor_alpha: 1.0, sampling_mode: 0,
                light: 1.0, _pad_light: 0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene_light_texture"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let light_texture_view = light_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self { pipeline_plain, pipeline_xbrz, pipeline_super_2xsai, pipeline_mdapt, pipeline_crt_color, pipeline_crt_bloom, pipeline_linear_to_srgb, pipeline_lights, pipeline_apply_light, layout_plain, layout_xbrz, layout_super_2xsai, layout_mdapt, layout_mdapt_dual, layout_crt_color, layout_crt_bloom, layout_linear_to_srgb, layout_lights, layout_apply_light, uniform_plain, uniform_xbrz, uniform_super_2xsai, uniform_tex_size, sampler, sampler_linear, light_buffer, light_buffer_capacity: 64, light_camera_uniform, light_texture, light_texture_view, light_texture_size: (1, 1) }
    }

    fn glsl_layout(device: &wgpu::Device, label: &'static str, uniform_min_size: u64) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(uniform_min_size),
                    },
                    count: None,
                },
            ],
        })
    }

    /// Layout dos passos MDAPT 3/4, que leem Source + Original (2 texturas,
    /// cada uma com seu sampler) + bloco de uniforms.
    fn glsl_layout_dual(device: &wgpu::Device, label: &'static str, uniform_min_size: u64) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(uniform_min_size),
                    },
                    count: None,
                },
            ],
        })
    }

    /// Layout dos pós na imagem final: textura filterable + sampler Filtering
    /// (o halo do bloom amostra em offsets fracionários) + uniform opcional.
    fn glsl_layout_filter(device: &wgpu::Device, label: &'static str, uniform_min_size: Option<u64>) -> wgpu::BindGroupLayout {
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        if let Some(min_size) = uniform_min_size {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(min_size),
                },
                count: None,
            });
        }
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries })
    }

    fn glsl_shader(device: &wgpu::Device, label: &'static str, stage: wgpu::naga::ShaderStage, source: &'static str) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Glsl { shader: source.into(), stage, defines: wgpu::naga::FastHashMap::default() },
        })
    }

    fn pipeline(device: &wgpu::Device, label: &'static str, layout: &wgpu::BindGroupLayout, vertex: &wgpu::ShaderModule, vertex_entry: &'static str, fragment: &wgpu::ShaderModule, fragment_entry: &'static str) -> wgpu::RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label), bind_group_layouts: &[layout], push_constant_ranges: &[],
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: vertex, entry_point: vertex_entry, buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: fragment,
                entry_point: fragment_entry,
                targets: &[Some(wgpu::ColorTargetState { format: OFFSCREEN_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(), depth_stencil: None,
            multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
        })
    }

    /// Resolve a cena completa em uma única imagem. Nenhum filtro toca um
    /// sprite isolado; transparências e sobreposições já foram compostas.
    /// Cobre o modo Off (nearest) e o Retro (bilinear), aplicados diretamente
    /// sobre a cena com a razão final.
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, source: &SceneTarget, target: &OffscreenTarget, mode: u32, cells: ([f32; 2], [f32; 2])) {
        let (source_cell_size, output_cell_size) = cells;
        queue.write_buffer(&self.uniform_plain, 0, bytemuck::bytes_of(&ScaleUniform {
            source_size: [source.width, source.height], output_size: [target.width, target.height],
            source_cell_size, output_cell_size,
            mode, _align_padding: 0, _padding: [0; 2],
        }));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_bg"), layout: &self.layout_plain,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform_plain.as_entire_binding() },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_plain, &bind_group, &target.view, "scene_scaler_blit_pass");
    }

    /// Chain pixel-art do RME de referência (map_drawer.cpp, composite chain):
    /// a cena entra nativa (1 texel por pixel do mapa) e o filtro sobe em
    /// múltiplos inteiros — Super 2xSaI sempre 2×, xBRZ sempre 4× — para um
    /// alvo intermediário. O blit final (bilinear; nearest quando a razão é
    /// inteira) encaixa o resultado no tamanho da janela. O grid do filtro
    /// permanece inteiro em qualquer zoom, eliminando o tremor fracionário.
    #[allow(clippy::too_many_arguments)]
    pub fn render_chain(&self, device: &wgpu::Device, queue: &wgpu::Queue, source: &SceneTarget, filter: &SceneTarget, target: &OffscreenTarget, mode: u32, up_ratio: u32, cells: ([f32; 2], [f32; 2])) {
        let (source_cell_size, output_cell_size) = cells;
        let up = up_ratio as f32;

        let (pipeline, bind_group) = if mode == MODE_SUPER_2XSAI {
            queue.write_buffer(&self.uniform_super_2xsai, 0, bytemuck::bytes_of(&Super2xSaiUniform {
                texture_size: [source.width as f32, source.height as f32],
                output_size: [filter.width as f32, filter.height as f32],
                input_size: [source.width as f32, source.height as f32],
                _padding: [0.0; 2],
            }));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene_scaler_2xsai_bg"), layout: &self.layout_super_2xsai,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.uniform_super_2xsai.as_entire_binding() },
                ],
            });
            (&self.pipeline_super_2xsai, bind_group)
        } else {
            queue.write_buffer(&self.uniform_xbrz, 0, bytemuck::bytes_of(&XbrzUniform {
                source_cell_size: source_cell_size[0].round() as i32,
                output_cell_size: up,
                tex_size: [source.width as f32, source.height as f32],
            }));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene_scaler_xbrz_bg"), layout: &self.layout_xbrz,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.uniform_xbrz.as_entire_binding() },
                ],
            });
            (&self.pipeline_xbrz, bind_group)
        };
        self.encode_draw(device, queue, pipeline, &bind_group, &filter.view, "scene_scaler_filter_pass");

        // Blit final do resultado inteiro para o tamanho da janela.
        let ratio = output_cell_size[0] / up;
        let blit_mode = if (ratio - ratio.round()).abs() < 1e-3 { 0 } else { 1 };
        queue.write_buffer(&self.uniform_plain, 0, bytemuck::bytes_of(&ScaleUniform {
            source_size: [filter.width, filter.height], output_size: [target.width, target.height],
            source_cell_size: [1.0; 2], output_cell_size: [1.0; 2],
            mode: blit_mode, _align_padding: 0, _padding: [0; 2],
        }));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_blit_bg"), layout: &self.layout_plain,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&filter.view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform_plain.as_entire_binding() },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_plain, &bind_group, &target.view, "scene_scaler_blit_pass");
    }

    /// MDAPT (checkerboard dithering, Sp00kyFox) aplicado na cena nativa antes
    /// do upscaling, como no composite do RME de referência. Cinco passadas em
    /// um único encoder; o resultado final fica em `t[0]`. `scene` não muda
    /// (é a "Original" dos passos 3/4).
    pub fn render_mdapt(&self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &SceneTarget, t: [&SceneTarget; 4]) {
        queue.write_buffer(&self.uniform_tex_size, 0, bytemuck::bytes_of(&TexSizeUniform {
            texture_size: [scene.width as f32, scene.height as f32],
            params: [0.0; 2],
        }));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scene_scaler_mdapt_pass") });

        let bg0 = self.mdapt_bg(device, scene);
        self.draw_pass(&mut encoder, &self.pipeline_mdapt[0], &bg0, &t[0].view, "scene_scaler_mdapt_pass0");
        let bg1 = self.mdapt_bg(device, t[0]);
        self.draw_pass(&mut encoder, &self.pipeline_mdapt[1], &bg1, &t[1].view, "scene_scaler_mdapt_pass1");
        let bg2 = self.mdapt_bg(device, t[1]);
        self.draw_pass(&mut encoder, &self.pipeline_mdapt[2], &bg2, &t[2].view, "scene_scaler_mdapt_pass2");
        let bg3 = self.mdapt_dual_bg(device, t[2], scene);
        self.draw_pass(&mut encoder, &self.pipeline_mdapt[3], &bg3, &t[3].view, "scene_scaler_mdapt_pass3");
        let bg4 = self.mdapt_dual_bg(device, t[3], scene);
        self.draw_pass(&mut encoder, &self.pipeline_mdapt[4], &bg4, &t[0].view, "scene_scaler_mdapt_pass4");

        queue.submit(Some(encoder.finish()));
    }

    /// CRT bloom (halo de fósforo, sem scanlines) aplicado na imagem final já
    /// upscaled. Amostra com o sampler linear (offsets de halo fracionários).
    /// `world_light` em [0,1] (metodo Tibia): menor valor = glow mais ativo
    /// (estilo neon 80s), maior valor = glow atenuado (sem embranquecer).
    pub fn post_bloom(&self, device: &wgpu::Device, queue: &wgpu::Queue,
                      source_size: (u32, u32), source: &wgpu::TextureView,
                      target: &wgpu::TextureView, world_light: f32) {
        queue.write_buffer(&self.uniform_tex_size, 0, bytemuck::bytes_of(&TexSizeUniform {
            texture_size: [source_size.0 as f32, source_size.1 as f32],
            params: [world_light, 0.0],
        }));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_bloom_bg"), layout: &self.layout_crt_bloom,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_linear) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_tex_size.as_entire_binding() },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_crt_bloom, &bind_group, target, "scene_scaler_bloom_pass");
    }

    /// CRT colour: fosforo P22 (NTSC D65) aplicado fielmente à imagem final.
    pub fn post_color(&self, device: &wgpu::Device, queue: &wgpu::Queue, source: &wgpu::TextureView, target: &wgpu::TextureView) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_crt_color_bg"), layout: &self.layout_crt_color,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_linear) },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_crt_color, &bind_group, target, "scene_scaler_crt_color_pass");
    }

    /// Conversão final linear → sRGB para apresentação no egui.
    /// O pipeline roda todo em linear; esta passagem converte para sRGB antes de exibir.
    pub fn post_srgb(&self, device: &wgpu::Device, queue: &wgpu::Queue, source: &wgpu::TextureView, target: &wgpu::TextureView) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_linear_to_srgb_bg"), layout: &self.layout_linear_to_srgb,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler_linear) },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_linear_to_srgb, &bind_group, target, "scene_scaler_linear_to_srgb_pass");
    }

    /// Pass de luz (método OTClient, adaptado para quads por fonte): limpa o
    /// light buffer com a cor ambiente e desenha cada fonte como um quad de
    /// raio `intensity` tiles com falloff linear (lightview.cpp:updatePixels),
    /// composto por máximo por canal (blend Max). O alvo é Rgba8Unorm linear
    /// na resolução nativa da cena. `ambient` é a cor ambiente 0..1 (piso 15%).
    #[allow(clippy::too_many_arguments)]
    pub fn render_lights(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, camera: &CameraUniform, lights: &[TileLight], scene_size: (u32, u32), ambient: [f32; 3]) {
        // Garante textura do light buffer no tamanho da cena nativa.
        if self.light_texture_size != scene_size {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene_light_texture"),
                size: wgpu::Extent3d { width: scene_size.0.max(1), height: scene_size.1.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.light_texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.light_texture = texture;
            self.light_texture_size = scene_size;
        }

        // Cresce o vertex buffer de instâncias conforme o nº de luzes visíveis.
        let byte_len = std::mem::size_of_val(lights);
        if lights.len() > self.light_buffer_capacity {
            self.light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene_light_buffer"),
                size: byte_len.max(64) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.light_buffer_capacity = byte_len.max(64) / std::mem::size_of::<TileLight>();
        }

        queue.write_buffer(&self.light_camera_uniform, 0, bytemuck::bytes_of(camera));
        if !lights.is_empty() {
            queue.write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(lights));
        }

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_light_bg"), layout: &self.layout_lights,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.light_camera_uniform.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scene_light_pass") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene_light_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.light_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: ambient[0] as f64, g: ambient[1] as f64, b: ambient[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline_lights);
            pass.set_bind_group(0, &bind_group, &[]);
            if !lights.is_empty() {
                pass.set_vertex_buffer(0, self.light_buffer.slice(..));
                pass.draw(0..4, 0..lights.len() as u32);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Multiplica a cena composta pelo light buffer (mesma resolução nativa),
    /// produzindo a cena iluminada fora do alvo original (sem feedback de
    /// leitura/escrita no mesmo alvo).
    pub fn apply_light(&self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &wgpu::TextureView, target: &wgpu::TextureView) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_apply_light_bg"), layout: &self.layout_apply_light,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scene) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.light_texture_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_apply_light, &bind_group, target, "scene_apply_light_pass");
    }

    /// Cópia 1:1 nearest de um alvo para outro (pós usam um alvo intermediário;
    /// o resultado volta ao target de apresentação).
    pub fn copy_scene(&self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32), source: &wgpu::TextureView, target: &wgpu::TextureView) {
        queue.write_buffer(&self.uniform_plain, 0, bytemuck::bytes_of(&ScaleUniform {
            source_size: [size.0, size.1], output_size: [size.0, size.1],
            source_cell_size: [1.0; 2], output_cell_size: [1.0; 2],
            mode: 0, _align_padding: 0, _padding: [0; 2],
        }));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_copy_bg"), layout: &self.layout_plain,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform_plain.as_entire_binding() },
            ],
        });
        self.encode_draw(device, queue, &self.pipeline_plain, &bind_group, target, "scene_scaler_copy_pass");
    }

    fn mdapt_bg(&self, device: &wgpu::Device, src: &SceneTarget) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_mdapt_bg"), layout: &self.layout_mdapt,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_tex_size.as_entire_binding() },
            ],
        })
    }

    fn mdapt_dual_bg(&self, device: &wgpu::Device, src: &SceneTarget, original: &SceneTarget) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_scaler_mdapt_dual_bg"), layout: &self.layout_mdapt_dual,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&original.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: self.uniform_tex_size.as_entire_binding() },
            ],
        })
    }

    fn draw_pass(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, bind_group: &wgpu::BindGroup, target: &wgpu::TextureView, label: &'static str) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_draw(&self, device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &wgpu::RenderPipeline, bind_group: &wgpu::BindGroup, target: &wgpu::TextureView, label: &'static str) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}