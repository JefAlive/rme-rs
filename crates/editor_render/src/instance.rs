use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TileInstance {
    pub world_pos: [f32; 2],
    pub pixel_offset: [f32; 2],
    pub layer_index: u32,
    pub tint: [f32; 4], // branco = sem tingimento; usado depois p/ zonas (PZ/PVP/etc)
    /// Animação: `frames(8) | step_cells(16) | reservado(8)`. `frames == 0`
    /// significa estático (layer_index inalterado). `step_cells` é o avanço
    /// em células do atlas entre fases consecutivas (sprites por frame ×
    /// blocos 32×32 do sprite) — as fases são pré-decodificadas contíguas.
    pub anim_meta: u32,
    /// Animação por relógio: `dur_ms(16) | seed_ms(16)` (0 em ambos = 500ms
    /// padrão e sincronizado). `seed_ms` dessincroniza itens async por tile
    /// (estilo RME); sincronizados usam seed 0 → fase global.
    pub anim_clock: u32,
}

impl TileInstance {
    const ATTRS: [wgpu::VertexAttribute; 6] =
        wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Uint32, 4 => Float32x4, 5 => Uint32, 6 => Uint32];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}
