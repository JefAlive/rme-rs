use bytemuck::{Pod, Zeroable};

/// Uma entrada por *tipo de item* (estático ou animado), nunca por
/// instância — poucas centenas de entradas mesmo com milhares de tiles.
/// `mode`: 0 = síncrono (todas as instâncias do tipo animam em fase),
/// 1 = assíncrono (fase deslocada por hash de posição, no shader).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct AnimEntry {
    pub first_frame: u32,
    pub frame_count: u32,
    pub frame_duration_ms: u32,
    pub mode: u32,
}

/// Duas storage buffers: `entries` (uma por tipo resolvido) e `frames`
/// (índices de layer do atlas, concatenados por entrada). Cresce sob
/// demanda dobrando a capacidade — mesmo padrão do `SpriteAtlas`.
pub struct AnimTable {
    entries: Vec<AnimEntry>,
    frames: Vec<u32>,
    entries_buf: wgpu::Buffer,
    frames_buf: wgpu::Buffer,
    entries_cap: u32,
    frames_cap: u32,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

pub const INITIAL_ENTRIES_CAP: u32 = 8192;
pub const INITIAL_FRAMES_CAP: u32 = 32768;

impl AnimTable {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let entries_buf = Self::make_buf(
            device, "anim_entries",
            INITIAL_ENTRIES_CAP as u64 * std::mem::size_of::<AnimEntry>() as u64,
        );
        let frames_buf = Self::make_buf(device, "anim_frames", INITIAL_FRAMES_CAP as u64 * 4);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("anim_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = Self::make_bind_group(device, &bind_group_layout, &entries_buf, &frames_buf);

        let mut table = Self {
            entries: Vec::new(), frames: Vec::new(),
            entries_buf, frames_buf,
            entries_cap: INITIAL_ENTRIES_CAP, frames_cap: INITIAL_FRAMES_CAP,
            bind_group_layout, bind_group,
        };

        // anim_id 0 reservado: 1 frame apontando para a layer 0 (transparente
        // no atlas) — sentinela para type_id desconhecido/vazio.
        table.push_static(device, queue, 0);
        table
    }

    fn make_buf(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size.max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn make_bind_group(
        device: &wgpu::Device, layout: &wgpu::BindGroupLayout,
        entries_buf: &wgpu::Buffer, frames_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("anim_bg"), layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: entries_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: frames_buf.as_entire_binding() },
            ],
        })
    }

    fn ensure_entries_capacity(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, needed: u32) {
        if needed <= self.entries_cap { return; }
        let mut cap = self.entries_cap;
        while cap < needed { cap = cap.saturating_mul(2); }
        let new_buf = Self::make_buf(device, "anim_entries", cap as u64 * std::mem::size_of::<AnimEntry>() as u64);
        queue.write_buffer(&new_buf, 0, bytemuck::cast_slice(&self.entries));
        self.entries_buf = new_buf;
        self.entries_cap = cap;
        self.bind_group = Self::make_bind_group(device, &self.bind_group_layout, &self.entries_buf, &self.frames_buf);
    }

    fn ensure_frames_capacity(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, needed: u32) {
        if needed <= self.frames_cap { return; }
        let mut cap = self.frames_cap;
        while cap < needed { cap = cap.saturating_mul(2); }
        let new_buf = Self::make_buf(device, "anim_frames", cap as u64 * 4);
        queue.write_buffer(&new_buf, 0, bytemuck::cast_slice(&self.frames));
        self.frames_buf = new_buf;
        self.frames_cap = cap;
        self.bind_group = Self::make_bind_group(device, &self.bind_group_layout, &self.entries_buf, &self.frames_buf);
    }

    /// Registra um item estático (1 frame) apontando para `layer`.
    pub fn push_static(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layer: u32) -> u32 {
        self.push_animated(device, queue, &[layer], 1, false)
    }

    /// Registra um item animado (N frames), devolve o `anim_id`.
    pub fn push_animated(
        &mut self, device: &wgpu::Device, queue: &wgpu::Queue,
        frame_layers: &[u32], frame_duration_ms: u32, async_mode: bool,
    ) -> u32 {
        let first_frame = self.frames.len() as u32;
        self.ensure_frames_capacity(device, queue, first_frame + frame_layers.len() as u32);
        self.frames.extend_from_slice(frame_layers);
        queue.write_buffer(&self.frames_buf, first_frame as u64 * 4, bytemuck::cast_slice(frame_layers));

        let id = self.entries.len() as u32;
        self.ensure_entries_capacity(device, queue, id + 1);
        let entry = AnimEntry {
            first_frame,
            frame_count: frame_layers.len().max(1) as u32,
            frame_duration_ms: frame_duration_ms.max(1),
            mode: if async_mode { 1 } else { 0 },
        };
        self.entries.push(entry);
        queue.write_buffer(
            &self.entries_buf,
            id as u64 * std::mem::size_of::<AnimEntry>() as u64,
            bytemuck::cast_slice(&[entry]),
        );
        id
    }
}