//! egui-wgpu 配置（仅 egui backend 使用）

/// Windows 强制 DX12（QEMU/RDP 无真实 GPU 时走 WARP）；其他平台默认。
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
