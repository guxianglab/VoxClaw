pub mod app;
pub mod clipboard;
pub mod open_url;

use crate::agent::tool::Tool;

pub fn create_all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(clipboard::ClipboardTool),
        Box::new(open_url::OpenUrlTool),
        Box::new(app::OpenCalculatorTool),
        Box::new(app::OpenBrowserTool),
        Box::new(app::OpenNotepadTool),
        Box::new(app::OpenExplorerTool),
        Box::new(app::ScreenshotTool),
        Box::new(app::ComposeEmailTool),
    ]
}
