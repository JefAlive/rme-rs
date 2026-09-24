use ahash::AHashMap;

pub const SPRITE_SIZE: u32 = 32;

/// Limite desejado de layers. O limite real é o `max_texture_array_layers`
/// do adapter (Vulkan/MTL/DX12 geralmente 2048; GL mínimo 256), então
/// clampamos com `min`.
const DESIRED_LAYERS: u32 = 2048;

/// "Atlas" degradado — TESTE TEMPORÁRIO, sem otimização.
///
/// Array de texturas na GPU com UMA layer 32×32 por sprite: o índice do
/// sprite É a própria layer (sem packing, sem evicção FIFO, sem coordenada
/// de slot). O shader resolve a layer e amostra direto com `texture_2d_array`.
///
/// A única "inteligência" mantida é a deduplicação sprite_id → layer
/// (guardada em `sprite_to_layer`), que é corretude, não otimização: sem ela
/// cada tile reescreveria milhões de cópias do mesmo sprite.
pub struct SpriteAtlas {
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    texture: wgpu::Texture,
    // Mantidos vivos deliberadamente: o bind group referencia essas views.
    #[allow(dead_code)]
    view: wgpu::TextureView,
    #[allow(dead_code)]
    sampler: wgpu::Sampler,
    capacity: u32,
    next_layer: u32,
    sprite_to_layer: AHashMap<u32, u32>,
}

impl SpriteAtlas {
    /// Cria o array de texturas na GPU (layer 0 reservada transparente).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let capacity = device.limits().max_texture_array_layers.min(DESIRED_LAYERS);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite_array"),
            size: wgpu::Extent3d {
                width: SPRITE_SIZE,
                height: SPRITE_SIZE,
                depth_or_array_layers: capacity,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("sprite_array_view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_array_layer: 0,
            array_layer_count: Some(capacity),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let table = Self {
            bind_group,
            bind_group_layout,
            texture,
            view,
            sampler,
            capacity,
            next_layer: 1, // layer 0 reservada transparente/vazia
            sprite_to_layer: AHashMap::new(),
        };
        // Zero a layer 0 (transparente) — sentinela para sprite desconhecido.
        let zeros = [0u8; (SPRITE_SIZE * SPRITE_SIZE * 4) as usize];
        table.write_layer(queue, 0, &zeros);
        table
    }

    /// Retorna a layer do sprite no array de texturas.
    pub fn get_slot(&self, sprite_id: u32) -> Option<u32> {
        if sprite_id == 0 {
            return Some(0);
        }
        self.sprite_to_layer.get(&sprite_id).copied()
    }

    /// Adiciona o sprite como uma nova layer (ou reusa a já existente), e
    /// grava seus pixels na GPU via `queue.write_texture`.
    pub fn insert(&mut self, queue: &wgpu::Queue, sprite_id: u32, rgba: &[u8; 32 * 32 * 4]) -> u32 {
        if sprite_id == 0 {
            return 0;
        }
        if let Some(&layer) = self.sprite_to_layer.get(&sprite_id) {
            return layer;
        }

        let layer = self.next_layer;
        if layer >= self.capacity {
            eprintln!(
                "[atlas/TESTE] capacidade de layers excedida ({layer} >= {}) sprite={sprite_id} — \
                 desenhando transparente. Aumente DESIRED_LAYERS em atlas.rs.",
                self.capacity
            );
            return 0;
        }
        self.next_layer += 1;
        self.sprite_to_layer.insert(sprite_id, layer);
        self.write_layer(queue, layer, rgba);
        layer
    }

    fn write_layer(&self, queue: &wgpu::Queue, layer: u32, rgba: &[u8]) {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(SPRITE_SIZE * 4),
                rows_per_image: Some(SPRITE_SIZE),
            },
            wgpu::Extent3d { width: SPRITE_SIZE, height: SPRITE_SIZE, depth_or_array_layers: 1 },
        );
    }

    /// Número de sprites atualmente em cache (exclui a layer transparente).
    pub fn layer_count(&self) -> u32 {
        (self.next_layer - 1).max(1)
    }

    /// TESTE TEMPORÁRIO (debug): acesso direto à textura p/ readback headless.
    pub fn debug_texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// TESTE TEMPORÁRIO (debug): layer indices inseridos, p/ conferência CPU.
    pub fn debug_sprite_layer(&self) -> &AHashMap<u32, u32> {
        &self.sprite_to_layer
    }

    /// Total de layers disponíveis no array de texturas.
    pub fn max_layers(&self) -> u32 {
        self.capacity
    }
}