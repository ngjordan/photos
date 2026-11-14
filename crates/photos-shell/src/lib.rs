//! Photos host adapter. Reuses the standalone app's platform services without owning its event loop.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

// These remain shared with the standalone binary: file formats, recovery paths and platform
// behavior must not acquire a second implementation when the editor is hosted.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/app_dirs.rs"]
mod app_dirs;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/appearance.rs"]
mod appearance;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/crash_guard.rs"]
mod crash_guard;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/external_svg.rs"]
mod external_svg;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/file_dialog_probe.rs"]
mod file_dialog_probe;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/gpu_startup.rs"]
mod gpu_startup;
#[cfg(any(target_os = "linux", test))]
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/linux_libs.rs"]
mod linux_libs;
#[cfg(target_os = "macos")]
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/mac_menu.rs"]
mod mac_menu;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/photoshop_settings.rs"]
mod photoshop_settings;
#[cfg(target_os = "linux")]
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/portal_save.rs"]
mod portal_save;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/services.rs"]
mod services;
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)] // Shared standalone-only entrypoints are intentionally unused by the host.
#[path = "../../../apps/photocraft/src/updates.rs"]
mod updates;

use eframe::App as _;
use photocraft_engine::Session;
use photocraft_ui_egui::{PhotocraftApp, theme};

#[derive(Default)]
pub struct Launch {
    pub files: Vec<String>,
}

pub struct Shell {
    app: PhotocraftApp,
    ctx: egui::Context,
    fonts: egui::FontDefinitions,
}

impl Shell {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let mut shell = Self::new_for_host(&cc.egui_ctx, cc.storage, launch);
        if let Some(rs) = cc.wgpu_render_state.clone() {
            let prefs = &shell.app.session.prefs().performance;
            if rs.adapter.get_info().device_type != eframe::wgpu::DeviceType::Cpu
                && prefs.gpu_backend != photocraft_engine::prefs::GpuBackend::Cpu
                && prefs.effective_rendering_mode() != photocraft_engine::prefs::RenderingMode::Cpu
            {
                shell.app.set_wgpu(rs);
                shell.app.start_compositor(&cc.egui_ctx);
            } else {
                shell.app.watch_window_device(&rs.device);
            }
        }
        shell
    }

    /// Preferences and recovery use the existing profile, not eframe storage.
    pub fn new_for_host(ctx: &egui::Context, _storage: Option<&dyn eframe::Storage>, launch: Launch) -> Self {
        // The native file dialogs, clipboard and preset folder do not exist in a browser tab: no services on wasm32.
        #[cfg(target_arch = "wasm32")]
        let services = photocraft_ui_egui::Services::default();
        #[cfg(not(target_arch = "wasm32"))]
        let services = {
            let mut services = services::native(None);
            // Updates and process/window ownership belong to Workspace, not a hosted editor.
            services.check_updates = None;
            services.preset_store = services::presets_dir().map(photocraft_engine::preset_store::open_dir_async);
            services
        };
        let mut app = PhotocraftApp::new(Session::new(), services);
        app.background_jobs = true;
        let fonts = app.install_host_look(ctx);
        let mut shell = Self { app, ctx: ctx.clone(), fonts };
        for path in launch.files {
            shell.open_path(&path);
        }
        shell
    }

    pub fn logic(&mut self, ctx: &egui::Context) {
        self.app.logic_for_host(ctx);
    }

    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui_for_host(ui);
    }

    pub fn backdrop(&self) -> [f32; 4] {
        theme::Tokens::for_kind(self.app.ui.theme).canvas.to_normalized_gamma_f32()
    }

    pub fn open_path(&mut self, path: &str) {
        self.app.open_paths(&[path.to_owned()]);
    }

    /// The host negotiates close first. Do not dispatch file.exit (it closes the shared window).
    pub fn save_state(&mut self) {
        self.app.on_exit();
    }

    pub fn can_close(&self) -> bool {
        !self.app.has_unsaved_work() && !self.app.saving() && !self.app.printing()
    }

    pub fn zoom(&self) -> f32 {
        photocraft_ui_egui::prefs_ui::interface_zoom(&self.app, &self.ctx)
    }

    pub fn fonts(&self) -> egui::FontDefinitions {
        self.fonts.clone()
    }

    /// macOS owns a native menu; other platforms use the editor's in-window menu.
    pub fn install_native_menu(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.app.services.native_menu = mac_menu::install(&self.ctx, &self.app);
        }
    }
}

// Shared platform-service tests use the same child-process/file-lock exclusion as the binary.
#[cfg(test)]
pub(crate) static CHILD_SPAWNS: std::sync::RwLock<()> = std::sync::RwLock::new(());
#[cfg(test)]
pub(crate) fn no_child_spawns() -> std::sync::RwLockWriteGuard<'static, ()> {
    CHILD_SPAWNS.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_look_is_ready_before_the_first_frame_and_dirty_work_blocks_close() {
        let ctx = egui::Context::default();
        // No real profile writes or OS services in this headless regression.
        let app = PhotocraftApp::new(Session::new(), photocraft_ui_egui::Services::default());
        let mut shell = Shell { app, ctx: ctx.clone(), fonts: egui::FontDefinitions::default() };
        shell.fonts = shell.app.install_host_look(&ctx);
        assert!(shell.fonts.font_data.contains_key("Inter"));
        assert!(shell.fonts.families.contains_key(&egui::FontFamily::Name("medium".into())));
        assert!(shell.can_close());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            shell.logic(ui.ctx());
            shell.ui(ui);
            assert_eq!(ui.ctx().fonts(|f| f.definitions().families[&egui::FontFamily::Proportional][0].clone()), "Inter");
        });
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear(); // No renderer in this headless test.
        shell.app.run("file.new", serde_json::json!({"width": 8, "height": 8})).unwrap();
        shell.app.run("layer.new.layer", serde_json::json!({})).unwrap();
        assert!(!shell.can_close());
    }
}
