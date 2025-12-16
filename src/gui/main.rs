use eframe::egui;
use smart_relay::gui::App;
use smart_relay::telemetry;
use std::sync::Arc;
use tokio::runtime::Runtime;

fn main() -> eframe::Result<()> {
    // Initialize tracing
    telemetry::install_tracing();
    
    // Create Tokio runtime for async operations
    let rt = Arc::new(Runtime::new().expect("Failed to create Tokio runtime"));
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Smart Relay - AI Proxy Control")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    let rt_clone = rt.clone();
    eframe::run_native(
        "Smart Relay",
        options,
        Box::new(move |cc| {
            // Setup Chinese fonts for proper CJK character rendering
            use egui_chinese_font::setup_chinese_fonts;
            if let Err(e) = setup_chinese_fonts(&cc.egui_ctx) {
                eprintln!("Warning: Failed to load Chinese fonts: {}", e);
                eprintln!("Chinese characters may not display correctly");
            }
            
            let mut app = App::default();
            app.set_runtime(rt_clone.clone());
            Box::new(app)
        }),
    )
}

