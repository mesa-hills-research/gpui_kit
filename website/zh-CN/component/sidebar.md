---
title: Sidebar
description: 一个可组合、可主题化、可自定义的侧边栏组件。
---

# Sidebar

Sidebar 是一个灵活的应用导航组件，支持折叠状态、嵌套菜单项、头部与底部区域以及响应式布局。它非常适合文件浏览器、后台管理面板和多层级导航界面。

## 导入

```rust
use gpui_kit::component::sidebar::{
    Sidebar, SidebarHeader, SidebarFooter, SidebarGroup,
    SidebarMenu, SidebarMenuItem, SidebarToggleButton
};
```

## 用法

### 基础 Sidebar

```rust
use gpui_kit::component::{sidebar::*, Side};

Sidebar::new()
    .header(
        SidebarHeader::new()
            .child("My Application")
    )
    .child(
        SidebarGroup::new("Navigation")
            .child(
                SidebarMenu::new()
                    .child(
                        SidebarMenuItem::new("Dashboard")
                            .icon(IconName::LayoutDashboard)
                            .on_click(|_, _, _| println!("Dashboard clicked"))
                    )
                    .child(
                        SidebarMenuItem::new("Settings")
                            .icon(IconName::Settings)
                            .on_click(|_, _, _| println!("Settings clicked"))
                    )
            )
    )
    .footer(
        SidebarFooter::new()
            .child("User Profile")
    )
```

### 可折叠 Sidebar

```rust
let mut collapsed = false;

Sidebar::new()
    .collapsed(collapsed)
    .collapsible(true)
    .header(
        SidebarHeader::new()
            .child(
                h_flex()
                    .child(Icon::new(IconName::Home))
                    .when(!collapsed, |this| this.child("Home"))
            )
    )
```

配合切换按钮：

```rust
SidebarToggleButton::new()
    .collapsed(collapsed)
    .on_click(|_, _, _| {
        collapsed = !collapsed;
    })
```

### 折叠过渡

Sidebar 在两种宽度之间切换时，会按主题的 `motion_tokens().duration_normal` 平滑过渡。`transition_duration` 可以修改时长，`Duration::ZERO` 表示在一帧内直接切换。开启“减少动态效果”时始终直接切换。

```rust
use std::time::Duration;

Sidebar::new("sidebar")
    .collapsed(collapsed)
    .transition_duration(Duration::ZERO)
```

如果 Sidebar 切换得很频繁，旁边的内容（例如阅读区）又会随它的宽度重新排版，建议直接切换：平滑过渡的每一帧都要重新排版这些内容。

### 嵌套菜单

```rust
SidebarMenuItem::new("Projects")
    .icon(IconName::FolderOpen)
    .active(true)
    .children([
        SidebarMenuItem::new("Web App").active(false),
        SidebarMenuItem::new("Mobile App").active(true),
        SidebarMenuItem::new("Desktop App"),
    ])
```

### 多分组

```rust
Sidebar::new()
    .child(
        SidebarGroup::new("Main")
            .child(
                SidebarMenu::new()
                    .child(SidebarMenuItem::new("Dashboard").icon(IconName::Home))
                    .child(SidebarMenuItem::new("Analytics").icon(IconName::BarChart))
            )
    )
```

### Badge 与后缀

```rust
use gpui_kit::component::{Badge, Switch};

SidebarMenuItem::new("Notifications")
    .icon(IconName::Bell)
    .suffix(
        Badge::new()
            .count(5)
            .child("5")
    )
```

### 右侧放置

```rust
Sidebar::new()
    .side(Side::Right)
    .width(300)
    .header(
        SidebarHeader::new()
            .child("Right Panel")
    )
```

### 右键菜单

```rust
use gpui_kit::component::menu::PopupMenu;

SidebarMenuItem::new("Project Files")
    .icon(IconName::Folder)
    .context_menu(|menu, _, _| {
        menu.link("Open in Editor", "https://editor.example.com")
            .separator()
            .menu_with_description("Rename", "Rename this project", Box::new(RenameAction))
            .menu_with_description("Delete", "Delete this project", Box::new(DeleteAction))
    })
```

### 自定义宽度与样式

```rust
Sidebar::new()
    .width(280)
    .border_width(2)
    .header(
        SidebarHeader::new()
            .p_4()
            .rounded(cx.theme().radius)
            .child("Custom Styled Sidebar")
    )
```

## 键盘

每个 `SidebarMenu` 只占一个 Tab 停靠点。Tab 和 Shift-Tab 会落在活动项上，没有活动项时落在第一个可用项上。方向键按显示顺序在各项之间移动焦点，已展开子菜单中的项也包括在内。

| 按键 | 行为 |
| --- | --- |
| `↑` / `↓` | 将焦点移到上一个 / 下一个可用项，到两端时环绕 |
| `Home` / `End` | 将焦点移到第一个 / 最后一个可用项 |
| `→` | 展开获得焦点的项的子菜单，已展开时进入子菜单 |
| `←` | 收起获得焦点的项已展开的子菜单，未展开时移到父项 |
| `Enter` / `Space` | 执行获得焦点的项的 `on_click` |
| `Tab` | 离开菜单 |

禁用的项会被跳过。用指针点击某项也会让它获得焦点，只有键盘焦点才显示焦点环。比窗口更高的 Sidebar 目前还不会滚动到获得焦点的项。

## 主题

Sidebar 使用一组独立的主题颜色：

```rust
cx.theme().sidebar
cx.theme().sidebar_foreground
cx.theme().sidebar_border
cx.theme().sidebar_accent
cx.theme().sidebar_accent_foreground
cx.theme().sidebar_primary
cx.theme().sidebar_primary_foreground
```

## 示例

### 文件浏览器

```rust
Sidebar::new()
    .header(
        SidebarHeader::new()
            .child(
                h_flex()
                    .gap_2()
                    .child(IconName::Folder)
                    .child("Explorer")
            )
    )
```

### 管理后台

```rust
Sidebar::new()
    .header(
        SidebarHeader::new()
            .child(
                h_flex()
                    .gap_2()
                    .child("Admin Panel")
            )
    )
```

### 设置侧栏

```rust
Sidebar::new()
    .width(300)
    .header(
        SidebarHeader::new()
            .child("Settings")
    )
```
