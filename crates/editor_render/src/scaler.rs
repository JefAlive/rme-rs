use wgpu::util::DeviceExt;
use crate::offscreen::{OFFSCREEN_FORMAT, OffscreenTarget, SceneTarget};

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

pub struct ScaleResources {
    pipeline_plain: wgpu::RenderPipeline,
    pipeline_xbrz: wgpu::RenderPipeline,
    pipeline_super_2xsai: wgpu::RenderPipeline,
    layout_plain: wgpu::BindGroupLayout,
    layout_xbrz: wgpu::BindGroupLayout,
    layout_super_2xsai: wgpu::BindGroupLayout,
    uniform_plain: wgpu::Buffer,
    uniform_xbrz: wgpu::Buffer,
    uniform_super_2xsai: wgpu::Buffer,
    sampler: wgpu::Sampler,
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

        let pipeline_plain = Self::pipeline(device, "scene_scaler_pipeline", &layout_plain, &vertex_module, "vs_main", &plain_fragment, "fs_main");
        let pipeline_xbrz = Self::pipeline(device, "scene_scaler_xbrz", &layout_xbrz, &glsl_vertex, "main", &xbrz_fragment, "main");
        let pipeline_super_2xsai = Self::pipeline(device, "scene_scaler_2xsai", &layout_super_2xsai, &glsl_vertex, "main", &sai_fragment, "main");

        Self { pipeline_plain, pipeline_xbrz, pipeline_super_2xsai, layout_plain, layout_xbrz, layout_super_2xsai, uniform_plain, uniform_xbrz, uniform_super_2xsai, sampler }
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