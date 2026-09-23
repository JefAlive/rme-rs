pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct OffscreenTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

impl OffscreenTarget {
    pub fn create(device: &wgpu::Device, renderer: &mut egui_wgpu::Renderer, width: u32, height: u32) -> Self {
        let (texture, view) = Self::make_texture(device, width, height);
        let id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);
        Self { texture, view, id, width, height }
    }

    /// Chamado todo frame; só recria a textura se o tamanho realmente mudou
    /// (redimensionar o painel do dock, por exemplo).
    pub fn resize_if_needed(&mut self, device: &wgpu::Device, renderer: &mut egui_wgpu::Renderer, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        renderer.free_texture(&self.id);
        let (texture, view) = Self::make_texture(device, width, height);
        self.id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);
        self.texture = texture;
        self.view = view;
        self.width = width;
        self.height = height;
    }

    fn make_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_offscreen"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }
}