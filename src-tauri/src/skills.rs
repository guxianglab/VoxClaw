use std::collections::HashSet;
use std::net::IpAddr;
use std::process::Command;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::storage::PromptProfile;

pub const SWITCH_POLISH_SCENE_SKILL_ID: &str = "switch_polish_scene";
pub const OPEN_BROWSER_SKILL_ID: &str = "open_browser";
pub const OPEN_WINDOWS_SKILL_ID: &str = "open_windows";
pub const BROWSER_OPEN_TARGET_SUB_COMMAND_ID: &str = "open_target";
pub const BROWSER_NEW_TAB_SUB_COMMAND_ID: &str = "new_tab";
pub const BROWSER_CLOSE_TAB_SUB_COMMAND_ID: &str = "close_tab";
pub const BROWSER_NEXT_TAB_SUB_COMMAND_ID: &str = "next_tab";
pub const BROWSER_PREVIOUS_TAB_SUB_COMMAND_ID: &str = "previous_tab";
pub const BROWSER_SWITCH_TAB_INDEX_SUB_COMMAND_ID: &str = "switch_tab_index";
pub const BROWSER_REOPEN_TAB_SUB_COMMAND_ID: &str = "reopen_tab";
pub const BROWSER_CLOSE_OTHER_TABS_SUB_COMMAND_ID: &str = "close_other_tabs";
pub const BROWSER_CLOSE_TABS_TO_RIGHT_SUB_COMMAND_ID: &str = "close_tabs_to_right";
pub const BROWSER_GO_BACK_SUB_COMMAND_ID: &str = "go_back";
pub const BROWSER_GO_FORWARD_SUB_COMMAND_ID: &str = "go_forward";
pub const BROWSER_REFRESH_SUB_COMMAND_ID: &str = "refresh";
pub const BROWSER_HARD_REFRESH_SUB_COMMAND_ID: &str = "hard_refresh";
pub const BROWSER_STOP_LOADING_SUB_COMMAND_ID: &str = "stop_loading";
pub const BROWSER_GO_HOME_SUB_COMMAND_ID: &str = "go_home";
pub const BROWSER_SCROLL_UP_SUB_COMMAND_ID: &str = "scroll_up";
pub const BROWSER_SCROLL_DOWN_SUB_COMMAND_ID: &str = "scroll_down";
pub const BROWSER_SCROLL_TOP_SUB_COMMAND_ID: &str = "scroll_top";
pub const BROWSER_SCROLL_BOTTOM_SUB_COMMAND_ID: &str = "scroll_bottom";
pub const BROWSER_PAGE_UP_SUB_COMMAND_ID: &str = "page_up";
pub const BROWSER_PAGE_DOWN_SUB_COMMAND_ID: &str = "page_down";
pub const BROWSER_FIND_SUB_COMMAND_ID: &str = "find";
pub const BROWSER_FULLSCREEN_SUB_COMMAND_ID: &str = "fullscreen";
pub const BROWSER_COPY_URL_SUB_COMMAND_ID: &str = "copy_url";
pub const BROWSER_OPEN_HISTORY_SUB_COMMAND_ID: &str = "open_history";
pub const BROWSER_OPEN_DOWNLOADS_SUB_COMMAND_ID: &str = "open_downloads";
pub const BROWSER_OPEN_DEVTOOLS_SUB_COMMAND_ID: &str = "open_devtools";
pub const BROWSER_MINIMIZE_WINDOW_SUB_COMMAND_ID: &str = "minimize_window";
pub const BROWSER_MAXIMIZE_WINDOW_SUB_COMMAND_ID: &str = "maximize_window";
pub const BROWSER_NEW_PRIVATE_WINDOW_SUB_COMMAND_ID: &str = "new_private_window";
pub const WINDOWS_OPEN_TARGET_SUB_COMMAND_ID: &str = "open_target";
pub const WINDOWS_SHOW_DESKTOP_SUB_COMMAND_ID: &str = "show_desktop";
pub const WINDOWS_LOCK_SCREEN_SUB_COMMAND_ID: &str = "lock_screen";
pub const WINDOWS_OPEN_RUN_DIALOG_SUB_COMMAND_ID: &str = "open_run_dialog";
pub const WINDOWS_OPEN_CLIPBOARD_HISTORY_SUB_COMMAND_ID: &str = "open_clipboard_history";
pub const WINDOWS_OPEN_QUICK_SETTINGS_SUB_COMMAND_ID: &str = "open_quick_settings";
pub const WINDOWS_OPEN_NOTIFICATIONS_SUB_COMMAND_ID: &str = "open_notifications";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SkillSubCommandConfig {
    pub id: String,
    pub name: String,
    pub keywords: String,
    pub enabled: bool,
}

