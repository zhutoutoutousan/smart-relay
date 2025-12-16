use eframe::egui;

#[derive(Default)]
pub struct CyberpunkTheme;

impl CyberpunkTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        
        // Dark background
        style.visuals.dark_mode = true;
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(11, 15, 26);
        style.visuals.panel_fill = egui::Color32::from_rgb(11, 15, 26);
        style.visuals.window_fill = egui::Color32::from_rgb(11, 15, 26);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(20, 25, 40);
        
        // Neon accents - use widgets for colors
        style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 25, 40);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0, 240, 255); // Light cyan for buttons
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 200, 220); // Slightly darker when active
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0, 250, 255); // Brighter on hover
        
        // Text colors - white for general UI, dark for buttons
        style.visuals.override_text_color = Some(egui::Color32::WHITE);
        
        // Widget text colors - dark text on light button backgrounds
        style.visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::WHITE;
        style.visuals.widgets.inactive.fg_stroke.color = egui::Color32::BLACK; // Dark text on light buttons
        style.visuals.widgets.active.fg_stroke.color = egui::Color32::BLACK; // Dark text on bright buttons
        style.visuals.widgets.hovered.fg_stroke.color = egui::Color32::BLACK; // Dark text on hover
        style.visuals.widgets.open.fg_stroke.color = egui::Color32::WHITE;
        
        // Hyperlinks
        style.visuals.hyperlink_color = egui::Color32::from_rgb(0, 240, 255);
        
        // Selection
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 240, 255).linear_multiply(0.3);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 240, 255));
        
        // Buttons - already configured via widgets above
        
        ctx.set_style(style);
    }
}

