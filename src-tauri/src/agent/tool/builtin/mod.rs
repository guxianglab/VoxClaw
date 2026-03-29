pub mod app;
pub mod bash;
pub mod clipboard;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod open_url;

use crate::agent::tool::Tool;

pub fn create_all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(bash::BashTool),
        Box::new(file_read::FileReadTool),
        Box::new(file_write::FileWriteTool),
        Box::new(file_edit::FileEditTool),
        Box::new(glob::GlobTool),
        Box::new(grep::GrepTool),
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
