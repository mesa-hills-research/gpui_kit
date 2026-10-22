---
title: Text Selection
description: 在多个自绘文本参与者之间实现窗口级文本选择。
order: 3
---

# Text Selection

`gpui-base` 的文本选择基础设施允许一次拖拽跨越多个独立绘制的文本块，同时保留每个参与者自己的布局与绘制逻辑。它适合文档、消息流和其他由多个自定义元素组成、但用户期望像连续文本一样选择的界面。

## 可选择的高亮文本

`SelectableText` 为普通文本段处理布局和窗口级选择。`highlights` builder
按范围应用 `HighlightStyle`，原始文本仍可选择和复制。范围使用 UTF-8 字节偏移，
必须按顺序排列、互不重叠，且起止位置都位于字符边界。再次调用会替换之前的高亮。

```rust
use gpui_kit::{FontWeight, HighlightStyle};
use gpui_kit::base::SelectableText;

SelectableText::new("result", "Find matching text")
    .highlights([(5..13, HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    })]);
```

保持元素 ID 在多次渲染之间稳定。多个文本段组成同一可选择文档时，使用
`SelectableText::with_handle` 和 `document_order`。Kit 窗口已安装选择层，
仅使用 Base 的窗口需要按下文安装。

`SelectableText::layout` 返回绘制该文本段所用的 `TextLayout`。包装该文本段的元素可以在
`prepaint` 或 `paint` 中读取它，例如用 `position_for_index` 找到搜索匹配所在的行，
无需再次排版文本。

## 开始使用

核心由窗口级 `TextSelectionState`、稳定的 `SelectableTextHandle`、prepaint 阶段注册的几何信息，以及把全局选择投影到各文本 run 的辅助 API 组成。

## 工作方式

每个参与者在 prepaint 时报告文本、屏幕几何和逻辑顺序。窗口状态根据指针锚点与当前点计算跨参与者的选择区间；绘制时，各参与者只查询落在自身范围内的切片并绘制选中背景。状态更新发生在事件回调中，不在 `render` 中写入。

## 安装窗口元素

在窗口内容的稳定外层安装选择宿主，使它能够接收拖拽、释放和复制操作。宿主应覆盖所有需要共同选择的参与者，但不应改变它们的布局或样式。

## 创建稳定句柄

每个参与者的 `SelectableTextHandle` 应保存在 entity 中或由稳定数据键派生。不要在每次渲染时创建新身份，否则拖拽途中重绘会丢失锚点、逻辑顺序或选择投影。

## 在 prepaint 注册几何

文本最终布局完成后注册边界、行与字符位置。注册顺序必须与用户看到和辅助技术读取的逻辑顺序一致。动态插入、删除或移动参与者时，用稳定身份更新对应记录。

当前帧没有注册的句柄会自动退出选择。如果参与者是同一文档的多个视图之一（例如列表中一篇长回复的各行），可以用 `with_document_position(document, position)` 说明它的位置。同一文档的参与者跨帧按位置排序，被选区覆盖时滚出视野的参与者会一直留在选区中，直到选区结束。`TextView::document_position` 和 `SelectableText::document_position` 为这两种元素设置位置，因此对话中的各条消息这类独立视图也能组成一个文档。离开当前帧的 `SelectableText` 只有在用 `SelectableText::with_handle` 创建、并且应用保留了句柄时，才会留在选区中。其中持有选区端点的那一个只复制被选中的部分，位置可以是任意 `u64`。

## 把选择投影到文本 run

绘制各 run 前查询当前窗口选择，取得与本地文本相交的字节或字符范围，再把范围转换为文本系统需要的高亮几何。注意 UTF-8 边界，不要把字节偏移当作字符索引。

## 完整 Rust 示例

```bash
cargo run -p gpui-base-examples -- text-selection
```

<<< ../../../crates/base/examples/showcase/components/text_selection.rs{rust}

## 查询与控制窗口选择

应用可以读取当前选中文本、主动清除选择，并把复制命令连接到窗口状态。程序化更新后调用 `cx.notify()`，让所有受影响参与者重绘。

### 触摸选择

