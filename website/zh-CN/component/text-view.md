---
title: TextView
description: 渲染 Markdown 与 HTML 文本，并支持自定义 Markdown 插件。
---

# TextView

`TextView` 用于在 GPUI 中渲染格式化文本。它支持 Markdown、简单 HTML、文本选择、代码块操作，以及通过 Markdown 插件解析和渲染项目自定义语法。

标准实现现在位于 `gpui-base`；本模块保留兼容重导出和组件主题适配。仅使用 Base 时的设置、完整默认样式及可选语法高亮请参阅 [GPUI Base TextView](../base/text-view.md)。

`TextView::selectable(true)` 使用 `gpui-base` 提供的窗口级文本选择引擎。如果要让普通文本或自定义 renderer 参与同一选择，请参阅 [GPUI Base Text Selection](../base/text-selection.md)。

## 导入

```rust
use gpui_kit::component::text::{markdown, TextView};
```

## 用法

### Markdown

只需要渲染 Markdown 时，可以使用 `markdown` helper：

```rust
use gpui_kit::component::text::markdown;

markdown("# Hello\n\nThis is **Markdown**.")
    .selectable(true)
    .scrollable(true)
```

如果需要稳定 id，也可以直接构造 `TextView`：

```rust
use gpui_kit::component::text::TextView;

TextView::markdown("preview", markdown_source)
    .selectable(true)
```

### HTML

```rust
TextView::html("html-preview", "<strong>Hello</strong>")
```

### 放在有底色的容器里

没有显式设置 `.style()` 的 `TextView` 会跟随容器的文字颜色。如果底色与页面明暗相反，例如 `Filled` 样式的 [Bubble](bubble.md)，链接、次要文字、代码和表格背景、边框、选择色也会从这个颜色推算，并且不使用语法高亮，所以内容不需要额外设置样式：

```rust
Bubble::new().child(TextView::markdown("reply", text))
```

### 流式文字淡入

聊天回复是分块到达的。`stream_fade(true)` 会让每一块新文字在落点处淡入，而不是直接蹦出来，观感与 Claude 展示回复的方式一致：

```rust
TextView::new(&self.reply).stream_fade(true)
```

淡入以渲染后的文字为准：`push_str`，或者 `set_text` 传入以当前文本为前缀的更长文本，新增的部分从透明渐变到正常颜色，用时 350ms、ease-out 曲线，这是从 Claude 实测得到的节奏：比模型每块 50–300ms 的到达间隔更长，于是相邻几块的淡入互相重叠，尾部呈现为一段渐变，而不是最新一块突然变实。代码块里的代码和表格单元格里的文字同样参与。流式过程中被补齐的 Markdown 标记（`**bo` 变成粗体 `bold`）只让发生变化的字形重新淡入，不会整段闪烁。替换当前内容的文本直接显示；系统开启减少动态效果时也直接显示。不开启就没有任何动画。