impl SkillSubCommandConfig {
    fn new(id: &str, name: &str, keywords: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            keywords: keywords.to_string(),
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BrowserSiteConfig {
    pub id: String,
    pub name: String,
    pub aliases: String,
    pub url: String,
    pub enabled: bool,
}

impl BrowserSiteConfig {
    fn new(id: &str, name: &str, aliases: &str, url: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            aliases: aliases.to_string(),
            url: url.to_string(),
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BrowserSkillOptions {
    #[serde(default = "default_true")]
    pub llm_site_resolution_enabled: bool,
    #[serde(default = "default_true")]
    pub search_fallback_enabled: bool,
    #[serde(default = "default_search_url_template")]
    pub search_url_template: String,
    #[serde(default = "default_browser_sites")]
    pub sites: Vec<BrowserSiteConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowsLaunchKind {
    Command,
    Shell,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WindowsTargetConfig {
    pub id: String,
    pub name: String,
    pub aliases: String,
    pub launch_kind: WindowsLaunchKind,
    pub launch_target: String,
    #[serde(default)]
    pub launch_args: Vec<String>,
    pub enabled: bool,
}

impl WindowsTargetConfig {
    fn command(
        id: &str,
        name: &str,
        aliases: &str,
        launch_target: &str,
        launch_args: &[&str],
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            aliases: aliases.to_string(),
            launch_kind: WindowsLaunchKind::Command,
            launch_target: launch_target.to_string(),
            launch_args: launch_args.iter().map(|value| value.to_string()).collect(),
            enabled: true,
        }
    }

    fn shell(id: &str, name: &str, aliases: &str, launch_target: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            aliases: aliases.to_string(),
            launch_kind: WindowsLaunchKind::Shell,
            launch_target: launch_target.to_string(),
            launch_args: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WindowsSkillOptions {
    #[serde(default = "default_true")]
    pub llm_target_resolution_enabled: bool,
    #[serde(default = "default_windows_targets")]
    pub targets: Vec<WindowsTargetConfig>,
}

impl Default for WindowsSkillOptions {
    fn default() -> Self {
        Self {
            llm_target_resolution_enabled: true,
            targets: default_windows_targets(),
        }
    }
}

impl Default for BrowserSkillOptions {
    fn default() -> Self {
        Self {
            llm_site_resolution_enabled: true,
            search_fallback_enabled: true,
            search_url_template: default_search_url_template(),
            sites: default_browser_sites(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SkillConfig {
    pub id: String,
    pub name: String,
    pub keywords: String,
    pub enabled: bool,
    #[serde(default)]
    pub sub_commands: Vec<SkillSubCommandConfig>,
    #[serde(default)]
    pub browser_options: Option<BrowserSkillOptions>,
    #[serde(default)]
    pub windows_options: Option<WindowsSkillOptions>,
}

impl SkillConfig {
    fn new(id: &str, name: &str, keywords: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            keywords: keywords.to_string(),
            enabled: true,
            sub_commands: Vec::new(),
            browser_options: None,
            windows_options: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMatch {
    pub skill_id: String,
    pub keyword: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubCommandMatch {
    pub sub_command_id: String,
    pub name: String,
    pub keyword: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinSkillId {
    ComposeEmail,
    OpenCalculator,
    OpenBrowser,
    OpenWindows,
    OpenNotepad,
    OpenExplorer,
    Screenshot,
    SwitchPolishScene,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneResolveResult {
    Unique {
        profile_id: String,
        profile_name: String,
    },
    None,
    Ambiguous(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    OpenTarget { query: String },
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    SwitchTabIndex { index: usize },
    ReopenTab,
    CloseOtherTabs,
    CloseTabsToRight,
    GoBack,
    GoForward,
    Refresh,
    HardRefresh,
    StopLoading,
    GoHome,
    ScrollUp,
    ScrollDown,
    ScrollTop,
    ScrollBottom,
    PageUp,
    PageDown,
    Find { query: Option<String> },
    Fullscreen,
    CopyUrl,
    OpenHistory,
    OpenDownloads,
    OpenDevtools,
    MinimizeWindow,
    MaximizeWindow,
    NewPrivateWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserActionPlan {
    pub action: BrowserAction,
    pub action_name: String,
    pub note: Option<String>,
    pub consumed_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserPlanResult {
    Action(BrowserActionPlan),
    Feedback(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsAction {
    OpenTarget { query: String },
    ShowDesktop,
    LockScreen,
    OpenRunDialog,
    OpenClipboardHistory,
    OpenQuickSettings,
    OpenNotifications,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActionPlan {
    pub action: WindowsAction,
    pub action_name: String,
    pub note: Option<String>,
    pub consumed_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsPlanResult {
    Action(WindowsActionPlan),
    Feedback(String),
    None,
}

fn default_true() -> bool {
    true
}

fn default_search_url_template() -> String {
    "https://www.bing.com/search?q={query}".to_string()
}

fn default_browser_sites() -> Vec<BrowserSiteConfig> {
    vec![
        BrowserSiteConfig::new("github", "GitHub", "github,代码托管", "https://github.com"),
        BrowserSiteConfig::new(
            "bilibili",
            "B站",
            "b站,哔哩哔哩,bilibili",
            "https://www.bilibili.com",
        ),
        BrowserSiteConfig::new(
            "gmail",
            "Gmail",
            "gmail,谷歌邮箱",
            "https://mail.google.com",
        ),
    ]
}

fn default_windows_targets() -> Vec<WindowsTargetConfig> {
    vec![
        WindowsTargetConfig::shell(
            "settings",
            "Windows Settings",
            "settings,windows settings,system settings,设置,系统设置",
            "ms-settings:",
        ),
        WindowsTargetConfig::shell(
            "display_settings",
            "Display Settings",
            "display settings,screen settings,显示设置,屏幕设置",
            "ms-settings:display",
        ),
        WindowsTargetConfig::shell(
            "sound_settings",
            "Sound Settings",
            "sound settings,audio settings,声音设置,音频设置",
            "ms-settings:sound",
        ),
        WindowsTargetConfig::shell(
            "wifi_settings",
            "Wi-Fi Settings",
            "wifi,wifi settings,wireless network,无线网络,wifi设置",
            "ms-settings:network-wifi",
        ),
        WindowsTargetConfig::shell(
            "bluetooth_settings",
            "Bluetooth Settings",
            "bluetooth,bluetooth settings,蓝牙,蓝牙设置",
            "ms-settings:bluetooth",
        ),
        WindowsTargetConfig::shell(
            "windows_update",
            "Windows Update",
            "windows update,update settings,更新,更新设置",
            "ms-settings:windowsupdate",
        ),
        WindowsTargetConfig::shell(
            "apps_settings",
            "Apps Settings",
            "apps installed,installed apps,应用,应用和功能,已安装应用",
            "ms-settings:appsfeatures",
        ),
        WindowsTargetConfig::command(
            "task_manager",
            "Task Manager",
            "task manager,任务管理器",
            "taskmgr",
            &[],
        ),
        WindowsTargetConfig::command(
            "control_panel",
            "Control Panel",
            "control panel,控制面板",
            "control",
            &[],
        ),
        WindowsTargetConfig::shell(
            "device_manager",
            "Device Manager",
            "device manager,设备管理器",
            "devmgmt.msc",
        ),
        WindowsTargetConfig::shell(
            "disk_management",
            "Disk Management",
            "disk management,磁盘管理",
            "diskmgmt.msc",
        ),
        WindowsTargetConfig::shell(
            "services",
            "Services",
            "services,windows services,服务,系统服务",
            "services.msc",
        ),
        WindowsTargetConfig::shell(
            "event_viewer",
            "Event Viewer",
            "event viewer,日志查看器,事件查看器",
            "eventvwr.msc",
        ),
        WindowsTargetConfig::command(
            "terminal",
            "Windows Terminal",
            "terminal,windows terminal,终端",
            "wt",
            &[],
        ),
        WindowsTargetConfig::command(
            "command_prompt",
            "Command Prompt",
            "cmd,command prompt,dos,命令提示符",
            "cmd",
            &[],
        ),
        WindowsTargetConfig::command(
            "powershell",
            "PowerShell",
            "powershell,power shell",
            "powershell",
            &[],
        ),
        WindowsTargetConfig::command(
            "explorer",
            "File Explorer",
            "explorer,file explorer,资源管理器,文件管理器",
            "explorer",
            &[],
        ),
        WindowsTargetConfig::shell(
            "downloads",
            "Downloads",
            "downloads,download folder,下载,下载文件夹",
            "shell:Downloads",
        ),
        WindowsTargetConfig::shell(
            "documents",
            "Documents",
            "documents,document folder,文档,我的文档",
            "shell:Personal",
        ),
        WindowsTargetConfig::shell("desktop", "Desktop", "desktop,桌面", "shell:Desktop"),
        WindowsTargetConfig::shell(
            "pictures",
            "Pictures",
            "pictures,photos,图片,照片",
            "shell:My Pictures",
        ),
    ]
}

fn default_browser_sub_commands() -> Vec<SkillSubCommandConfig> {
    vec![
        SkillSubCommandConfig::new(
            BROWSER_OPEN_TARGET_SUB_COMMAND_ID,
            "打开目标",
            "打开,访问,前往,进入",
        ),
        SkillSubCommandConfig::new(
            BROWSER_NEW_TAB_SUB_COMMAND_ID,
            "新建页面",
            "新建页面,新建标签页,新建标签,打开新页面,打开新标签页",
        ),
        SkillSubCommandConfig::new(
            BROWSER_CLOSE_TAB_SUB_COMMAND_ID,
            "关闭页面",
            "关闭页面,关闭当前页面,关闭标签页,关闭标签",
        ),
        SkillSubCommandConfig::new(
            BROWSER_NEXT_TAB_SUB_COMMAND_ID,
            "下一个页面",
            "下一个页面,下一个标签页,下一个标签,切到下一个页面,切到下一个标签页",
        ),
        SkillSubCommandConfig::new(
            BROWSER_PREVIOUS_TAB_SUB_COMMAND_ID,
            "上一个页面",
            "上一个页面,上一个标签页,上一个标签,切到上一个页面,切到上一个标签页",
        ),
        SkillSubCommandConfig::new(
            BROWSER_SWITCH_TAB_INDEX_SUB_COMMAND_ID,
            "第几个页面",
            "第,切到第,打开第,第几个页面,第几个标签页,第几页,第几个标签",
        ),
        SkillSubCommandConfig::new(
            BROWSER_REOPEN_TAB_SUB_COMMAND_ID,
            "重新打开",
            "重新打开,重新打开页面,重新打开标签页,恢复关闭页面,恢复关闭标签页",
        ),
        SkillSubCommandConfig::new(
            BROWSER_CLOSE_OTHER_TABS_SUB_COMMAND_ID,
            "关闭其他页面",
            "关闭其他页面,关闭其他标签页,关闭其他标签",
        ),
        SkillSubCommandConfig::new(
            BROWSER_CLOSE_TABS_TO_RIGHT_SUB_COMMAND_ID,
            "关闭右侧页面",
            "关闭右侧页面,关闭右侧标签页,关闭右侧标签",
        ),
        SkillSubCommandConfig::new(BROWSER_GO_BACK_SUB_COMMAND_ID, "后退", "后退,返回上一页"),
        SkillSubCommandConfig::new(BROWSER_GO_FORWARD_SUB_COMMAND_ID, "前进", "前进,前往下一页"),
        SkillSubCommandConfig::new(BROWSER_REFRESH_SUB_COMMAND_ID, "刷新", "刷新,刷新页面"),
        SkillSubCommandConfig::new(
            BROWSER_HARD_REFRESH_SUB_COMMAND_ID,
            "强制刷新",
            "强制刷新,硬刷新,重新加载页面",
        ),
        SkillSubCommandConfig::new(
            BROWSER_STOP_LOADING_SUB_COMMAND_ID,
            "停止加载",
            "停止加载,停止打开,停止页面加载",
        ),
        SkillSubCommandConfig::new(
            BROWSER_GO_HOME_SUB_COMMAND_ID,
            "回主页",
            "回主页,打开主页,返回主页",
        ),
        SkillSubCommandConfig::new(
            BROWSER_SCROLL_UP_SUB_COMMAND_ID,
            "往上",
            "往上,往山,向上滚动,往上滚,向上翻",
        ),
        SkillSubCommandConfig::new(
            BROWSER_SCROLL_DOWN_SUB_COMMAND_ID,
            "往下",
            "往下,向下滚动,往下滚,向下翻",
        ),
        SkillSubCommandConfig::new(
            BROWSER_SCROLL_TOP_SUB_COMMAND_ID,
            "滚到顶部",
            "滚到顶部,回到顶部,到顶部,跳到顶部",
        ),
        SkillSubCommandConfig::new(
            BROWSER_SCROLL_BOTTOM_SUB_COMMAND_ID,
            "滚到底部",
            "滚到底部,回到底部,到底部,跳到底部",
        ),
        SkillSubCommandConfig::new(BROWSER_PAGE_UP_SUB_COMMAND_ID, "上一页", "上一页,上翻页"),
        SkillSubCommandConfig::new(BROWSER_PAGE_DOWN_SUB_COMMAND_ID, "下一页", "下一页,下翻页"),
        SkillSubCommandConfig::new(
            BROWSER_FIND_SUB_COMMAND_ID,
            "查找",
            "查找,搜索页面,页内查找",
        ),
        SkillSubCommandConfig::new(
            BROWSER_FULLSCREEN_SUB_COMMAND_ID,
            "全屏",
            "全屏,进入全屏,退出全屏",
        ),
        SkillSubCommandConfig::new(
            BROWSER_COPY_URL_SUB_COMMAND_ID,
            "复制网址",
            "复制网址,复制链接,复制当前网址,复制当前链接",
        ),
        SkillSubCommandConfig::new(
            BROWSER_OPEN_HISTORY_SUB_COMMAND_ID,
            "打开历史记录",
            "打开历史记录,历史记录,打开浏览历史",
        ),
        SkillSubCommandConfig::new(
            BROWSER_OPEN_DOWNLOADS_SUB_COMMAND_ID,
            "打开下载",
            "打开下载,下载列表,下载记录",
        ),
        SkillSubCommandConfig::new(
            BROWSER_OPEN_DEVTOOLS_SUB_COMMAND_ID,
            "打开开发者工具",
            "打开开发者工具,开发者工具,调试工具",
        ),
        SkillSubCommandConfig::new(
            BROWSER_MINIMIZE_WINDOW_SUB_COMMAND_ID,
            "最小化",
            "最小化,最小化窗口",
        ),
        SkillSubCommandConfig::new(
            BROWSER_MAXIMIZE_WINDOW_SUB_COMMAND_ID,
            "最大化",
            "最大化,最大化窗口",
        ),
        SkillSubCommandConfig::new(
            BROWSER_NEW_PRIVATE_WINDOW_SUB_COMMAND_ID,
            "新建隐私窗口",
            "新建隐私窗口,新建无痕窗口,打开无痕窗口,打开隐私窗口",
        ),
    ]
}

fn default_windows_sub_commands() -> Vec<SkillSubCommandConfig> {
    vec![
        SkillSubCommandConfig::new(
            WINDOWS_OPEN_TARGET_SUB_COMMAND_ID,
            "Open Windows Target",
            "open,launch,start,run,打开,启动,运行,进入",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_SHOW_DESKTOP_SUB_COMMAND_ID,
            "Show Desktop",
            "show desktop,desktop,显示桌面,回到桌面",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_LOCK_SCREEN_SUB_COMMAND_ID,
            "Lock Screen",
            "lock screen,lock windows,锁屏,锁定电脑",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_OPEN_RUN_DIALOG_SUB_COMMAND_ID,
            "Open Run Dialog",
            "run dialog,open run,运行,打开运行",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_OPEN_CLIPBOARD_HISTORY_SUB_COMMAND_ID,
            "Open Clipboard History",
            "clipboard history,clip history,剪贴板历史,打开剪贴板",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_OPEN_QUICK_SETTINGS_SUB_COMMAND_ID,
            "Open Quick Settings",
            "quick settings,action center controls,快速设置",
        ),
        SkillSubCommandConfig::new(
            WINDOWS_OPEN_NOTIFICATIONS_SUB_COMMAND_ID,
            "Open Notifications",
            "notifications,notification center,打开通知,通知中心",
        ),
    ]
}

impl BuiltinSkillId {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "compose_email" => Some(Self::ComposeEmail),
            "open_calculator" => Some(Self::OpenCalculator),
            OPEN_BROWSER_SKILL_ID => Some(Self::OpenBrowser),
            OPEN_WINDOWS_SKILL_ID => Some(Self::OpenWindows),
            "open_notepad" => Some(Self::OpenNotepad),
            "open_explorer" => Some(Self::OpenExplorer),
            "screenshot" => Some(Self::Screenshot),
            SWITCH_POLISH_SCENE_SKILL_ID => Some(Self::SwitchPolishScene),
            _ => None,
        }
    }

    fn is_config_control(&self) -> bool {
        matches!(self, Self::SwitchPolishScene)
    }
}

pub fn is_config_skill(skill_id: &str) -> bool {
    BuiltinSkillId::from_str(skill_id)
        .map(|skill| skill.is_config_control())
        .unwrap_or(false)
}

pub fn get_default_skills() -> Vec<SkillConfig> {
    let mut browser_skill =
        SkillConfig::new(OPEN_BROWSER_SKILL_ID, "浏览器", "打开浏览器,上网,浏览器");
    browser_skill.sub_commands = default_browser_sub_commands();
    browser_skill.browser_options = Some(BrowserSkillOptions::default());
    let mut windows_skill = SkillConfig::new(
        OPEN_WINDOWS_SKILL_ID,
        "Windows",
        "windows,system,desktop,电脑,系统,桌面",
    );
    windows_skill.sub_commands = default_windows_sub_commands();
    windows_skill.windows_options = Some(WindowsSkillOptions::default());

    vec![
        SkillConfig::new("compose_email", "写邮件", "写邮件,发邮件,新邮件,发送邮件"),
        SkillConfig::new("open_calculator", "计算器", "计算器,打开计算器"),
        browser_skill,
        windows_skill,
        SkillConfig::new("open_notepad", "记事本", "记事本,打开记事本"),
        SkillConfig::new(
            "open_explorer",
            "文件管理器",
            "文件管理器,资源管理器,打开文件夹",
        ),
        SkillConfig::new("screenshot", "截图", "截图,截屏,屏幕截图"),
        SkillConfig::new(
            SWITCH_POLISH_SCENE_SKILL_ID,
            "切换润色场景",
            "切换到,切到,切换成,切成,使用",
        ),
    ]
}

pub fn merge_with_default_skills(existing_skills: Vec<SkillConfig>) -> (Vec<SkillConfig>, bool) {
    let defaults = get_default_skills();
    let mut changed = false;
    let mut merged: Vec<SkillConfig> = existing_skills
        .into_iter()
        .filter(|skill| {
            let keep = !matches!(skill.id.as_str(), "enable_polish" | "disable_polish");
            changed |= !keep;
            keep
        })
        .collect();
    let existing_ids: HashSet<String> = merged.iter().map(|skill| skill.id.clone()).collect();

    for skill in &mut merged {
        if let Some(default_skill) = defaults
            .iter()
            .find(|default_skill| default_skill.id == skill.id)
        {
            changed |= merge_skill_defaults(skill, default_skill);
        }
    }

    for skill in defaults {
        if !existing_ids.contains(&skill.id) {
            merged.push(skill);
            changed = true;
        }
    }

    (merged, changed)
}

fn merge_skill_defaults(skill: &mut SkillConfig, default_skill: &SkillConfig) -> bool {
    let mut changed = false;
    changed |= merge_builtin_keywords(&mut skill.keywords, &default_skill.keywords);
    changed |= merge_sub_commands(&mut skill.sub_commands, &default_skill.sub_commands);

    match (&mut skill.browser_options, &default_skill.browser_options) {
        (None, Some(default_options)) => {
            skill.browser_options = Some(default_options.clone());
            changed = true;
        }
        (Some(options), Some(default_options)) => {
            if options.search_url_template.trim().is_empty() {
                options.search_url_template = default_options.search_url_template.clone();
                changed = true;
            }
            changed |= merge_browser_sites(&mut options.sites, &default_options.sites);
        }
        _ => {}
    }

    match (&mut skill.windows_options, &default_skill.windows_options) {
        (None, Some(default_options)) => {
            skill.windows_options = Some(default_options.clone());
            changed = true;
        }
        (Some(options), Some(default_options)) => {
            changed |= merge_windows_targets(&mut options.targets, &default_options.targets);
        }
        _ => {}
    }

    changed
}

fn merge_builtin_keywords(existing_keywords: &mut String, default_keywords: &str) -> bool {
    let existing: HashSet<String> = existing_keywords
        .split(',')
        .map(|keyword| keyword.trim().to_lowercase())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let missing: Vec<&str> = default_keywords
        .split(',')
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty() && !existing.contains(&keyword.to_lowercase()))
        .collect();

    if missing.is_empty() {
        return false;
    }

    if !existing_keywords.trim().is_empty() && !existing_keywords.trim_end().ends_with(',') {
        existing_keywords.push(',');
    }
    existing_keywords.push_str(&missing.join(","));
    true
}

fn merge_sub_commands(
    existing: &mut Vec<SkillSubCommandConfig>,
    defaults: &[SkillSubCommandConfig],
) -> bool {
    let existing_ids: HashSet<String> = existing.iter().map(|command| command.id.clone()).collect();
    let mut changed = false;

    for default_command in defaults {
        if !existing_ids.contains(&default_command.id) {
            existing.push(default_command.clone());
            changed = true;
        }
    }

    changed
}

fn merge_browser_sites(
    existing: &mut Vec<BrowserSiteConfig>,
    defaults: &[BrowserSiteConfig],
) -> bool {
    let existing_ids: HashSet<String> = existing.iter().map(|site| site.id.clone()).collect();
    let mut changed = false;

    for default_site in defaults {
        if !existing_ids.contains(&default_site.id) {
            existing.push(default_site.clone());
            changed = true;
        }
    }

    changed
}

fn merge_windows_targets(
    existing: &mut Vec<WindowsTargetConfig>,
    defaults: &[WindowsTargetConfig],
) -> bool {
    let existing_ids: HashSet<String> = existing.iter().map(|target| target.id.clone()).collect();
    let mut changed = false;

    for default_target in defaults {
        if !existing_ids.contains(&default_target.id) {
            existing.push(default_target.clone());
            changed = true;
        }
    }

    changed
}

pub fn match_skills(text: &str, skills: &[SkillConfig]) -> Vec<SkillMatch> {
    let text_lower = text.to_lowercase();
    let mut matches: Vec<(usize, usize, SkillMatch)> = Vec::new();

    for (skill_index, skill) in skills.iter().enumerate() {
        if !skill.enabled {
            continue;
        }

        let mut best_match: Option<SkillMatch> = None;

        for raw_keyword in skill.keywords.split(',') {
            let keyword = raw_keyword.trim();
            if keyword.is_empty() {
                continue;
            }

            let keyword_lower = keyword.to_lowercase();
            if let Some(pos) = text_lower.find(&keyword_lower) {
                let candidate = SkillMatch {
                    skill_id: skill.id.clone(),
                    keyword: keyword.to_string(),
                    start: pos,
                    end: pos + keyword_lower.len(),
                };

                let should_replace = best_match
                    .as_ref()
                    .map(|existing| candidate.start < existing.start)
                    .unwrap_or(true);

                if should_replace {
                    best_match = Some(candidate);
                }
            }
        }

        if let Some(skill_match) = best_match {
            println!(
                "[SKILL] Matched skill '{}' with keyword '{}'",
                skill_match.skill_id, skill_match.keyword
            );
            matches.push((skill_match.start, skill_index, skill_match));
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    matches
        .into_iter()
        .map(|(_, _, skill_match)| skill_match)
        .collect()
}

pub fn match_sub_commands(
    text: &str,
    sub_commands: &[SkillSubCommandConfig],
) -> Vec<SubCommandMatch> {
    let text_lower = text.to_lowercase();
    let mut matches: Vec<(usize, usize, SubCommandMatch)> = Vec::new();

    for (command_index, command) in sub_commands.iter().enumerate() {
        if !command.enabled {
            continue;
        }

        let mut best_match: Option<SubCommandMatch> = None;

        for raw_keyword in command.keywords.split(',') {
            let keyword = raw_keyword.trim();
            if keyword.is_empty() {
                continue;
            }

            let keyword_lower = keyword.to_lowercase();
            if let Some(pos) = text_lower.find(&keyword_lower) {
                let candidate = SubCommandMatch {
                    sub_command_id: command.id.clone(),
                    name: command.name.clone(),
                    keyword: keyword.to_string(),
                    start: pos,
                    end: pos + keyword_lower.len(),
                };

                let should_replace = best_match
                    .as_ref()
                    .map(|existing| candidate.start < existing.start)
                    .unwrap_or(true);

                if should_replace {
                    best_match = Some(candidate);
                }
            }
        }

        if let Some(command_match) = best_match {
            matches.push((command_match.start, command_index, command_match));
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    matches
        .into_iter()
        .map(|(_, _, command_match)| command_match)
        .collect()
}

pub fn find_skill_config<'a>(skills: &'a [SkillConfig], skill_id: &str) -> Option<&'a SkillConfig> {
    skills.iter().find(|skill| skill.id == skill_id)
}

pub fn plan_browser_action(
    transcript: &str,
    browser_skill: &SkillConfig,
    browser_match: Option<&SkillMatch>,
) -> BrowserPlanResult {
    if !browser_skill.enabled {
        return BrowserPlanResult::None;
    }

    let mut sub_matches = match_sub_commands(transcript, &browser_skill.sub_commands);
    if let Some(browser_match) = browser_match {
        sub_matches.retain(|command_match| command_match.start >= browser_match.end);
    }

    let selected_match = sub_matches
        .iter()
        .find(|command_match| command_match.sub_command_id != BROWSER_OPEN_TARGET_SUB_COMMAND_ID)
        .or_else(|| sub_matches.first());

    if let Some(first_match) = selected_match {
        let next_start = sub_matches
            .iter()
            .filter(|next_match| next_match.start > first_match.start)
            .map(|next_match| next_match.start)
            .next();
        let consumed_end = next_start.unwrap_or(transcript.len());
        let note = if sub_matches.len() > 1 {
            Some(format!(
                "检测到多个浏览器动作，已执行第一个：{}",
                first_match.name
            ))
        } else {
            None
        };

        return match first_match.sub_command_id.as_str() {
            BROWSER_OPEN_TARGET_SUB_COMMAND_ID => {
                let next_start = sub_matches
                    .iter()
                    .filter(|next_match| next_match.start > first_match.start)
                    .map(|next_match| next_match.start)
                    .next();
                let query = extract_freeform_query(transcript, first_match.end, next_start);
                if query.is_empty() {
                    BrowserPlanResult::Feedback("未识别到要打开的网站或网址".to_string())
                } else {
                    BrowserPlanResult::Action(BrowserActionPlan {
                        action: BrowserAction::OpenTarget { query },
                        action_name: first_match.name.clone(),
                        note,
                        consumed_end,
                    })
                }
            }
            BROWSER_NEW_TAB_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::NewTab,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_CLOSE_TAB_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::CloseTab,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_NEXT_TAB_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::NextTab,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_PREVIOUS_TAB_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::PreviousTab,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_SWITCH_TAB_INDEX_SUB_COMMAND_ID => {
                let next_start = sub_matches
                    .iter()
                    .filter(|next_match| next_match.start > first_match.start)
                    .map(|next_match| next_match.start)
                    .next();
                let query = extract_freeform_query(transcript, first_match.end, next_start);
                match parse_tab_index(&query) {
                    Some(index) => BrowserPlanResult::Action(BrowserActionPlan {
                        action: BrowserAction::SwitchTabIndex { index },
                        action_name: first_match.name.clone(),
                        note,
                        consumed_end,
                    }),
                    None => BrowserPlanResult::Feedback("未识别到要切换到第几个页面".to_string()),
                }
            }
            BROWSER_REOPEN_TAB_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::ReopenTab,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_CLOSE_OTHER_TABS_SUB_COMMAND_ID => {
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::CloseOtherTabs,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            BROWSER_CLOSE_TABS_TO_RIGHT_SUB_COMMAND_ID => {
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::CloseTabsToRight,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            BROWSER_GO_BACK_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::GoBack,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_GO_FORWARD_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::GoForward,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_REFRESH_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::Refresh,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_HARD_REFRESH_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::HardRefresh,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_STOP_LOADING_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::StopLoading,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_GO_HOME_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::GoHome,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_SCROLL_UP_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::ScrollUp,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_SCROLL_DOWN_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::ScrollDown,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_SCROLL_TOP_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::ScrollTop,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_SCROLL_BOTTOM_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::ScrollBottom,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_PAGE_UP_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::PageUp,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_PAGE_DOWN_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::PageDown,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_FIND_SUB_COMMAND_ID => {
                let next_start = sub_matches
                    .iter()
                    .filter(|next_match| next_match.start > first_match.start)
                    .map(|next_match| next_match.start)
                    .next();
                let query = extract_freeform_query(transcript, first_match.end, next_start);
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::Find {
                        query: (!query.is_empty()).then_some(query),
                    },
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            BROWSER_FULLSCREEN_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::Fullscreen,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_COPY_URL_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::CopyUrl,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_OPEN_HISTORY_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::OpenHistory,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_OPEN_DOWNLOADS_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::OpenDownloads,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_OPEN_DEVTOOLS_SUB_COMMAND_ID => BrowserPlanResult::Action(BrowserActionPlan {
                action: BrowserAction::OpenDevtools,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            BROWSER_MINIMIZE_WINDOW_SUB_COMMAND_ID => {
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::MinimizeWindow,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            BROWSER_MAXIMIZE_WINDOW_SUB_COMMAND_ID => {
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::MaximizeWindow,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            BROWSER_NEW_PRIVATE_WINDOW_SUB_COMMAND_ID => {
                BrowserPlanResult::Action(BrowserActionPlan {
                    action: BrowserAction::NewPrivateWindow,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            _ => BrowserPlanResult::None,
        };
    }

    if let Some(browser_match) = browser_match {
        let query = extract_freeform_query(transcript, browser_match.end, None);
        if query.is_empty() {
            return BrowserPlanResult::Feedback("未识别到要打开的网站或网址".to_string());
        }

        return BrowserPlanResult::Action(BrowserActionPlan {
            action: BrowserAction::OpenTarget { query },
            action_name: "打开目标".to_string(),
            note: None,
            consumed_end: transcript.len(),
        });
    }

    BrowserPlanResult::None
}

pub fn resolve_browser_site_url(browser_skill: &SkillConfig, query: &str) -> Option<String> {
    let options = browser_skill.browser_options.as_ref()?;
    let normalized_query = normalize_browser_target_token(query);
    if normalized_query.is_empty() {
        return None;
    }

    for site in options.sites.iter().filter(|site| site.enabled) {
        let alias_iter = std::iter::once(site.name.as_str()).chain(
            site.aliases
                .split(',')
                .map(str::trim)
                .filter(|alias| !alias.is_empty()),
        );

        if alias_iter
            .map(normalize_browser_target_token)
            .any(|candidate| !candidate.is_empty() && candidate == normalized_query)
        {
            return Some(site.url.trim().to_string());
        }
    }

    None
}

pub fn plan_windows_action(
    transcript: &str,
    windows_skill: &SkillConfig,
    windows_match: Option<&SkillMatch>,
) -> WindowsPlanResult {
    if !windows_skill.enabled {
        return WindowsPlanResult::None;
    }

    let mut sub_matches = match_sub_commands(transcript, &windows_skill.sub_commands);
    if let Some(windows_match) = windows_match {
        sub_matches.retain(|command_match| command_match.start >= windows_match.end);
    }

    let selected_match = sub_matches
        .iter()
        .find(|command_match| command_match.sub_command_id != WINDOWS_OPEN_TARGET_SUB_COMMAND_ID)
        .or_else(|| sub_matches.first());

    if let Some(first_match) = selected_match {
        let next_start = sub_matches
            .iter()
            .filter(|next_match| next_match.start > first_match.start)
            .map(|next_match| next_match.start)
            .next();
        let consumed_end = next_start.unwrap_or(transcript.len());
        let note = if sub_matches.len() > 1 {
            Some(format!(
                "Detected multiple Windows actions. Executing the first one: {}",
                first_match.name
            ))
        } else {
            None
        };

        return match first_match.sub_command_id.as_str() {
            WINDOWS_OPEN_TARGET_SUB_COMMAND_ID => {
                let query = extract_freeform_query(transcript, first_match.end, next_start);
                if query.is_empty() {
                    WindowsPlanResult::Feedback(
                        "No Windows target was recognized to open".to_string(),
                    )
                } else {
                    WindowsPlanResult::Action(WindowsActionPlan {
                        action: WindowsAction::OpenTarget { query },
                        action_name: first_match.name.clone(),
                        note,
                        consumed_end,
                    })
                }
            }
            WINDOWS_SHOW_DESKTOP_SUB_COMMAND_ID => WindowsPlanResult::Action(WindowsActionPlan {
                action: WindowsAction::ShowDesktop,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            WINDOWS_LOCK_SCREEN_SUB_COMMAND_ID => WindowsPlanResult::Action(WindowsActionPlan {
                action: WindowsAction::LockScreen,
                action_name: first_match.name.clone(),
                note,
                consumed_end,
            }),
            WINDOWS_OPEN_RUN_DIALOG_SUB_COMMAND_ID => {
                WindowsPlanResult::Action(WindowsActionPlan {
                    action: WindowsAction::OpenRunDialog,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            WINDOWS_OPEN_CLIPBOARD_HISTORY_SUB_COMMAND_ID => {
                WindowsPlanResult::Action(WindowsActionPlan {
                    action: WindowsAction::OpenClipboardHistory,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            WINDOWS_OPEN_QUICK_SETTINGS_SUB_COMMAND_ID => {
                WindowsPlanResult::Action(WindowsActionPlan {
                    action: WindowsAction::OpenQuickSettings,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            WINDOWS_OPEN_NOTIFICATIONS_SUB_COMMAND_ID => {
                WindowsPlanResult::Action(WindowsActionPlan {
                    action: WindowsAction::OpenNotifications,
                    action_name: first_match.name.clone(),
                    note,
                    consumed_end,
                })
            }
            _ => WindowsPlanResult::None,
        };
    }

    if let Some(windows_match) = windows_match {
        let query = extract_freeform_query(transcript, windows_match.end, None);
        if query.is_empty() {
            return WindowsPlanResult::Feedback(
                "No Windows target was recognized to open".to_string(),
            );
        }

        return WindowsPlanResult::Action(WindowsActionPlan {
            action: WindowsAction::OpenTarget { query },
            action_name: "Open Windows Target".to_string(),
            note: None,
            consumed_end: transcript.len(),
        });
    }

    WindowsPlanResult::None
}

pub fn resolve_windows_target(
    windows_skill: &SkillConfig,
    query: &str,
) -> Option<WindowsTargetConfig> {
    let options = windows_skill.windows_options.as_ref()?;
    let normalized_query = normalize_browser_target_token(query);
    if normalized_query.is_empty() {
        return None;
    }

    options
        .targets
        .iter()
        .filter(|target| target.enabled)
        .find(|target| {
            std::iter::once(target.name.as_str())
                .chain(
                    target
                        .aliases
                        .split(',')
                        .map(str::trim)
                        .filter(|alias| !alias.is_empty()),
                )
                .map(normalize_browser_target_token)
                .any(|candidate| !candidate.is_empty() && candidate == normalized_query)
        })
        .cloned()
}

pub fn resolve_windows_target_by_id(
    windows_skill: &SkillConfig,
    target_id: &str,
) -> Option<WindowsTargetConfig> {
    let options = windows_skill.windows_options.as_ref()?;
    options
        .targets
        .iter()
        .find(|target| target.enabled && target.id == target_id)
        .cloned()
}

pub fn normalize_browser_url(raw: &str) -> Result<String, String> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return Err("URL 为空".to_string());
    }
    if candidate.chars().any(char::is_whitespace) {
        return Err("URL 包含空白字符".to_string());
    }

    if let Ok(url) = Url::parse(candidate) {
        return validate_safe_browser_url(url);
    }

    if candidate.contains("://") {
        return Err("仅支持 http 或 https 地址".to_string());
    }

    let prefixed = format!("https://{}", candidate);
    let url = Url::parse(&prefixed).map_err(|_| "无法识别为可打开的网址".to_string())?;

    let host = url.host_str().unwrap_or_default();
    if !is_likely_public_host(host) {
        return Err("无法识别为可打开的网址".to_string());
    }

    validate_safe_browser_url(url)
}

pub fn build_browser_search_url(template: &str, query: &str) -> Result<String, String> {
    let encoded_query: String =
        url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
    let resolved = if template.contains("{query}") {
        template.replace("{query}", &encoded_query)
    } else {
        format!("{}{}", template, encoded_query)
    };
    normalize_browser_url(&resolved)
}

pub fn execute_skill(skill_id: &str) -> Result<(), String> {
    match BuiltinSkillId::from_str(skill_id) {
        Some(BuiltinSkillId::ComposeEmail) => {
            println!("[SKILL] Executing: compose_email");
            Command::new("cmd")
                .args(["/C", "start", "", "mailto:"])
                .spawn()
                .map_err(|e| format!("Failed to open email client: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenCalculator) => {
            println!("[SKILL] Executing: open_calculator");
            Command::new("calc")
                .spawn()
                .map_err(|e| format!("Failed to open calculator: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenBrowser) => {
            Err("Browser skill requires sub-command planning before execution".to_string())
        }
        Some(BuiltinSkillId::OpenWindows) => {
            Err("Windows skill requires sub-command planning before execution".to_string())
        }
        Some(BuiltinSkillId::OpenNotepad) => {
            println!("[SKILL] Executing: open_notepad");
            Command::new("notepad")
                .spawn()
                .map_err(|e| format!("Failed to open notepad: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::OpenExplorer) => {
            println!("[SKILL] Executing: open_explorer");
            Command::new("explorer")
                .spawn()
                .map_err(|e| format!("Failed to open explorer: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::Screenshot) => {
            println!("[SKILL] Executing: screenshot (Win+Shift+S)");
            Command::new("powershell")
                .args([
                    "-Command",
                    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('+#{s}')",
                ])
                .spawn()
                .map_err(|e| format!("Failed to trigger screenshot: {}", e))?;
            Ok(())
        }
        Some(BuiltinSkillId::SwitchPolishScene) => Err(format!(
            "Config skill '{}' must be handled by the transcription pipeline",
            skill_id
        )),
        None => Err(format!("Unknown skill: {}", skill_id)),
    }
}

pub fn open_browser_url(url: &str) -> Result<(), String> {
    let normalized = normalize_browser_url(url)?;
    println!("[SKILL] Opening browser URL: {}", normalized);
    Command::new("cmd")
        .args(["/C", "start", "", normalized.as_str()])
        .spawn()
        .map_err(|e| format!("Failed to open browser URL: {}", e))?;
    Ok(())
}

pub fn open_windows_target(target: &WindowsTargetConfig) -> Result<(), String> {
    println!(
        "[SKILL] Opening Windows target '{}' via {:?}: {}",
        target.id, target.launch_kind, target.launch_target
    );

    match target.launch_kind {
        WindowsLaunchKind::Command => Command::new(&target.launch_target)
            .args(&target.launch_args)
            .spawn()
            .map_err(|e| format!("Failed to open Windows target '{}': {}", target.name, e))?,
        WindowsLaunchKind::Shell => Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(&target.launch_target)
            .args(&target.launch_args)
            .spawn()
            .map_err(|e| format!("Failed to open Windows target '{}': {}", target.name, e))?,
    };

    Ok(())
}

pub fn execute_browser_shortcut_action(action: &BrowserAction) -> Result<(), String> {
    use crate::keyboard::{send_key_click, send_key_press, send_key_release, Key};

    let send_chord = |modifiers: &[Key], key: Key| -> Result<(), String> {
        for modifier in modifiers {
            send_key_press(*modifier);
        }
        send_key_click(key);
        for modifier in modifiers.iter().rev() {
            send_key_release(*modifier);
        }
        Ok(())
    };

    match action {
        BrowserAction::OpenTarget { .. } => {
            Err("OpenTarget must be executed via URL navigation".to_string())
        }
        BrowserAction::NewTab => send_chord(&[Key::Control], Key::Unicode('t')),
        BrowserAction::CloseTab => send_chord(&[Key::Control], Key::Unicode('w')),
        BrowserAction::NextTab => send_chord(&[Key::Control], Key::Tab),
        BrowserAction::PreviousTab => send_chord(&[Key::Control, Key::Shift], Key::Tab),
        BrowserAction::SwitchTabIndex { index } => {
            let normalized = (*index).clamp(1, 9);
            let key = if normalized >= 9 {
                Key::Unicode('9')
            } else {
                Key::Unicode(char::from_digit(normalized as u32, 10).unwrap_or('1'))
            };
            send_chord(&[Key::Control], key)
        }
        BrowserAction::ReopenTab => {
            send_chord(&[Key::Control, Key::Shift], Key::Unicode('t'))
        }
        BrowserAction::CloseOtherTabs | BrowserAction::CloseTabsToRight => {
            Err("当前快捷键模式暂不支持这个浏览器操作".to_string())
        }
        BrowserAction::GoBack => send_chord(&[Key::Alt], Key::LeftArrow),
        BrowserAction::GoForward => send_chord(&[Key::Alt], Key::RightArrow),
        BrowserAction::Refresh => Ok(send_key_click(Key::F5)),
        BrowserAction::HardRefresh => send_chord(&[Key::Control], Key::F5),
        BrowserAction::StopLoading => Ok(send_key_click(Key::Escape)),
        BrowserAction::GoHome => send_chord(&[Key::Alt], Key::Home),
        BrowserAction::ScrollUp => Ok(send_key_click(Key::UpArrow)),
        BrowserAction::ScrollDown => Ok(send_key_click(Key::DownArrow)),
        BrowserAction::ScrollTop => send_chord(&[Key::Control], Key::Home),
        BrowserAction::ScrollBottom => send_chord(&[Key::Control], Key::End),
        BrowserAction::PageUp => Ok(send_key_click(Key::PageUp)),
        BrowserAction::PageDown => Ok(send_key_click(Key::PageDown)),
        BrowserAction::Find { query } => {
            send_chord(&[Key::Control], Key::Unicode('f'))?;
            if let Some(query) = query {
                for ch in query.chars() {
                    send_key_click(Key::Unicode(ch));
                }
            }
            Ok(())
        }
        BrowserAction::Fullscreen => Ok(send_key_click(Key::F11)),
        BrowserAction::CopyUrl => {
            send_chord(&[Key::Control], Key::Unicode('l'))?;
            send_chord(&[Key::Control], Key::Unicode('c'))
        }
        BrowserAction::OpenHistory => send_chord(&[Key::Control], Key::Unicode('h')),
        BrowserAction::OpenDownloads => send_chord(&[Key::Control], Key::Unicode('j')),
        BrowserAction::OpenDevtools => Ok(send_key_click(Key::F12)),
        BrowserAction::MinimizeWindow => send_chord(&[Key::Meta], Key::DownArrow),
        BrowserAction::MaximizeWindow => send_chord(&[Key::Meta], Key::UpArrow),
        BrowserAction::NewPrivateWindow => {
            send_chord(&[Key::Control, Key::Shift], Key::Unicode('n'))
        }
    }
}

pub fn execute_windows_shortcut_action(action: &WindowsAction) -> Result<(), String> {
    use crate::keyboard::{send_key_click, send_key_press, send_key_release, Key};

    let send_chord = |modifiers: &[Key], key: Key| -> Result<(), String> {
        for modifier in modifiers {
            send_key_press(*modifier);
        }
        send_key_click(key);
        for modifier in modifiers.iter().rev() {
            send_key_release(*modifier);
        }
        Ok(())
    };

    match action {
        WindowsAction::OpenTarget { .. } => {
            Err("OpenTarget must be executed via Windows target resolution".to_string())
        }
        WindowsAction::LockScreen => Command::new("rundll32.exe")
            .args(["user32.dll,LockWorkStation"])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to lock screen: {}", e)),
        other_action => {
            match other_action {
                WindowsAction::ShowDesktop => send_chord(&[Key::Meta], Key::Unicode('d')),
                WindowsAction::OpenRunDialog => send_chord(&[Key::Meta], Key::Unicode('r')),
                WindowsAction::OpenClipboardHistory => send_chord(&[Key::Meta], Key::Unicode('v')),
                WindowsAction::OpenQuickSettings => send_chord(&[Key::Meta], Key::Unicode('a')),
                WindowsAction::OpenNotifications => send_chord(&[Key::Meta], Key::Unicode('n')),
                WindowsAction::OpenTarget { .. } | WindowsAction::LockScreen => unreachable!(),
            }
        }
    }
}

pub fn extract_scene_query(
    transcript: &str,
    current: &SkillMatch,
    next: Option<&SkillMatch>,
) -> String {
    let end = next
        .map(|skill_match| skill_match.start)
        .unwrap_or(transcript.len());
    if current.end >= end
        || current.end > transcript.len()
        || !transcript.is_char_boundary(current.end)
        || !transcript.is_char_boundary(end)
    {
        return String::new();
    }

    transcript[current.end..end]
        .trim_matches(is_scene_boundary_char)
        .to_string()
}

pub fn resolve_scene(profiles: &[PromptProfile], raw_query: &str) -> SceneResolveResult {
    let normalized_query = normalize_scene_token(raw_query);
    if normalized_query.is_empty() {
        return SceneResolveResult::None;
    }

    let alias_matches = collect_scene_matches(profiles, &normalized_query, true);
    if !alias_matches.is_empty() {
        return collapse_scene_matches(alias_matches);
    }

    collapse_scene_matches(collect_scene_matches(profiles, &normalized_query, false))
}

fn collect_scene_matches(
    profiles: &[PromptProfile],
    normalized_query: &str,
    use_aliases: bool,
) -> Vec<(String, String)> {
    let mut matches = Vec::new();
    let mut seen_ids = HashSet::new();

    for profile in profiles {
        let candidates: Vec<&str> = if use_aliases {
            profile.voice_aliases.iter().map(String::as_str).collect()
        } else {
            vec![profile.name.as_str()]
        };

        if candidates
            .into_iter()
            .map(normalize_scene_token)
            .any(|candidate| !candidate.is_empty() && candidate == normalized_query)
            && seen_ids.insert(profile.id.clone())
        {
            matches.push((profile.id.clone(), profile.name.clone()));
        }
    }

    matches
}

fn collapse_scene_matches(matches: Vec<(String, String)>) -> SceneResolveResult {
    match matches.len() {
        0 => SceneResolveResult::None,
        1 => {
            let (profile_id, profile_name) = matches.into_iter().next().unwrap();
            SceneResolveResult::Unique {
                profile_id,
                profile_name,
            }
        }
        _ => {
            let mut names: Vec<String> = matches.into_iter().map(|(_, name)| name).collect();
            names.sort();
            names.dedup();
            SceneResolveResult::Ambiguous(names)
        }
    }
}

pub fn normalize_scene_token(value: &str) -> String {
    let mut normalized: String = value
        .chars()
        .filter(|ch| !is_ignored_scene_char(*ch))
        .flat_map(|ch| ch.to_lowercase())
        .collect();

    loop {
        if let Some(stripped) = normalized.strip_suffix("场景") {
            normalized = stripped.to_string();
            continue;
        }
        if let Some(stripped) = normalized.strip_suffix("模式") {
            normalized = stripped.to_string();
            continue;
        }
        break;
    }

    normalized
}

fn extract_freeform_query(transcript: &str, start: usize, end: Option<usize>) -> String {
    let end = end.unwrap_or(transcript.len());
    if start >= end
        || start > transcript.len()
        || end > transcript.len()
        || !transcript.is_char_boundary(start)
        || !transcript.is_char_boundary(end)
    {
        return String::new();
    }

    let mut query = transcript[start..end]
        .trim_matches(is_browser_query_boundary_char)
        .trim()
        .to_string();

    loop {
        let mut stripped = false;
        for suffix in ["然后", "再", "并且", "并", "接着"] {
            if let Some(candidate) = query.strip_suffix(suffix) {
                query = candidate
                    .trim_matches(is_browser_query_boundary_char)
                    .trim()
                    .to_string();
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }

    query
}

fn parse_tab_index(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() {
        return digits.parse::<usize>().ok().filter(|index| *index >= 1);
    }

    let normalized = trimmed
        .replace(['第', '个'], "")
        .replace("页面", "")
        .replace("标签页", "")
        .replace("标签", "")
        .replace("页", "")
        .trim()
        .to_string();

    let value = match normalized.as_str() {
        "一" | "壹" | "幺" => 1,
        "二" | "两" | "贰" => 2,
        "三" | "叁" => 3,
        "四" | "肆" => 4,
        "五" | "伍" => 5,
        "六" | "陆" => 6,
        "七" | "柒" => 7,
        "八" | "捌" => 8,
        "九" | "玖" => 9,
        "十" => 10,
        _ => return None,
    };

    Some(value)
}

fn validate_safe_browser_url(url: Url) -> Result<String, String> {
    match url.scheme() {
        "http" | "https" => Ok(url.to_string()),
        _ => Err("仅支持 http 或 https 地址".to_string()),
    }
}

fn is_likely_public_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    if host.parse::<IpAddr>().is_ok() {
        return true;
    }

    host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
}

fn normalize_browser_target_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !is_ignored_scene_char(*ch))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn is_ignored_scene_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | ':'
                | ';'
                | '!'
                | '?'
                | '/'
                | '\\'
                | '|'
                | '-'
                | '_'
                | '，'
                | '。'
                | '：'
                | '；'
                | '！'
                | '？'
                | '、'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '"'
                | '\''
                | '('
                | ')'
                | '（'
                | '）'
        )
}

fn is_scene_boundary_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.' | ':' | ';' | '!' | '?' | '，' | '。' | '：' | '；' | '！' | '？' | '、'
        )
}

fn is_browser_query_boundary_char(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | ':'
                | ';'
                | '!'
                | '?'
                | '，'
                | '。'
                | '：'
                | '；'
                | '！'
                | '？'
                | '、'
                | '"'
                | '\''
                | '“'
                | '”'
                | '‘'
                | '’'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill(id: &str, keywords: &str) -> SkillConfig {
        SkillConfig {
            id: id.to_string(),
            name: id.to_string(),
            keywords: keywords.to_string(),
            enabled: true,
            sub_commands: Vec::new(),
            browser_options: None,
            windows_options: None,
        }
    }

    fn matched_ids(text: &str, skills: &[SkillConfig]) -> Vec<String> {
        match_skills(text, skills)
            .into_iter()
            .map(|skill_match| skill_match.skill_id)
            .collect()
    }

    #[test]
    fn test_match_skills_single() {
        let skills = vec![
            test_skill("compose_email", "compose email,write email"),
            test_skill("open_calculator", "calculator,open calculator"),
        ];

        assert_eq!(
            matched_ids("compose email", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            matched_ids("please write email to the team", &skills),
            vec!["compose_email".to_string()]
        );
        assert_eq!(
            matched_ids("open calculator", &skills),
            vec!["open_calculator".to_string()]
        );
        assert!(matched_ids("the weather is nice today", &skills).is_empty());
    }

    #[test]
    fn test_match_skills_multiple_in_transcript_order() {
        let skills = vec![
            test_skill("open_calculator", "calculator,open calculator"),
            test_skill(OPEN_BROWSER_SKILL_ID, "browser,open browser"),
        ];

        assert_eq!(
            matched_ids("browser and calculator", &skills),
            vec![
                OPEN_BROWSER_SKILL_ID.to_string(),
                "open_calculator".to_string()
            ]
        );
    }

    #[test]
    fn test_match_skills_deduplicates_per_skill() {
        let skills = vec![test_skill(OPEN_BROWSER_SKILL_ID, "browser,open browser")];

        assert_eq!(
            matched_ids("please open browser in the browser", &skills),
            vec![OPEN_BROWSER_SKILL_ID.to_string()]
        );
    }

    #[test]
    fn merge_with_default_skills_backfills_browser_settings() {
        let legacy_browser = SkillConfig {
            id: OPEN_BROWSER_SKILL_ID.to_string(),
            name: "浏览器".to_string(),
            keywords: "打开浏览器".to_string(),
            enabled: true,
            sub_commands: Vec::new(),
            browser_options: None,
            windows_options: None,
        };

        let (merged, changed) = merge_with_default_skills(vec![legacy_browser]);
        let browser = find_skill_config(&merged, OPEN_BROWSER_SKILL_ID).expect("browser skill");

        assert!(changed);
        assert_eq!(
            browser.sub_commands.len(),
            default_browser_sub_commands().len()
        );
        assert!(browser.browser_options.is_some());
        assert!(browser
            .browser_options
            .as_ref()
            .expect("browser options")
            .sites
            .iter()
            .any(|site| site.id == "bilibili"));
    }

    #[test]
    fn merge_with_default_skills_appends_missing_builtin_keywords() {
        let legacy_switch_scene = SkillConfig {
            id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
            name: "切换润色场景".to_string(),
            keywords: "切换到,切到,使用".to_string(),
            enabled: true,
            sub_commands: Vec::new(),
            browser_options: None,
            windows_options: None,
        };

        let (merged, changed) = merge_with_default_skills(vec![legacy_switch_scene]);
        let switch_scene =
            find_skill_config(&merged, SWITCH_POLISH_SCENE_SKILL_ID).expect("switch scene skill");

        assert!(changed);
        assert!(switch_scene.keywords.contains("切换成"));
        assert!(switch_scene.keywords.contains("切成"));
    }

    #[test]
    fn browser_plan_prefers_first_sub_command() {
        let browser = find_skill_config(&get_default_skills(), OPEN_BROWSER_SKILL_ID)
            .expect("browser skill")
            .clone();

        let plan = plan_browser_action("打开 github 然后关闭页面", &browser, None);
        match plan {
            BrowserPlanResult::Action(plan) => {
                assert_eq!(plan.action, BrowserAction::CloseTab);
                assert!(plan.note.is_some());
            }
            other => panic!("unexpected plan: {:?}", other),
        }
    }

    #[test]
    fn browser_plan_prefers_specific_sub_command_over_open_target() {
        let browser = find_skill_config(&get_default_skills(), OPEN_BROWSER_SKILL_ID)
            .expect("browser skill")
            .clone();

        let plan = plan_browser_action("打开开发者工具", &browser, None);
        match plan {
            BrowserPlanResult::Action(plan) => {
                assert_eq!(plan.action, BrowserAction::OpenDevtools);
            }
            other => panic!("unexpected plan: {:?}", other),
        }
    }

    #[test]
    fn browser_plan_uses_parent_match_when_only_browser_keyword_present() {
        let browser = find_skill_config(&get_default_skills(), OPEN_BROWSER_SKILL_ID)
            .expect("browser skill")
            .clone();
        let browser_match = SkillMatch {
            skill_id: OPEN_BROWSER_SKILL_ID.to_string(),
            keyword: "打开浏览器".to_string(),
            start: 0,
            end: "打开浏览器".len(),
        };

        let plan = plan_browser_action("打开浏览器 github.com", &browser, Some(&browser_match));
        match plan {
            BrowserPlanResult::Action(plan) => {
                assert_eq!(
                    plan.action,
                    BrowserAction::OpenTarget {
                        query: "github.com".to_string()
                    }
                );
            }
            other => panic!("unexpected plan: {:?}", other),
        }
    }

    #[test]
    fn resolve_browser_site_matches_alias() {
        let browser = find_skill_config(&get_default_skills(), OPEN_BROWSER_SKILL_ID)
            .expect("browser skill")
            .clone();

        assert_eq!(
            resolve_browser_site_url(&browser, "B站"),
            Some("https://www.bilibili.com".to_string())
        );
    }

    #[test]
    fn normalize_browser_url_accepts_bare_domain() {
        assert_eq!(
            normalize_browser_url("github.com").expect("normalized"),
            "https://github.com/"
        );
    }

    #[test]
    fn normalize_browser_url_rejects_dangerous_scheme() {
        assert!(normalize_browser_url("javascript:alert(1)").is_err());
        assert!(normalize_browser_url("file:///c:/windows").is_err());
    }

    #[test]
    fn build_browser_search_url_replaces_query_placeholder() {
        assert_eq!(
            build_browser_search_url("https://www.bing.com/search?q={query}", "飞书 文档")
                .expect("search url"),
            "https://www.bing.com/search?q=%E9%A3%9E%E4%B9%A6+%E6%96%87%E6%A1%A3"
        );
    }

    #[test]
    fn parse_tab_index_supports_digits_and_chinese_numbers() {
        assert_eq!(parse_tab_index("3个页面"), Some(3));
        assert_eq!(parse_tab_index("第六个标签页"), Some(6));
    }

    #[test]
    fn windows_plan_extracts_open_target() {
        let windows = find_skill_config(&get_default_skills(), OPEN_WINDOWS_SKILL_ID)
            .expect("windows skill")
            .clone();

        let plan = plan_windows_action("打开任务管理器", &windows, None);
        match plan {
            WindowsPlanResult::Action(plan) => {
                assert_eq!(
                    plan.action,
                    WindowsAction::OpenTarget {
                        query: "任务管理器".to_string()
                    }
                );
            }
            other => panic!("unexpected plan: {:?}", other),
        }
    }

    #[test]
    fn resolve_windows_target_matches_alias() {
        let windows = find_skill_config(&get_default_skills(), OPEN_WINDOWS_SKILL_ID)
            .expect("windows skill")
            .clone();

        let target = resolve_windows_target(&windows, "任务管理器").expect("windows target");
        assert_eq!(target.id, "task_manager");
    }

    #[test]
    fn test_extract_scene_query_stops_before_next_skill() {
        let matches = vec![
            SkillMatch {
                skill_id: "dummy".to_string(),
                keyword: "启用润色".to_string(),
                start: 0,
                end: "启用润色".len(),
            },
            SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: "启用润色".len(),
                end: "启用润色切换到".len(),
            },
        ];

        assert_eq!(
            extract_scene_query("启用润色切换到邮件", &matches[1], None),
            "邮件".to_string()
        );
    }

    #[test]
    fn test_resolve_scene_prefers_aliases() {
        let profiles = vec![
            PromptProfile {
                id: "email".to_string(),
                name: "邮件写作".to_string(),
                voice_aliases: vec!["邮件".to_string()],
                ..PromptProfile::new_default()
            },
            PromptProfile {
                id: "notes".to_string(),
                name: "邮件".to_string(),
                voice_aliases: vec![],
                ..PromptProfile::new_default()
            },
        ];

        assert_eq!(
            resolve_scene(&profiles, "邮件场景"),
            SceneResolveResult::Unique {
                profile_id: "email".to_string(),
                profile_name: "邮件写作".to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_scene_detects_alias_conflicts() {
        let profiles = vec![
            PromptProfile {
                id: "email".to_string(),
                name: "邮件".to_string(),
                voice_aliases: vec!["客服".to_string()],
                ..PromptProfile::new_default()
            },
            PromptProfile {
                id: "support".to_string(),
                name: "客服".to_string(),
                voice_aliases: vec!["客服".to_string()],
                ..PromptProfile::new_default()
            },
        ];

        assert_eq!(
            resolve_scene(&profiles, "客服"),
            SceneResolveResult::Ambiguous(vec!["客服".to_string(), "邮件".to_string()])
        );
    }
}