在参与者上长按会选中手指下的单词；抬起手指后，这个选区成为*触摸选区*：两端各带一个拖动 handle，并附带编辑菜单。手势和拖动由 Base 负责；展示层根据 `TextSelectionSnapshot` 之外的 `TouchSelectionSnapshot` 绘制 handle 和菜单，它以窗口坐标提供选区两端的光标行框。

通过 `TextSelection::touch_selection` 读取快照，用 `TextSelection::observe_touch_selection` 在快照变化时重绘；`begin_edge_drag`、`update_edge_drag`、`end_edge_drag` 拖动其中一端，`select_all` 全选被按下的参与者，`close_edit_menu` 在菜单自身的命令执行后关闭菜单。handle 由参与者自己绘制，位于它在绘制顺序中的位置，因此盖住文字的东西也会盖住 handle：在 prepaint 调用 `TextSelectionHandle::prepaint_touch_handles`（插入手指可按的 hitbox），在 paint 末尾、`register` 之后调用 `TextSelectionHandle::paint_touch_handles` 并传入选区颜色；`TextView` 已完成这两步。参与者通过 `TextSelectionRegistration::with_selection_edges` 上报其选区两端的绘制位置。绘制菜单的一方需要在每帧 paint 时调用 `TextSelection::register_touch_ui` 登记菜单 bounds，这样落在菜单上的按压不会清除它所属的选区；GPUI Component 的 `Root` 负责绘制菜单。

### 上下文菜单

在参与者上松开右键时，会向应用请求可选择文字的上下文菜单。在获得焦点的 `TextView` 中按 Shift-F10 或 Menu 键也一样。右键不会改变选区，无论落在哪里。指针下接管这次按压的控件会保留它。这样的控件在按下右键时调用 `GlobalState::suppress_text_selection(cx)`（GPUI Component 的输入框和 `ContextMenuExt` 都这样做），或者停止事件传播（设置了 `on_link_click` 的链接就是这样）。

```rust
use gpui_kit::base::{CopyTextSelection, SelectAllText, TextSelection};

TextSelection::on_context_menu(
    |request, window, cx| {
        // request.position()、request.selected_text()、request.has_selection()
        // 和 request.supports_select_all() 描述要显示的菜单。
        // 菜单项派发 CopyTextSelection 和 SelectAllText。
    },
    cx,
);

// 在其他地方打开菜单，效果与在 position 处右键相同。
TextSelection::show_context_menu(&handle, position, window, cx);
```

无论焦点在哪里，`CopyTextSelection` 复制的内容都与 Cmd-C 或 Ctrl-C 相同。`SelectAllText` 全选菜单所在的参与者。两者都由 `Root` 处理。参与者通过 `TextSelectionHandle::select_all_with` 提供回调后，菜单中才会出现全选，`TextView` 已经这样做。GPUI 绘制的菜单需要在每帧 paint 时调用 `TextSelection::register_menu(bounds, window, cx)`，这样按下菜单项时选区会保留，与按下系统菜单一样。GPUI Component 安装的处理函数按应用的上下文菜单样式显示复制和全选。

窗口有选区时，获得焦点的 `TextView` 即使没有绘制也保留自己的按键，例如被列表滚走的行。`Root` 会代替它接收按键，因此 Cmd-C 或 Ctrl-C 复制选区，Cmd-A 或 Ctrl-A 全选该视图，Shift-F10 和 Menu 键在仍绘制的选区末端下方打开菜单。在此期间焦点仍留在该视图上，因此窗口的失去焦点监听器不会因它而运行。

## 高级参与者适配器

自定义文本布局可以实现参与者接口，提供命中测试、范围投影和文本提取。适配器应只桥接已有布局数据，避免在指针移动热路径中重新排版或分配大型缓冲区。

## 用 scope 隔离模态内容

Dialog、Sheet 等模态内容应使用独立 scope，避免一次选择跨越背景与前景。模态关闭后恢复原 scope，不要让已卸载参与者留在窗口注册表中。

## 集成检查清单

- 窗口只安装一个相应 scope 的选择宿主。
- 参与者身份和逻辑顺序跨渲染稳定。
- 只在 prepaint 注册最终几何，并正确处理 UTF-8。
- 拖拽更新有界，不在 render 中修改状态。
- 验证跨块拖拽、反向选择、复制、动态内容、滚动和模态隔离。
