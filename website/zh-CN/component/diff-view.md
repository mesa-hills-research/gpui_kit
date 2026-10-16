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

`Diff::between` 计算逐行 diff（Myers 最短编辑脚本），并把更改分组为 hunk，每处更改前后保留三行未更改的内容。它知道整个文件，所以 hunk 之间未更改的行会折叠成一行，点击后在原位置展开，`unfold_all` 会展开全部。只有一行的间隔直接显示那一行，而不是展开按钮。解析得到的 diff 只知道其 hunk 内的行，所以 hunk 之间只有标题。

两者都是对你自己数据的普通计算。输入很大时，请在后台线程构建 `Diff`，再交给 `DiffViewState::new` 或 `set_diff`。在 debug 构建中测得：一个 10,000 行、有 100 处分散修改的文件约 4 ms，两个毫不相关的 5,000 行文件约 28 ms。编辑超过 1,500 处后搜索会停止，改用在两个版本中都只出现一次的行作为锚点，锚点之间的部分再分别计算 diff。即使一个长文件每十行改一行，显示的也只是这些行。

## 尺寸

默认情况下视图会填满父元素，或使用你给的尺寸，并在内部滚动。放在各项高度不同的列表中时（例如对话记录中工具调用的编辑），用 `max_rows` 让它取各行的高度：

```rust
DiffView::new(&state).max_rows(24)
```

这样五行的编辑只占五行（加上文件行和 hunk 标题），较长的编辑占 24 行并可滚动。`DiffView::row_height(cx)` 返回一行的高度，便于自行设置尺寸。

## 键盘

视图是一个 Tab 停靠点，在其中点击也会让它获得焦点。获得焦点后，`↑` 和 `↓` 滚动一行，`←` 和 `→` 横向滚动，`Page Up` 和 `Page Down` 滚动一页，`Home` 和 `End` 跳到顶部和底部。

## 显示的内容

- **换行符。** 换行符在 `\n` 与 `\r\n` 之间改变的行算作更改。当一个文件的更改行同时有两种结尾时，每行在文本后显示 `CRLF` 或 `LF`。
- **文件末尾的换行。** 没有换行符的最后一行在发生更改的地方显示“文件末尾没有换行符”，复制的 diff 会保留 git 的 `\ No newline at end of file` 标记。
- **重命名、模式和二进制文件。** 根据 `git diff` 的头部，重命名的文件显示为 `旧 → 新`，模式更改显示 `模式 100644 → 100755`，二进制文件会注明，并且不显示它没有的行数。git 为含特殊字符的文件名加的引号会被去掉。
- **长行。** 超过 2,000 个字符的行（例如压缩后的代码）显示前 2,000 个字符以及剩余的字符数。

解析得到的 hunk 在其标题的计数用完时结束，但仍会收下之后新增或删除的行，因为手写的 diff 经常数错。因此 `git log -p` 和 `git format-patch` 的输出解析后，hunk 中不会混入提交说明或签名。

## 语法颜色

高亮名称（`string`、`comment` 等）只在后台线程为 diff 的两侧各查找一次：未更改和删除的行作为旧版本，未更改和新增的行作为新版本，这样跨行的字符串或注释在一个 hunk 内也能正确着色。每行在绘制时从当前高亮主题中查找颜色，所以切换主题只是重绘。没有启用 `tree-sitter` feature，或者扩展名没有对应的语法时，各行显示为纯文本。

## 复制

每个文件的复制按钮会把该文件的更改复制为统一 diff 文本，文本只在点击时生成。`Diff::to_unified` 返回整个 diff。

## API 参考

| 项 | 说明 |
| --- | --- |
| `Diff::between(path, old, new)` | 文件两个版本之间的逐行 diff |
| `Diff::parse(text)` | 解析统一 diff 文本，可包含一个或多个文件 |
| `Diff::files()` | 文件列表：每个文件有 `path()`、`old_path()`、`new_path()`、`lines()`、`added_lines()`、`removed_lines()` |
| `DiffFile::is_rename()` / `is_binary()` / `mode_change()` | git 头部对文件的说明 |
| `Diff::added_lines()` / `removed_lines()` / `is_empty()` | 总计 |
| `Diff::to_unified()` / `file_unified(ix)` | 统一 diff 文本 |
| `DiffLine::change()` / `old_line()` / `new_line()` / `text()` | 一行：`LineChange::{Unchanged, Added, Removed}` 及其行号 |
| `DiffLine::ending()` | 行的结尾：`LineEnding::{Lf, CrLf, Missing}` |
| `DiffViewState::new(diff, cx)` / `set_diff(diff, cx)` | 从顶部开始、以折叠状态显示 diff |
| `DiffViewState::unfold_all(cx)` | 展开所有未更改的行 |
| `DiffView::new(&state)` | 视图，可通过 `Styled` 设置尺寸 |
| `DiffView::max_rows(n)` | 取各行的高度，最多 `n` 行 |
| `DiffView::row_height(cx)` | 一行的高度 |
