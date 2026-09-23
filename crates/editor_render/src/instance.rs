use bytemuck::{Pod, Zeroable};

/// v0 deliberadamente mínimo: cor sólida, sem atlas/layer/anim ainda.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TileInstance {
    pub world_pos: [f32; 2],
    pub color: [f32; 4],
}

impl TileInstance {
    const ATTRS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}