需要自定义时长、缓动，或者让每块按词逐个浮现时，通过 `.motion(...)` 传入 `TextViewMotion`，详见 [GPUI Base TextView](../base/text-view.md#保留状态与动态更新)。

### 流式输出时的代码高亮

启用 `tree-sitter` feature 后，代码块按围栏标注的语言高亮。每种语言都有对应的 Cargo feature，例如 `tree-sitter-rust`，`tree-sitter-languages` 会启用全部语言，即 Astro、Bash、C、C#、C++、CMake、CSS、Dart、Diff、EJS、Elixir、Elm、ERB、Erlang、F#、GLSL、Go、GraphQL、Haskell、HCL、HTML、Java、JavaScript、JSON、Julia、Kotlin、Lua、Make、Markdown、Objective-C、OCaml、Perl、PHP、PowerShell、Protocol Buffers、Python、R、Ruby、Rust、Scala、SQL、Svelte、Swift、TOML、TSX、TypeScript、XML、YAML 和 Zig。围栏可以用常见的写法标注语言，例如用 `shell`、`zsh` 或 `console` 表示 Bash，`jsx`，用 `patch` 表示 diff，用 `ps1` 表示 PowerShell，用 `tf` 表示 HCL，或者 `yml`。多种语言共用的简称归给更常见的那种，所以 `pl` 是 Perl，`ps` 是 PowerShell，`h` 是 C，`fs` 是 F#，`console` 会把 shell 会话（包括其中的输出）按 Bash 高亮。

代码块在后台线程中高亮。在下一次高亮完成之前，代码块显示已有的高亮，之后到达的代码以纯文本显示，因此无论语言和大小，每帧花在代码块高亮上的时间约为 20 µs。高亮器会保留每个代码块的解析结果和高亮，并能认出带着更多代码重新出现的同一个代码块。流式输出的代码块只在变化处重新解析，只从第一行可能变化的位置重新高亮，工作期间到达的多段内容会合并处理。流式输出的代码块在围栏闭合后会再从头完整高亮一次。用 `code_block_max_lines` 只显示部分行的代码块，只高亮到显示的行为止。高亮以高亮名称保存，绘制时才换成主题颜色，因此切换主题只会重新着色，不会重新解析。

某种语言在第一次被代码块用到时才会准备。准备时在后台线程解码语法表并编译查询，视语法不同需要约一毫秒到约一秒（Swift 和 F# 最慢），在此之前它的代码块显示为纯文本。如果应用知道会显示哪些语言，可以在启动时提前准备：

```rust
use gpui_kit::component::highlighter::LanguageRegistry;

LanguageRegistry::singleton().warm_up(["rust", "typescript", "python", "bash"], cx);
```

准备好的语法会常驻内存，从不到 1 MB 到 F# 的约 27 MB 不等，因此只预热应用可能显示的语言，而不是启用的全部语言。

### 把长文档显示为多行

TextView 每一帧都会布局整篇文档。聊天记录里的长回复可以拆成 `MessageScroller` 的多行显示，每个顶层块或一组块占一行，列表只布局屏幕上可见的行，开销小得多。用一个 `TextViewState` 作为文档解析整篇回复，每一行用 `TextViewState::block_range` 显示其中的部分块：

```rust
use gpui_kit::component::text::{TextView, TextViewState};

let answer = cx.new(|cx| TextViewState::markdown("", cx));
// 流式文字推给文档。
answer.update(cx, |answer, cx| answer.push_str(chunk, cx));

// 回复每多出一个块就加一行，这里一行一个块。
let blocks = answer.read(cx).blocks();
let row = cx.new(|cx| TextViewState::block_range(&answer, ix..ix + 1, cx));

// 在 scroller 的行渲染函数里：
TextView::new(&row)
```

文档每完成一次解析，视图就从文档取回自己的块，块全部完成的视图此后不再变化。末端开放的范围（例如 `ix..`）会包含之后追加的块。`blocks()` 会为文档的每个顶层块给出它是什么（段落、标题、列表、代码块及其语言、表格……）、在源文本中的范围、追加文字后是否还会变化，以及在只追加文字期间保持不变的 id。按块的类型把块分组成行，文本被替换时用 id 判断哪些行可以保留。如果设置的文字只是首尾空白不同（例如回复结束后改为修剪过的文字），就不算替换，id、各行和其中的选区都保持不变。`block_count()` 不用构建列表就能判断文档自上次查看后是否多了块，`blocks_from(ix)` 只列出从 `ix` 起的块。在 `TextViewState::finish` 把回复标记为结束、并且此前推入的文字解析完成之前，最后一个块一直算作未完成。此后，它所在的行也和其他行一样不再变化。选区可以跨行，就像跨多个独立的 TextView 一样，每个视图选择自己的块。复制时，同一文档的各行像同一个视图里的块那样拼接，彼此隔一个空行，与前后的其他文字之间也隔一个空行。同一文档各行上的选区在这些行滚出视野后依然保留。落在已滚走行中的选区端点保持不变，这些行会和其余部分一起复制，滚回视野时仍显示为选中。获得焦点的行滚出视野后仍保留自己的按键，因此 Cmd-C 或 Ctrl-C 依然复制整个选区，Cmd-A 或 Ctrl-A 全选该行，Shift-F10 和 Menu 键打开选区的上下文菜单。做选择期间列表从未布局过的行不属于选区。整篇回复的复制按钮可以改为复制文档的文本。文档本身和它的块视图只显示其一，因为它们共用解析出的块。

独立的视图也可以共用一个文档，例如对话中的各条消息，每条消息都是一个单独的 TextView。`document_position` 把视图放进应用指定的文档，此后跨这些视图的选区会像跨行时一样保留。

```rust
// 位置按阅读顺序递增。文档可以是窗口中其他文档都没有使用的任意 id。
TextView::new(&message).document_position(conversation.entity_id().as_u64(), ix as u64)
```

`SelectableText::document_position` 对用应用保留的句柄创建的纯文本 run 起同样的作用。视图设置的位置会取代 `block_range` 行自己的位置，所以一条消息拆成多行显示时，要让每一行的位置排在前面各条消息的行之后。

解析由文档完成，所以 Markdown 扩展和插件要设在文档上，而不是各行的 `TextView` 上，各行按文档的扩展渲染：

```rust
let extensions = MarkdownExtensions::default().plugin(FrontmatterPlugin);
let answer = cx.new(|cx| TextViewState::markdown("", cx).markdown_extensions(extensions));
```

在一篇两万 token 的回复末尾流式输出时，一块一行每帧需要 0.9 ms，而整篇作为一行需要 12.5 ms（`cargo bench -p gpui-component --features tree-sitter-rust --bench transcript_stream`）。

`code_block_max_lines` 只显示每个代码块的前几行，适合需要读者手动展开的代码块。超出的行既不布局也不高亮，所以长代码块的开销只取决于显示的行数。`CodeBlock::code` 和块在源文本中的范围仍然包含全部内容，可用于复制按钮或统计行数：

```rust
TextView::new(&row).when(!expanded, |view| view.code_block_max_lines(40))
```

读者无法选中被隐藏的行，所以选区一旦越过最后一行显示的内容，复制的就是整个代码块。比上限长的代码块都会被截断，哪怕只隐藏一行。只比上限多几行的代码块，可以不设上限。范围高亮和定位在隐藏的行里不会显示，所以要定位到那里的查找结果，先展开代码块。结果在块内的行号，就是块在 `rendered_text()` 中的文字里、结果之前的换行数。

流式淡入会在每块文字到达后的 350 ms 内以每秒约 30 次重绘。回复拆成多行后，这些帧只布局屏幕上的行，上面这篇回复每帧大约一毫秒，而且只有文字变化的行会淡入。

### 高亮文本范围

应用在文档里搜索，或者指向文档里的某处引用时，用 `set_range_highlights` 把这些范围标出来。搜索由应用负责：在视图显示的文本 `rendered_text()` 里找到范围，连同绘制用的颜色一起交回，当前结果用更醒目的颜色：

```rust
use gpui_kit::component::{
    ActiveTheme as _,
    text::{RangeHighlight, RangeHighlightError, TextViewState},
};

fn highlight_matches(
    state: &mut TextViewState,
    query: &str,
    current_match: usize,
    cx: &mut Context<TextViewState>,
) -> Result<(), RangeHighlightError> {
    let (color, current_color) = (cx.theme().warning.opacity(0.3), cx.theme().warning);
    let text = state.rendered_text();
    let matches = if query.is_empty() {
        Vec::new()
    } else {
        text.as_str().match_indices(query).collect()
    };
    let highlights = matches.into_iter().enumerate().map(|(ix, (start, found))| {
        RangeHighlight::new(
            start..start + found.len(),
            if ix == current_match { current_color } else { color },
        )
    });
    state.set_range_highlights(highlights, cx)
}
```

`rendered_text()` 与纯文本复制得到的文字一致。`hello **world**` 读作 `hello world`，转义字符已经还原，标题和列表的标记不在其中，各块之间隔一个空行。偏移量是 UTF-8 字节偏移，`str` 搜索返回的范围可以直接传入；重复出现的词组按各自的位置区分。文本在第一次读取时才会生成。

高亮绘制在文字背后、选区之下，换行、对齐、语法颜色、链接、选择和复制都保持不变。多个高亮重叠时，后面的覆盖前面的。跨越两个块的范围在两个块里分别绘制。不属于任何块的文字不会绘制：块之间的换行、表格单元格之间的空格、自定义块、HTML 块和 inline plugin 对象。只有起点在终点之后、超出范围或不在字符边界上的范围会被拒绝，同一批的整组高亮也一并拒绝。

内容变化时，高亮会跟随它所在的块，保留到这个块里文字开始变化的位置为止。流式追加的文字（无论通过 `push_str` 还是 `set_text`）不影响前面的高亮；修改某处时，修改前后的高亮都会保留。表格单元格只按位置区分，因此修改表格内部时，被修改的那一行及其后各行单元格的高亮都会失效。视图会发出通知：观察这个 state，重新在新的 `rendered_text()` 里搜索即可。请在同一次 state 更新中计算范围并调用 `set_range_highlights`，确保范围对应当前文本。属于文字本身的背景（例如 `<mark>` 和语法高亮）会覆盖在范围高亮之上（行内代码的背景在高亮之下），高亮也不会随流式文字一起淡入。HTML 视图不支持范围高亮。

如果应用保存的是 Markdown 源码里的范围，例如用户评论某段文字时 `selected_source_range()` 返回的范围，就在同一个 `rendered_text()` 上用 `range_for_source` 逐个转换：

```rust
use std::ops::Range;

use gpui_kit::component::{
    ActiveTheme as _,
    text::{RangeHighlight, RangeHighlightError, TextViewState},
};

fn highlight_comments(
    state: &mut TextViewState,
    markdown: &str,
    comments: &[Range<usize>],
    cx: &mut Context<TextViewState>,
) -> Result<(), RangeHighlightError> {
    let text = state.rendered_text();
    // 范围对应的是 `markdown`。它解析完成之前，视图显示的仍是之前的源码；
    // 等视图发出通知后再转换一次。
    if text.source() != markdown {
        return Ok(());
    }
    let color = cx.theme().info.opacity(0.3);
    let highlights = comments
        .iter()
        .filter_map(|comment| text.range_for_source(comment.clone()))
        .map(|range| RangeHighlight::new(range, color));
    state.set_range_highlights(highlights, cx)
}
```

`range_for_source` 返回能包含该源码范围所渲染出的全部字符的最小范围。不渲染任何文字的源码（强调符号、标题和列表标记、代码围栏、表格竖线、链接地址）不影响结果，因此 `**bold**` 和 `bold` 转换出的范围相同。只要字符的源码有一部分落在范围内，这个字符就会被包含：`&amp;` 的一部分对应 `&`，inline plugin 对象源码的一部分对应该对象的全部文字。把选区报告的源码范围转换回来，得到的就是原来选中的范围；只是块之间的换行不对应任何源码，落在选区两端时不会包含在内：全选转换回来的范围不含最后一个块之后的换行。

`rendered_text().source()` 是最近一次完成解析的源码。用 `push_str` 追加文字时，它是交给视图的文本的前缀；用 `set_text` 替换内容后，在新文本解析完成之前，它可能仍是之前那段不相干的文本。因此转换前要像示例那样，先确认它就是范围所对应的源码。范围为空、超出范围或不在字符边界上，或者没有渲染出任何文字时返回 `None`；HTML 视图总是返回 `None`。

### 滚动到范围

`reveal_range` 滚动到同一份文本里的某个范围，比如用户跳到下一个结果时的当前结果：

```rust
state.reveal_range(current_range, cx)?;
```

它把范围起点所在的那一行滚动到可见区域内，长段落中间的某一行也能定位到；这一行（或整块显示的块）已经可见时，视图保持不动。空范围会显示它所在位置的那一行。`scrollable` 视图自己滚动。按内容高度排版的视图会让最近的外层 `gpui::list` 滚动，聊天记录就是这种情况，前提是承载视图的那一行已经完成布局，这一行可能不在屏幕上时，先滚动到这一行。其他滚动容器（例如设置了 `overflow_y_scroll` 的 `div`）通过 `on_reveal` 滚动，回调会收到这一行在窗口坐标中的位置：

```rust
let scroll = scroll_handle.clone();
TextView::new(&state).on_reveal(move |line, _, _| {
    let viewport = scroll.bounds();
    let mut offset = scroll.offset();
    if line.bottom() > viewport.bottom() {
        offset.y -= line.bottom() - viewport.bottom();
    } else if line.top() < viewport.top() {
        offset.y += viewport.top() - line.top();
    }
    scroll.set_offset(offset);
})
```

不覆盖任何块文字的范围（例如自定义块里的文字）会把整个块滚动到可滚动视图的可见区域内。只执行最后一次请求。它像高亮一样跟随内容变化；如果它所在的文字发生变化、视图用 `max_lines` 限制了行数，或者一秒内无法显示，请求就会被取消，因此不会在很久之后才突然滚动。表格里横向滚出的文字不会被滚动出来；整块显示且比视图更高的块从下方滚入时，显示的是它的末尾；放在应用列表里的可滚动视图只滚动自身；共用同一个 state 的多个视图共用同一个请求。

滚动请求只是尽力而为：返回 `Ok(())` 表示范围对当前文本有效、请求已被接受，并不表示视图已经滚动；请求被取消时也不会另行通知。

## 触摸选择

在触摸屏上，长按会选中手指下的单词，手指按住不放时选区跟随手指移动。抬起手指后，选区上方会出现包含 `复制` 和 `全选` 的编辑菜单，并在选区两端各显示一个拖动 handle。拖动 handle 会移动对应的一端，另一端保持不动；`全选` 选中被按下的那个视图，其 handle 仍可继续调整结果。

handle 和菜单由 [`Root`](./root.md) 为整个窗口选区绘制，因此跨多个视图的选区也能覆盖到。点击其他位置会清除它们；手指滚动内容时菜单会暂时让开。

## 上下文菜单

在可选择的文字上右键会打开包含 `复制` 和 `全选` 的菜单。`复制` 复制的内容与 Cmd-C 或 Ctrl-C 相同，也就是跨行、跨视图的整个选区，段落之间保留空行。没有选中文字时，`复制` 不可用。`全选` 选中菜单所在的视图。右键不会改变选区，无论落在哪里。在获得焦点的 TextView 中，Shift-F10 和 Menu 键也会打开这个菜单。

菜单使用应用的上下文菜单样式（见 [Input：上下文菜单](./input.md#上下文菜单)），因此调用 `ContextMenuStyle::set_app_default(ContextMenuStyle::Drawn, cx)` 之后，它在所有平台上都按主题颜色绘制。

用 `TextSelectionMenu::set_builder` 添加应用自己的菜单项。builder 会收到一个空菜单和一个 target，target 可以读取选中的文字。

```rust
use gpui_kit::component::text::TextSelectionMenu;

TextSelectionMenu::set_builder(
    |menu, target, _, _| {
        target
            .standard_items(menu)
            .separator()
            .menu_with_disabled("Search the Web", !target.has_selection(), Box::new(SearchWeb))
    },
    cx,
);
```

自带上下文菜单的元素（`ContextMenuExt`）、输入框，以及设置了 `on_link_click` 的链接会保留自己的右键操作，这个菜单不会在那里打开。

## 图片

Markdown 的 `![alt](src)` 和 HTML 的 `<img src>` 都通过 GPUI 的 `img` 元素渲染，
`src` 决定字节从哪里来：

- `http://`、`https://` URL 使用应用的 HTTP client 拉取。
- `data:` URL 就地解码，文档可以内嵌自己的图片（`data:image/png;base64,…`，
  或者百分号编码的 `data:image/svg+xml,…`）。GPUI 能解码的图片格式都可以；
  media type 不是图片的 `data:` URL 会交给默认加载器，像其他加载失败的图片一样报错。
- 其余的值——相对路径、`file://`、自定义 scheme——原样作为 URI 传下去。
  `TextView` 不会替文档读文件系统或 asset bundle。

要解析这些来源，或者改变任意图片的加载方式，把 `TextView` 包在一个安装了 GPUI
`ImageCache` 的元素里。它内部的每个 `img`（包括文档生成的）都会先向这个 cache
请求自己的 `Resource`，再回退到默认加载器：

```rust
use gpui_kit::{ImageCache, ImageCacheProvider};

div()
    .image_cache(app_image_cache.clone())
    .child(markdown("![diagram](app://diagrams/pipeline.svg)"))
```

`ImageCache::load` 拿到 `Resource::Uri` 后自行决定怎样得到 `RenderImage`，
加载策略归应用所有，文档本身仍是普通 Markdown。

## Markdown 插件

使用 `.plugin(...)` 支持自定义 Markdown 格式。插件同时拥有解析和渲染逻辑，调用方只需要把它挂到 `TextView` 上：

```rust
markdown(source)
    .plugin(TickerPlugin::new())
```

Markdown 插件实现 `MarkdownPlugin`：

```rust
use gpui_kit::{App, IntoElement, ParentElement as _, Window};
use gpui_kit::component::text::{
    markdown_ast, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
};

struct TickerNode {
    symbol: String,
}

struct TickerPlugin;

impl TickerPlugin {
    fn new() -> Self {
        Self
    }
}

impl MarkdownPlugin for TickerPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "ticker"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Paragraph(paragraph) = node else {
            return None;
        };
        let [markdown_ast::Node::Text(text)] = paragraph.children.as_slice() else {
            return None;
        };
        let symbol = text.value.strip_prefix('$')?;

        Some(
            MarkdownNode::new(
                "ticker",
                TickerNode {
                    symbol: symbol.to_string(),
                },
            )
            .text(format!("${symbol}"))
            .markdown(cx.node_source(node).unwrap_or(text.value.as_str())),
        )
    }

    fn render(
        &self,
        node: &MarkdownNode,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        let ticker = node.data::<TickerNode>().expect("ticker node data");

        gpui_kit::div().child(format!("${}", ticker.symbol))
    }
}
```

然后挂到 Markdown `TextView`：

```rust
markdown("$AAPL.US")
    .plugin(TickerPlugin::new())
```

## MarkdownNode

`MarkdownNode` 是 `parse` 和 `render` 之间传递的中性数据结构。

```rust
MarkdownNode::new("ticker", TickerNode { symbol })
    .text("$AAPL.US")
    .markdown("$AAPL.US")
```

- `name` 是稳定的节点名称，用于匹配 renderer。
- `data` 是 parser 产生的类型化数据，通过 `node.data::<T>()` 读取。
- `text` 是纯文本表示，用于选择和未注册 renderer 时的回退渲染。
- `markdown` 是 Markdown 表示，用于将文档重新序列化为 Markdown。

## Block plugin

Block plugin 在 `is_block()` 中返回 `true`，使用 block parser 和 renderer：

```rust
fn is_block(&self) -> bool {
    true
}
```

Inline plugin 保留默认的 `is_block() == false`，`render_inline` 返回 `Option<InlineElement>`。通过 `InlineElement::new(...)` 包裹任意 GPUI 元素，使用原生样式与事件，并按需指定基线。TextView 将整个元素作为原子对象测量和选择，支持纯文本与 Markdown 复制、文本降级和异步布局失效。契约与 `.plugin(...)` 注册示例详见[Inline plugin](../base/text-view.md#inline-plugin)。Component 层导出相同的 `InlineElement` 和 `InlineRenderContext` 类型。

## YAML Frontmatter

YAML frontmatter 不属于 CommonMark 或 GFM，因此默认不启用。启用 parser
construct 并挂载 `FrontmatterPlugin` 后，顶层 mapping 会渲染为
`DescriptionList`：

```rust
use gpui_component::text::{markdown, FrontmatterPlugin, MarkdownExtensions};

let extensions = MarkdownExtensions::default().frontmatter();

markdown("---\nname: example\ndescription: Example metadata.\n---")
    .markdown_extensions(extensions)
    .plugin(FrontmatterPlugin::new())
```

值以纯文本渲染。支持简单的无引号值，以及使用 `|-` 或 `>-` 的 block scalar；
literal scalar 会保留内容缩进。带引号的值、行尾注释、集合、别名、其他 block
header，以及包含额外缩进行的 folded scalar 会回退为 YAML code block，
保留原始内容，避免显示错误解析的值。

## 代码块操作

可以为 Markdown 代码块渲染操作控件：

```rust
markdown(source)
    .code_block_actions(|code_block, _window, _cx| {
        gpui_kit::div().child(format!("Run {}", code_block.lang().unwrap_or_default()))
    })
```
