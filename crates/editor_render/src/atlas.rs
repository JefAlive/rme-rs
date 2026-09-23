

/// Sprite atlas textura em array 2D com crescimento dinâmico.
///
/// Cada camada contém um quadrado 32×32 de pixels RGBA.
/// O atlas cresce automaticamente quando necessário (dobrando a capacidade).
pub struct SpriteAtlas {
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    texture: wgpu::Texture,
    cells: Vec<[u8; 32 * 32 * 4]>,
    capacity: u32,
}

impl SpriteAtlas {
    /// Cria um atlas vazio com 1 camada (transparente).
    pub fn new(device: &wgpu::Device) -> Self {
        let capacity = 1;
        let cells = vec![[0u8; 32 * 32 * 4]]; // camada 0 transparente
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite_atlas"),
            size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: capacity },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Nearest, // pixel art — sem blur
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bg"), layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        Self { bind_group, bind_group_layout, texture, cells, capacity }
    }

    /// Retorna o número atual de camadas (sprites) no atlas.
    pub fn layer_count(&self) -> u32 {
        self.cells.len() as u32
    }

    /// Anexa uma nova célula RGBA (32×32) ao atlas, retornando o índice da camada.
    /// O atlas cresce automaticamente se a capacidade atual for insuficiente.
    pub fn append(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8; 32 * 32 * 4]) -> u32 {
        let layer = self.cells.len() as u32;

        if layer == self.capacity {
            // Dobrar a capacidade
            let new_cap = self.capacity.checked_mul(2).unwrap_or(1).max(1);

            let new_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sprite_atlas"),
                size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: new_cap },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            // Copiar células existentes
            for (i, &cell) in self.cells.iter().enumerate() {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &new_texture, mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &cell,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(32 * 4),
                        rows_per_image: Some(32),
                    },
                    wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: 1 },
                );
            }

            let new_view = new_texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });

            let new_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("atlas_sampler"),
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });

            let new_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("atlas_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

            let _new_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atlas_bg"), layout: &new_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&new_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&new_sampler) },
                ],
            });

            // Garantir que as células extras sejam zeradas
            while self.cells.len() < new_cap as usize {
                self.cells.push([0u8; 32 * 32 * 4]);
            }

            self.texture = new_texture;
            self.capacity = new_cap;
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atlas_bg"), layout: &new_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&new_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&new_sampler) },
                ],
            });
        }

        let idx = layer;
        self.cells.push(*rgba);

        // Atualizar capacidade se necessário
        if self.cells.len() > self.capacity as usize {
            self.capacity = self.cells.len() as u32;
        }

        idx
    }
}