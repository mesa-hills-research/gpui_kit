---
title: 拼写检查与单词建议
description: gpui-kit-writing 为 Textarea 提供真正的词典，以及从用户输入中学习的单词建议。
order: -9.04
---

# 拼写检查与单词建议

[Textarea](../component/textarea) 会给 `SpellChecker` 报告的拼写错误加下划线，并显示 `SuggestionProvider` 提供的补全。何时询问、下划线、修正菜单和幽灵文本由 Textarea 负责；词典和建议内容由应用负责。`gpui-kit-writing` 同时提供这两者，聊天输入框、笔记编辑器这类写作界面无需自己实现：

- **拼写检查**：对照词典检查，词典包括内置的英语词表（SCOWL 70 级，约 17 万个单词）、任意其他词表，以及用户添加的单词。代码、标识符、路径、URL、全大写和驼峰式单词不会被标记；右键给出的更正按常见的打字错误排序，例如 “mispeled” 首先建议 “misspelled”。
- **单词建议**：补全正在输入的单词，并在空格后预测下一个单词。建议来自在用户电脑上、从用户亲手输入的内容中学到的词汇；粘贴的文本、撤销与重做以及程序插入的文本都不会被学习。只有在建议更可能正确时才会显示，用户反复略过的单词也会越来越少出现。单词之间的标点（如逗号、冒号或斜杠）会截断预测所依据的上文：输入“UI/UX, ”之后，不会再给出在“UI/UX”之后学到的单词。

启用 `gpui-kit` 的 `writing` feature：

```toml
gpui-kit = { version = "...", features = ["writing"] }
```

## 设置

`Writing` 保存所有 Textarea 共享的内容：词典、词汇，以及建议出现的积极程度。在应用启动时加载一次；每个 Textarea 各自拥有检查器和建议：

```rust
use gpui_kit::component::input::{SuggestionOptions, TextEditor, TextareaState};
use gpui_kit::writing::{Writing, WritingOptions, accept_word_bindings};

// 启动时，在 gpui_kit::init 之后。
let writing = Writing::load(
    WritingOptions::new().with_data_dir(config_dir.join("my-app")),
    cx,
);
cx.bind_keys(accept_word_bindings());

// 每个 Textarea。
let suggestions = writing.word_suggestions();
let document = cx.new(|cx| {
    TextareaState::new(window, cx)
        .spell_checker(writing.spell_checker())
        .suggestion_provider(suggestions.clone())
        .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
});
// 保留这个订阅：Tab 和 Escape 会影响后续建议的节奏。
let pacing = suggestions.follow(&document, cx);
```

Tab 接受整条建议。`accept_word_bindings` 把 Ctrl+Right（macOS 上为 Cmd+Right）绑定为只接受建议中的下一个单词；没有建议时，这个键照常移动光标。

如果 Textarea 打开时就带有并非用户输入的文本（例如从磁盘读取的文档），请调用 `suggestions.opened_with(&text)` 告知建议，这样这些文本永远不会被学习。

## 文件

`WritingOptions` 指定文件的位置。没有数据目录时，不读取也不保存任何文件，学到的内容只在本次运行中有效。

| 选项 | 默认值 | 内容 |
| --- | --- | --- |
| `with_data_dir(path)` | 无 | 存放下面两个文件的目录 |
| `with_vocabulary_file(name)` | `vocabulary.json` | 单词建议学到的内容，旁边有一个 `vocabulary.lock` |
| `with_user_dictionary_file(name)` | `user-dictionary.txt` | **添加到词典** 添加的单词，每行一个 |
| `with_word_lists(lists)` | `[WordList::English]` | 词典接受的词表 |

`WordList` 可以是 `English`（`english` feature，默认开启）、加载时读取的 `File(path)`，或 `Text`。词表每行一个单词，`#` 开头的行是注释；Hunspell 词缀标记（`word/MS`）会被去掉，但不会展开。单词总数少于 10,000 时，词典接受所有单词，以免把所有内容都标为错误。

词汇在应用决定的时机保存：

```rust
// 保存文档、关闭窗口时，或定时保存。
writing.save_in_background(cx).detach();

// 退出时，此时保存必须完成。
writing.save();
```

多个进程可以共享同一个词汇文件。每次保存都会在加锁后，把本进程自上次保存以来学到的内容加到文件当前的内容上，并完整地重写文件；因此崩溃只会留下旧文件或新文件之一。无法解析的文件会被移到一旁，而不会被覆盖。

## 建议出现的积极程度

`writing.set_suggestion_mode(mode)` 从下一次按键起切换所有 Textarea：

| `SuggestionMode` | 行为 |
| --- | --- |
| `Off` | 不提供建议，新输入的内容也不会被学习。 |
| `Quiet`（默认） | 输入停顿 300 毫秒后出现。每略过一条建议，等待时间增加 150 毫秒，最长 1 秒；被略过的单词在十分钟内不会在同样的开头之后再次出现。用户顺着建议继续输入时，建议保持显示。 |
| `Eager` | 每次按键后出现。 |

`SuggestionMode` 序列化为 `"off"`、`"quiet"` 或 `"eager"`，并把 `true` 和 `false` 读作 Quiet 和 Off。

## 限制

- 仅支持英语：检查器会跳过含有 ASCII 以外字母的单词，代码与散文的分类器统计的是英语虚词。
- 拼写检查和单词建议是 `TextareaState` 的功能。单行 `Input` 和代码 `Editor` 都没有。
