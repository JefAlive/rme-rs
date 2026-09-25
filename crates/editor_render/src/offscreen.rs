pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Cena já composta antes da passagem de escalonamento. Não é registrada no
/// egui: somente o resultado final é apresentado na interface.
pub struct SceneTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl SceneTarget {
    pub fn create(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let (texture, view) = make_render_texture(device, "composited_scene", width, height);
        Self { texture, view, width, height }
    }

    pub fn resize_if_needed(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.width == width && self.height == height { return; }
        let (texture, view) = make_render_texture(device, "composited_scene", width, height);
        self.texture = texture;
        self.view = view;
        self.width = width;
        self.height = height;
    }
}

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
        // A textura apresentada já passou pelo filtro escolhido no shader.
        // Não aplique um segundo bilinear aqui: no modo Off ele transformaria
        // nearest-neighbour em uma imagem borrada em monitores HiDPI.
        let id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Nearest);
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
        self.id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Nearest);
        self.texture = texture;
        self.view = view;
        self.width = width;
        self.height = height;
    }

    fn make_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        make_render_texture(device, "viewport_offscreen", width, height)
    }
}

fn make_render_texture(device: &wgpu::Device, label: &'static str, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
