use bytemuck::{Pod, Zeroable};

/// Uma entrada por *tipo de item* (estático ou animado), nunca por
/// instância — poucas centenas/milhares de entradas mesmo com milhares de tiles.
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

/// Capacidade FIXA, pré-alocada por completo desde a criação — precisa bater
/// exatamente com as constantes `MAX_ANIM_ENTRIES`/`MAX_ANIM_FRAMES` no
/// shader.wgsl. Arrays de tamanho dinâmico em storage buffers exigem a
/// feature GPU DYNAMIC_ARRAY_SIZE, que o backend GLES não suporta; por isso
/// alocamos o máximo realista de uma vez e NUNCA recriamos o buffer.
///
/// 65536 = u16::MAX + 1, cobrindo TODO o espaço possível de `type_id` — um
/// `anim_id` (que é atribuído 1:1 por type_id via `anim_cache`) jamais pode
/// ultrapassar essa capacidade. Isso elimina de vez a classe de bug onde
/// tipos além da capacidade colapsavam todos no mesmo slot (o "efeito 8192"
/// que causava sprites errados/trocados de forma aparentemente aleatória,
/// já que a ordem de atribuição de anim_id segue a ordem de iteração de um
/// AHashMap, não a ordem numérica do type_id).
pub const MAX_ANIM_ENTRIES: u32 = 65536;
pub const MAX_ANIM_FRAMES: u32 = 262144;

pub struct AnimTable {
    entries_len: u32,
    frames_len: u32,
    entries_buf: wgpu::Buffer,
    frames_buf: wgpu::Buffer,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

impl AnimTable {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let entries_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("anim_entries"),
            size: MAX_ANIM_ENTRIES as u64 * std::mem::size_of::<AnimEntry>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let frames_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("anim_frames"),
            size: MAX_ANIM_FRAMES as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("anim_bg"), layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: entries_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: frames_buf.as_entire_binding() },
            ],
        });

        let mut table = Self {
            entries_len: 0,
            frames_len: 0,
            entries_buf,
            frames_buf,
            bind_group_layout,
            bind_group,
        };

        // anim_id 0 reservado: 1 frame apontando para o slot 0 (transparente
        // no atlas) — sentinela para type_id desconhecido/vazio.
        table.push_static(queue, 0);
        table
    }

    /// Registra um item estático (1 frame) apontando para `slot` do atlas.
    pub fn push_static(&mut self, queue: &wgpu::Queue, slot: u32) -> u32 {
        self.push_animated(queue, &[slot], 1, false)
    }

    /// Registra um item animado (N frames), devolve o `anim_id`.
    /// Loga um erro (sem travar) se a capacidade fixa for excedida — dado
    /// que MAX_ANIM_ENTRIES cobre todo o espaço de u16, isso não deveria
    /// acontecer nunca em uso real; se aparecer no console, é sinal de bug
    /// em outro lugar (ex: anim_cache não estar deduplicando corretamente).
    pub fn push_animated(
        &mut self, queue: &wgpu::Queue,
        frame_layers: &[u32], frame_duration_ms: u32, async_mode: bool,
    ) -> u32 {
        let first_frame = self.frames_len;
        let needed_frames = first_frame + frame_layers.len() as u32;
        if needed_frames > MAX_ANIM_FRAMES {
            eprintln!(
                "[anim] ERRO: capacidade de frames excedida ({needed_frames} > {MAX_ANIM_FRAMES}); \
                 aumente MAX_ANIM_FRAMES em anim.rs e shader.wgsl"
            );
            return 0;
        }
        queue.write_buffer(&self.frames_buf, first_frame as u64 * 4, bytemuck::cast_slice(frame_layers));
        self.frames_len = needed_frames;

        let id = self.entries_len;
        if id >= MAX_ANIM_ENTRIES {
            eprintln!(
                "[anim] ERRO: capacidade de entradas excedida ({id} >= {MAX_ANIM_ENTRIES}); \
                 isso não deveria acontecer (MAX_ANIM_ENTRIES cobre todo u16). Verifique anim_cache."
            );
            return 0;
        }
        let entry = AnimEntry {
            first_frame,
            frame_count: frame_layers.len().max(1) as u32,
            frame_duration_ms: frame_duration_ms.max(1),
            mode: if async_mode { 1 } else { 0 },
        };
        queue.write_buffer(
            &self.entries_buf,
            id as u64 * std::mem::size_of::<AnimEntry>() as u64,
            bytemuck::cast_slice(&[entry]),
        );
        self.entries_len += 1;
        id
    }

    /// TESTE TEMPORÁRIO (debug): conteúdo do entries/frames p/ conferência CPU.
    pub fn debug_readback(&self, device: &wgpu::Device, queue: &wgpu::Queue, max_frames: u32) -> (Vec<AnimEntry>, Vec<u32>) {
        let entries = self.entries_len;
        let frames = self.frames_len.min(max_frames);
        let mut ebuf = vec![0u8; (entries as usize) * std::mem::size_of::<AnimEntry>()];
        let mut fbuf = vec![0u8; (frames as usize) * 4];
        for (slot, data) in [(&self.entries_buf, &mut ebuf), (&self.frames_buf, &mut fbuf)] {
            let size = data.len() as u64;
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("anim_debug"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.copy_buffer_to_buffer(slot, 0, &staging, 0, size);
            queue.submit([enc.finish()]);
            let slice = staging.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            data.copy_from_slice(&slice.get_mapped_range());
        }
        let mut entries_v = Vec::with_capacity(entries as usize);
        for i in 0..entries as usize {
            let off = i * std::mem::size_of::<AnimEntry>();
            entries_v.push(AnimEntry {
                first_frame: u32::from_le_bytes(ebuf[off..off + 4].try_into().unwrap()),
                frame_count: u32::from_le_bytes(ebuf[off + 4..off + 8].try_into().unwrap()),
                frame_duration_ms: u32::from_le_bytes(ebuf[off + 8..off + 12].try_into().unwrap()),
                mode: u32::from_le_bytes(ebuf[off + 12..off + 16].try_into().unwrap()),
            });
        }
        let mut frames_v = Vec::with_capacity(frames as usize);
        for i in 0..frames as usize {
            let off = i * 4;
            frames_v.push(u32::from_le_bytes(fbuf[off..off + 4].try_into().unwrap()));
        }
        (entries_v, frames_v)
    }
}