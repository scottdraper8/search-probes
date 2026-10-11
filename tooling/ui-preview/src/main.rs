//! Standalone development host for the production overlay UI.
use search_probes_model::{
    controls::Controls,
    runtime::{Action, Availability, State},
    settings::Settings,
};
use search_probes_ui::{App, egui};
use std::{path::PathBuf, sync::Arc, time::Duration};

const SETTINGS_KEY: &str = "preview_settings";

struct Preview {
    app: App,
    controls: Arc<Controls>,
    command_directory: PathBuf,
    reload_marker: Option<PathBuf>,
    announced: bool,
}

impl Preview {
    fn new(
        cc: &eframe::CreationContext<'_>,
        reload_marker: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let settings = cc
            .storage
            .and_then(|storage| eframe::get_value::<Settings>(storage, SETTINGS_KEY))
            .filter(|settings| settings.validate().is_ok())
            .unwrap_or_default();
        let mut state = State::new(settings);
        state.availability = Availability::MainMenu;
        let controls = Arc::new(Controls::new(state));
        // Preview commands use their own ephemeral directory, never the installed mod's settings.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let command_directory = std::env::temp_dir().join(format!(
            "search-probes-preview-commands-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&command_directory)?;
        Ok(Self {
            app: App::new(&cc.egui_ctx, controls.clone()),
            controls,
            command_directory,
            reload_marker,
            announced: false,
        })
    }
}

impl eframe::App for Preview {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self
            .reload_marker
            .as_ref()
            .is_some_and(|path| path.exists())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Check reload requests even when no input arrives or the window is hidden.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.app.refresh();
        self.app.ui(ui);
        self.controls.prepare(&self.command_directory);
        while let Some(command) = self.controls.pop() {
            self.controls.update(|state| {
                match command.action {
                    Action::Configure { settings } => state.settings = settings,
                    Action::ClearHistory {} => state.cached_systems = 0,
                    _ => state.error = Some("Searching and navigation run in NMS.".into()),
                }
                state.last_command = command.id;
            });
            ui.ctx().request_repaint();
        }
        if !self.announced {
            println!(
                "Search Probes UI preview rendered (PID {}).",
                std::process::id()
            );
            self.announced = true;
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, self.app.settings());
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.command_directory);
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args_os().skip(1);
    let reload_marker = match args.next() {
        None => None,
        Some(flag) if flag == "--reload-marker" => {
            let path = args.next().expect("--reload-marker needs a path");
            assert!(args.next().is_none(), "unexpected preview argument");
            Some(PathBuf::from(path))
        }
        _ => panic!("usage: preview [--reload-marker PATH]"),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("search-probes-ui-preview")
            .with_inner_size([1800.0, 1100.0])
            .with_min_inner_size([420.0, 300.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Search Probes — UI Preview",
        options,
        Box::new(move |cc| Ok(Box::new(Preview::new(cc, reload_marker)?))),
    )
}
