//! egui-wgpu 配置（仅 egui backend 使用）

/// wgpu 后端配置：Windows 固定 DX12，其他平台用默认配置。
pub fn wgpu_configuration() -> eframe::egui_wgpu::WgpuConfiguration {
    #[cfg(target_os = "windows")]
    {
        let mut create_new = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        create_new.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
        eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(create_new),
            ..Default::default()
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        eframe::egui_wgpu::WgpuConfiguration::default()
    }
}
