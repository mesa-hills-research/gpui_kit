---
title: Diff View
description: 显示文件的更改，包括行号、新增和删除的行、hunk 标题以及语法颜色。
---

# Diff View

Diff View 显示一个或多个文件的更改：每个文件的路径、新增和删除的行数以及一个复制按钮，hunk 标题，以及带有旧行号和新行号的各行。新增的行以低不透明度的 success 颜色为背景，删除的行以 danger 颜色为背景，并且每行还会标上 `+` 或 `-`，所以更改不会只靠颜色表达。各行使用主题的代码字体，以及按文件扩展名选择的语法颜色。长 diff 滚动流畅：只布局屏幕上的行。

## 导入

```rust
use gpui_kit::component::diff_view::{Diff, DiffView, DiffViewState};
```

## 用法

构建一个 `Diff`，放进 `DiffViewState`，再渲染 `DiffView`：

```rust
// 文件的两个版本之间：
let diff = Diff::between("src/net.rs", &old_text, &new_text);
// 或者来自统一 diff 文本（`git diff`、`diff -u` 或只有 `@@` hunk）：
let diff = Diff::parse(&unified_text);

let state = cx.new(|cx| DiffViewState::new(diff, cx));

DiffView::new(&state).h(px(360.))
```

`Diff::between` 计算逐行 diff（Myers 最短编辑脚本），并把更改分组为 hunk，每处更改前后保留三行未更改的内容。它知道整个文件，所以 hunk 之间未更改的行会折叠成一行，点击后在原位置展开；`unfold_all` 会展开全部。解析得到的 diff 只知道其 hunk 内的行，所以 hunk 之间只有标题。

两者都是对你自己数据的普通计算。输入很大时，请在后台线程构建 `Diff`，再交给 `DiffViewState::new` 或 `set_diff`。在 debug 构建中测得：一个 10,000 行、有 100 处分散修改的文件约 4 ms；两个毫不相关的 5,000 行文件约 28 ms，因为编辑超过 1,500 处后搜索会放弃，把不同的中间部分显示为先删除、后新增。

## 语法颜色

颜色来自当前的高亮主题，并在后台线程为 diff 的两侧分别计算：未更改和删除的行作为旧版本，未更改和新增的行作为新版本，这样跨行的字符串或注释在一个 hunk 内也能正确着色。主题改变时会重新计算。没有启用 `tree-sitter` feature，或者扩展名没有对应的语法时，各行显示为纯文本。

## 复制

每个文件的复制按钮会把该文件的更改复制为统一 diff 文本，文本只在点击时生成。`Diff::to_unified` 返回整个 diff。

## API 参考

| 项 | 说明 |
| --- | --- |
| `Diff::between(path, old, new)` | 文件两个版本之间的逐行 diff |
| `Diff::parse(text)` | 解析统一 diff 文本，可包含一个或多个文件 |
| `Diff::files()` | 文件列表；每个文件有 `path()`、`old_path()`、`new_path()`、`lines()`、`added_lines()`、`removed_lines()` |
| `Diff::added_lines()` / `removed_lines()` / `is_empty()` | 总计 |
| `Diff::to_unified()` / `file_unified(ix)` | 统一 diff 文本 |
| `DiffLine::change()` / `old_line()` / `new_line()` / `text()` | 一行：`LineChange::{Unchanged, Added, Removed}` 及其行号 |
| `DiffViewState::new(diff, cx)` / `set_diff(diff, cx)` | 从顶部开始、以折叠状态显示 diff |
| `DiffViewState::unfold_all(cx)` | 展开所有未更改的行 |
| `DiffView::new(&state)` | 视图；可通过 `Styled` 设置尺寸 |
