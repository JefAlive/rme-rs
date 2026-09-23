use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TileInstance {
    pub world_pos: [f32; 2],
    /// Deslocamento fixo em pixels: soma do "shift" do item (appearances.dat)
    /// com a elevação acumulada da pilha até aqui (negativo = sobe na tela).
    pub pixel_offset: [f32; 2],
    /// Índice na AnimTable — 0 = estático/transparente por padrão.
    pub anim_id: u32,
    pub tint: [f32; 4],
}

impl TileInstance {
    const ATTRS: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Uint32, 4 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}