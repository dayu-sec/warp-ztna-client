use objc2::MainThreadMarker;
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSMenu, NSMenuItemBadge};
use objc2_foundation::NSString;
use tauri::Wry;
use tauri::tray::TrayIcon;

use crate::TrayResource;

const MIN_WIDTH: f64 = 272.0;

pub fn apply(tray: &TrayIcon<Wry>, resources: &[TrayResource]) {
    let statuses: Vec<String> = resources.iter().map(|res| res.status.clone()).collect();
    let styled = tray.with_inner_tray_icon(move |tray_icon| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(status_item) = tray_icon.ns_status_item() else {
            return;
        };
        let Some(menu) = status_item.menu(mtm) else {
            return;
        };
        expand_menu(&menu, &statuses, mtm);
    });
    if let Err(cause) = styled {
        log::warn!("tray: styling menu failed: {cause}");
    }
}

fn expand_menu(menu: &NSMenu, statuses: &[String], mtm: MainThreadMarker) {
    menu.setMinimumWidth(MIN_WIDTH);
    let count = menu.numberOfItems();
    for index in 0..count {
        let Some(item) = menu.itemAtIndex(index) else {
            continue;
        };
        let Some(submenu) = item.submenu() else {
            continue;
        };
        submenu.setMinimumWidth(MIN_WIDTH);
        set_statuses(&submenu, statuses, mtm);
    }
}

fn set_statuses(menu: &NSMenu, statuses: &[String], mtm: MainThreadMarker) {
    if AnyClass::get(c"NSMenuItemBadge").is_none() {
        return;
    }
    let count = menu.numberOfItems();
    for index in 0..count {
        let Some(status) = usize::try_from(index).ok().and_then(|at| statuses.get(at)) else {
            continue;
        };
        let Some(item) = menu.itemAtIndex(index) else {
            continue;
        };
        let badge = NSMenuItemBadge::initWithString(mtm.alloc(), &NSString::from_str(status));
        item.setBadge(Some(&badge));
    }
}
