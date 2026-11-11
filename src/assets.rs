//! Icons embedded in the binary.
//!
//! Only the icons we name are compiled in (the full Lucide set is 1,800 files).
//! Anything else falls through to the component kit's own small default set,
//! which its widgets (editor search bar, menus) rely on.

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    AppIcons,
    [
        AArrowDown,
        AArrowUp,
        BookOpen,
        Check,
        ChevronDown,
        ChevronRight,
        ChevronUp,
        CircleCheck,
        Code,
        Columns2,
        Contrast,
        Copy,
        Download,
        Expand,
        Eye,
        FileCode,
        FilePlus,
        FileText,
        Folder,
        FolderOpen,
        FolderPlus,
        Hash,
        Info,
        Link2,
        Minus,
        MessageSquareWarning,
        Moon,
        OctagonAlert,
        PanelLeft,
        Pencil,
        Pin,
        PinOff,
        Plus,
        Quote,
        Scan,
        Search,
        Share,
        Sun,
        TextSearch,
        TriangleAlert,
        Type,
        X,
        Zap,
        ZoomIn,
        ZoomOut,
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match AppIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names = AppIcons.list(path)?;
        names.extend(Assets.list(path)?);
        Ok(names)
    }
}
