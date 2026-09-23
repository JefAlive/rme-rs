use ahash::AHashMap;

pub const ATLAS_SIZE: u32 = 4096;
pub const SPRITE_SIZE: u32 = 32;
pub const ATLAS_COLS: u32 = ATLAS_SIZE / SPRITE_SIZE; // 128
pub const ATLAS_SLOTS: u32 = ATLAS_COLS * ATLAS_COLS; // 16384

/// Dynamic 2D sprite atlas.
///
/// Textura 2D (4096×4096) alocada na GPU dividida em slots 32×32.
/// Mantém um cache em memória `sprite_id -> slot`. Se encher, sobrescreve o mais antigo (FIFO).
pub struct SpriteAtlas {
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    texture: wgpu::Texture,
    sprite_to_slot: AHashMap<u32, u32>,
    slot_to_sprite: Vec<u32>,
    next_slot: u32,
    total_slots: u32,
    cols: u32,
}

impl SpriteAtlas {
    /// Cria o dynamic atlas 2D na GPU (slot 0 reservado como transparente).
    pub fn new(device: &wgpu::Device) -> Self {
        let total_slots = ATLAS_SLOTS;
        let cols = ATLAS_COLS;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite_atlas_2d"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("sprite_atlas_view"),
            dimension: Some(wgpu::TextureViewDimension::D2),
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

        let slot_to_sprite = vec![0u32; total_slots as usize];
        let sprite_to_slot = AHashMap::new();

        Self {
            bind_group,
            bind_group_layout,
            texture,
            sprite_to_slot,
            slot_to_sprite,
            next_slot: 1, // slot 0 reservado transparente/vazio
            total_slots,
            cols,
        }
    }

    /// Retorna a posição do sprite no atlas se já estiver em cache.
    pub fn get_slot(&self, sprite_id: u32) -> Option<u32> {
        if sprite_id == 0 {
            return Some(0);
        }
        self.sprite_to_slot.get(&sprite_id).copied()
    }

    /// Aloca um slot para o sprite e grava seus pixels na textura GPU via `queue.write_texture`.
    /// Se o atlas encher, o slot mais antigo é sobrescrito (ring buffer / FIFO).
    pub fn insert(&mut self, queue: &wgpu::Queue, sprite_id: u32, rgba: &[u8; 32 * 32 * 4]) -> u32 {
        if sprite_id == 0 {
            return 0;
        }
        if let Some(&slot) = self.sprite_to_slot.get(&sprite_id) {
            return slot;
        }

        let slot = self.next_slot;
        self.next_slot += 1;
        if self.next_slot >= self.total_slots {
            self.next_slot = 1; // substitui os mais antigos
        }

        // Se o slot já possuía outro sprite, remove do mapa
        let old_sprite = self.slot_to_sprite[slot as usize];
        if old_sprite != 0 && old_sprite != sprite_id {
            self.sprite_to_slot.remove(&old_sprite);
        }
        self.slot_to_sprite[slot as usize] = sprite_id;
        self.sprite_to_slot.insert(sprite_id, slot);

        let col = slot % self.cols;
        let row = slot / self.cols;

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: col * SPRITE_SIZE,
                    y: row * SPRITE_SIZE,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(SPRITE_SIZE * 4),
                rows_per_image: Some(SPRITE_SIZE),
            },
            wgpu::Extent3d {
                width: SPRITE_SIZE,
                height: SPRITE_SIZE,
                depth_or_array_layers: 1,
            },
        );

        slot
    }

    /// Retorna a quantidade de sprites atualmente mantidos em cache.
    pub fn layer_count(&self) -> u32 {
        self.sprite_to_slot.len().max(1) as u32
    }

    /// Total de slots disponíveis no atlas 2D.
    pub fn max_layers(&self) -> u32 {
        self.total_slots
    }
}