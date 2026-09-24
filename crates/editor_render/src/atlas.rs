/// Atlas 2D de sprites 32×32, organizado em slots lineares.
///
/// A largura usa o maior número potência de dois de sprites por linha que a
/// GPU suporta. A altura cresce em linhas sob demanda, sem usar o limite bem
/// menor de layers das texture arrays.
pub struct SpriteAtlas {
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    texture: wgpu::Texture,
    cells: Vec<[u8; 32 * 32 * 4]>,
    columns: u32,
    rows: u32,
    max_rows: u32,
}

impl SpriteAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let max_dimension = (device.limits().max_texture_dimension_2d / 32).max(1);
        let columns = 1 << (31 - max_dimension.leading_zeros());
        let max_rows = max_dimension;
        let rows = 1;
        let texture = create_texture(device, columns, rows);
        let bind_group_layout = create_bind_group_layout(device);
        let bind_group = create_bind_group(device, &bind_group_layout, &texture);

        Self {
            bind_group,
            bind_group_layout,
            texture,
            cells: vec![[0; 32 * 32 * 4]], // slot zero transparente
            columns,
            rows,
            max_rows,
        }
    }

    pub fn layer_count(&self) -> u32 {
        self.cells.len() as u32
    }

    /// Mantido para diagnóstico legado: agora representa slots totais
    /// endereçáveis, não texture-array layers.
    pub fn max_layers(&self) -> u32 {
        self.columns.saturating_mul(self.max_rows)
    }

    pub fn columns(&self) -> u32 {
        self.columns
    }

    pub fn append(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8; 32 * 32 * 4],
    ) -> u32 {
        let slot = self.cells.len() as u32;
        if slot >= self.max_layers() {
            return 0;
        }

        let needed_row = slot / self.columns;
        if needed_row >= self.rows && !self.grow_to_fit(device, queue, needed_row + 1) {
            return 0;
        }

        self.cells.push(*rgba);
        self.write_cell(queue, slot, rgba);
        slot
    }

    fn grow_to_fit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, needed_rows: u32) -> bool {
        let mut new_rows = self.rows;
        while new_rows < needed_rows && new_rows < self.max_rows {
            new_rows = new_rows.saturating_mul(2).min(self.max_rows);
        }
        if new_rows < needed_rows || new_rows == self.rows {
            return false;
        }

        let new_texture = create_texture(device, self.columns, new_rows);
        for (slot, cell) in self.cells.iter().enumerate() {
            self.write_cell_to(queue, &new_texture, slot as u32, cell);
        }

        self.bind_group = create_bind_group(device, &self.bind_group_layout, &new_texture);
        self.texture = new_texture;
        self.rows = new_rows;
        eprintln!(
            "sprite atlas: cresceu para {}x{} slots ({} sprites/linha)",
            self.columns * 32,
            self.rows * 32,
            self.columns,
        );
        true
    }

    fn write_cell(&self, queue: &wgpu::Queue, slot: u32, rgba: &[u8; 32 * 32 * 4]) {
        self.write_cell_to(queue, &self.texture, slot, rgba);
    }

    fn write_cell_to(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, slot: u32, rgba: &[u8]) {
        let x = (slot % self.columns) * 32;
        let y = (slot / self.columns) * 32;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(32 * 4),
                rows_per_image: Some(32),
            },
            wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: 1 },
        );
    }
}

fn create_texture(device: &wgpu::Device, columns: u32, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sprite_atlas_2d"),
        size: wgpu::Extent3d {
            width: columns * 32,
            height: rows * 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sprite_atlas_bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sprite_atlas_bg"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    })
}
