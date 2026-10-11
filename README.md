# vir

`vir` 是一个用 Rust 实现的现代、精简 vi 复刻。它保留 vi 的模态编辑、操作符与移动组合、寄存器、撤销和点重放，并提供一个可脚本化的 ex 前端。项目当前的目标是小而可靠的终端编辑器，不承诺覆盖全部历史 vi/ex。

当前精简功能范围已实现，后续维护集中于现有行为的正确性和资源边界。命令与兼容限制见本文及 `vir --help`。

## 快速开始

```sh
# 构建 release 二进制
cargo build --release --locked

# 在终端编辑文件
target/release/vir --vi notes.txt

# 以 ex 脚本方式编辑
printf '%s\n' '%p' 'q' | target/release/vir --ex notes.txt
```

`--vi` 需要终端标准输入和标准输出。`--ex` 可以交互使用，也可以从管道或重定向读取命令；管道模式不会显示提示符，命令本身不带前导冒号。

## 构建指南

### 环境要求

- Rust 1.88 或更新版本（Rust 2024 edition，最低版本见 Cargo.toml）。
- Cargo，与 Rust 工具链一同安装。
- UTF-8 locale 适合日常使用；visual 模式需要 Unix 终端提供 raw mode 和 PTY 行为。
- 运行 PTY 验收脚本需要 Python 3 标准库。普通构建和 Rust 测试不依赖 Python。

依赖由 Cargo 管理，直接依赖包括 `regex`、`crossterm`、`unicode-width` 和 `tempfile`。`Cargo.lock` 已提交，发布构建应使用 `--locked`，避免意外更新依赖版本。

### 获取源码并构建

```sh
git clone <repository-url> vir
cd vir

# debug 构建，适合开发和调试
cargo build --locked

# release 构建，生成 target/release/vir
cargo build --release --locked
```

第一次构建需要下载依赖。依赖已经位于本地 Cargo 缓存时，可以使用离线模式：

```sh
cargo build --offline --locked
cargo build --release --offline --locked
```

也可以直接运行而不手动指定二进制路径：

```sh
cargo run -- --help
cargo run --release -- --vi notes.txt
cargo run -- --ex notes.txt
```

需要安装到 Cargo 的用户二进制目录时：

```sh
cargo install --path . --locked
vir --help
```

### 测试与质量检查

提交前建议执行完整检查：

```sh
cargo test --offline --locked
cargo test --release --offline --locked
cargo fmt --check
cargo clippy --all-targets --all-features --offline --locked -- -D warnings
cargo build --release --offline --locked
```

visual 终端行为需要真实 PTY，不能只依赖普通 Rust 测试中的非 PTY 测试。先构建 debug 二进制，再在 Unix 环境执行：

```sh
cargo build --offline --locked
python3 tests/macro_pty.py
python3 tests/visual_pty.py
python3 tests/paste_pty.py
python3 tests/recovery_pty.py
python3 tests/input_corrections_pty.py
python3 tests/put_budget_pty.py
```

PTY 脚本使用临时目录和临时文件；它们会覆盖宏、终端恢复、缩放、粘贴、文件保存冲突和崩溃恢复等路径。Windows 等不提供相同 Unix PTY 接口的环境可以运行 Rust 测试，但不能据此声称 PTY 行为已经验证。

## 命令行入口

```text
vir --vi [file]
vir --ex [file ...]
vir --recover-list [directory]
vir --vi --recover SNAPSHOT
vir --ex --recover SNAPSHOT
vir --help
```

- `--vi [file]` 打开 visual 终端编辑器。最多打开一个文件；省略文件时使用未命名缓冲区。
- `--ex [file ...]` 启动 ex 前端，可以打开多个启动文件并使用 `args`、`n`/`n!` 在文件列表中切换。
- `--recover-list [directory]` 列出恢复快照。省略目录时优先使用 `VIR_RECOVERY_DIR`，否则使用当前目录。
- `--vi --recover SNAPSHOT` 或 `--ex --recover SNAPSHOT` 将快照恢复到未命名、已修改的缓冲区。恢复不会自动覆盖原文件，通常应使用 `w NEW_PATH` 保存到新路径。
- `--help`/`-h` 打印与源码一起发布的运行时帮助。

旧的两参数入口 `vir <unused-option> <filename>` 仍保留，但它只打印文件内容，不进入编辑器。新的脚本和用户调用应使用 `--ex` 或 `--vi`。

## ex 使用说明

ex 命令从标准输入读取，每行一个命令；交互终端会显示提示符，但命令仍不需要输入 `:`。发生错误时输出 `? ...` 并继续处理后续命令，脚本错误不保证反映为非零进程退出状态。

### 地址和范围

支持以下地址形式：

- 行号、`.` 当前行、`$` 最后一行、`%` 全文件；
- 小写文件内标记的行地址，例如 `'a`；
- `/pattern/` 向前搜索、`?pattern?` 向后搜索；
- `+`/`-` 偏移，以及用逗号连接的起止地址。

没有范围或只有一个地址时，各命令使用自身的默认范围。裸地址会打印对应范围；空命令打印下一行。地址 `0` 可用于在第一行之前插入或 put。

搜索和替换使用 Rust regex 语法，不是历史 vi BRE。`//` 和 `??` 重用上一次模式。文件路径按字面解析，允许空格，不做 shell 展开或 glob 展开。

### 支持的 ex 命令

| 命令 | 作用 |
| --- | --- |
| `[range]p` / `nu` | 打印行，或带行号打印 |
| `[address]a` / `i` | 在地址之后追加，或之前插入多行文本；单独的 `.` 结束输入 |
| `[range]c` / `d` | 修改或删除行；空 `c` 删除所选行 |
| `[range]m address` | 将行移动到目标地址之后 |
| `[range]t address` / `co` / `copy` | 将行复制到目标地址之后 |
| `[range]j` / `join` | 合并行，使用 visual `J` 的空白规则 |
| `[address]r [path]` | 读取 UTF-8/LF 文件并插入；省略路径使用当前文件名 |
| `[range]y [reg]` | 抽取行到未命名或指定寄存器 |
| `[address]put [reg]` | 放置未命名、命名或编号寄存器 |
| `u` | 切换最近一次编辑；保存不会清除撤销 |
| `[range]w [path]` | 写入完整缓冲区，或将范围导出到路径 |
| `[range]wq [path]` | 写入后按未保存状态和启动文件列表检查退出 |
| `e[!] [path]` | 打开其他文件或重新加载；`e!` 丢弃编辑 |
| `f [path]` | 显示状态，或设置当前文件名 |
| `args` | 显示启动文件列表 |
| `n[!]` | 编辑下一个启动文件，`n!` 丢弃编辑 |
| `q` / `q!` | 检查后退出，或丢弃修改强制退出 |
| `[range]s/pattern/replacement/[g]` | 每行替换第一个匹配，`g` 替换该行全部匹配 |
| `set ai` / `set noai` | 开启或关闭新行自动缩进 |

替换文本支持 `&`、`\1` 到 `\9`、`\&`、`\\` 和 `\/`；`$1` 按字面处理。只支持 `/` 分隔符和 `g` 标志。替换、join、read、move、copy 和 ex 编辑都可以撤销。

一个完整的非交互示例：

```sh
printf '%s\n' \
  'a' \
  'hello world' \
  '.' \
  '%s/world/vi/g' \
  'wq' \
| target/release/vir --ex notes.txt
```

这会创建或打开 `notes.txt`，追加一行，替换整文件中的 `world`，保存并退出。`EOF` 会退出但不会隐式保存；未终止的最后一行输入会被丢弃，已完成的输入行会保留在恢复快照中。

## visual 使用说明

启动：

```sh
target/release/vir --vi notes.txt
```

### 移动与搜索

- `h`/`j`/`k`/`l` 和方向键移动；`0`/`$` 或 Home/End 到行首/行尾。
- `g`/`G` 到首行/末行；带数字的 `5G`（或 `5g`）到指定行。
- PageUp/PageDown、Ctrl-B/Ctrl-F 滚动和移动。
- `w`/`b`/`e` 与 `W`/`B`/`E` 进行小词、大词移动；支持计数和 Unicode 标量语义。
- `f{char}`/`F{char}` 查找当前行下一个/上一个字符；`t{char}`/`T{char}` 停在目标前/后；`;` 重复同方向查找，`,` 反向重复。
- `/pattern`、`?pattern` 进入 visual 搜索；Enter 提交，空模式复用上次模式，`n`/`N` 顺向/反向重复。搜索会回绕，不提供高亮。
- `%` 在光标处或之后寻找当前行首个 ASCII `()[]{}` 括号并跳到匹配括号，可跨行；字符串和注释不会被解析。

移动从不修改文本、撤销记录或寄存器。复杂 emoji 和组合字符按 Unicode 标量而非 grapheme cluster 处理。

### 操作符、编辑和计数

- `d` 删除、`c` 修改、`y` 抽取；可与 `h/j/k/l`、`0/$`、`G`、词移动、字符查找、`%` 和标记移动组合。
- 支持操作符前后计数，例如 `2d3w` 作用于六个词，`2d3l` 删除六个字符；整行操作为 `dd`、`cc`、`yy`。
- `D` 删除到行尾并忽略计数；`C` 删除到计数范围末尾后进入输入；`Y` 抽取整行。
- `x` 从光标处删除，`X` 向光标左侧删除；`s` 替换当前行字符，`S` 修改整行。
- `J` 合并行；支持计数并保持文件末尾换行状态。
- `r{char}` 替换指定数量字符；`R` 进入 Replace 模式，逐个覆盖并在行尾追加，Enter 分行，Backspace 可恢复覆盖、分行或追加。
- `i`/`a`/`I`/`A` 进入输入；`o`/`O` 在下方/上方打开新行。一次输入会话作为一次撤销事务。
- `[count]i/a/I/A` 只重复输入会话的首段净文本；首个换行消耗计数，后续输入行只插入一次。`[count]o/O` 接受计数但仍只打开一次；`[count]R` 覆盖一次首段后插入额外副本。
- `p`/`P` 在光标后/前放置字符寄存器，或在当前行后/前放置整行寄存器；计数重复放置但只产生一次撤销。
- `u` 切换最近编辑；`U` 恢复本次停留期间当前行的初始内容。两者都保留寄存器和点重放规则。
- `.` 重放最近一次成功的 visual 文本编辑；移动、搜索、抽取、撤销、保存和 ex 命令不会覆盖点配方。
- `:` 进入 visual 的 ex 命令行，可使用 `w`、`q`、`q!`、`wq`、`e[!]`、`f`、`d`、`join`、`read [path]`、`move address`、`copy address`、`yank`、`put`、`u`、`set ai/noai` 和替换命令。其他 ex 命令目前不从 visual 冒号入口提供。
- 普通模式 `q` 或 Ctrl-C 在退出前检查未保存修改；保存、文件切换和 ex 命令不会覆盖最近的 visual 点配方。

失败或取消的编辑保持文本、寄存器、光标和撤销记录不变。`Esc`/Ctrl-C 可取消待处理的计数或操作符；已进入输入会话时它们提交当前会话。

### 输入、缩进和粘贴

- Insert/change/R 中 Enter、Tab、Backspace/Delete 可编辑文本；Insert/change 中 Ctrl-W 删除上一个输入词，Ctrl-U 清除到生成缩进或行首。
- `Ctrl-V` 或 `Ctrl-Q` 引用下一个字符，可将 Esc、Ctrl-C 和换行作为文本输入；引用 NUL 会被拒绝。
- `set ai` 开启自动缩进，复制当前行前导空格或 Tab。Ctrl-T/ Ctrl-D 按固定 8 列 tabstop 增减生成缩进；Ctrl-T 即使 `noai` 也可调整手工前导空白。Replace 模式不启用这些缩进控制。计数插入中，首个换行前被 Ctrl-T/D 重写的整段前导空白作为净输入重复；首个换行后自动生成的缩进只插入一次。
- 支持终端 bracketed paste。普通模式下粘贴作为一次插入，Insert/change/R 中粘贴进入当前事务；粘贴内容按字面处理，CRLF/CR 规范化为 LF，单次事件上限 64 KiB。粘贴不会触发自动缩进。
- terminal resize 会保留编辑状态；长行水平滚动，最后一列保留给终端换行保护。

### 寄存器、宏和标记

- 未命名寄存器、命名寄存器 `"a` 到 `"z`、编号删除寄存器和编号 put 都可用。大写命名寄存器追加；字符/行形态混合写入时由最后一次写入决定形态。
- `d`/`c`/`x`/`X`/`s`/`S`/`D`/`C` 会发布寄存器；yank、put、undo 和文件切换保留寄存器。系统剪贴板不参与。
- `@a` 执行字母寄存器中的按键，`@@` 重复上一次宏。宏可以嵌套、包含计数、控制字符、Insert/Replace/change 和 ex 命令；多个编辑共享一个撤销组。
- 宏在输入模式中耗尽时，物理键盘可以继续完成同一输入事务；这是当前明确的现代化简化，不模拟历史 vi 的逐行拆分。每次物理调用有展开和待处理按键上限，递归宏会停止并报错。
- `m[a-z]` 设置小写文件内标记；单引号形式跳到行首非空白，反引号形式跳到精确 Unicode 标量位置。标记也可用于 ex 地址和 d/c/y 操作符。
- 插入、移动、复制、删除、替换、join 和 undo 会按当前契约迁移或恢复标记；加载文件清除标记，保存不清除。没有跨文件标记、历史跳转栈、持久化标记或大写标记。

## 文件、编码和恢复

- 文件按 UTF-8/LF 读取；不接受 CR 字节。原文件是否有末尾 LF 会被保留，插入外部文件时按行处理并在需要时补 LF。
- 保存会先写入并同步临时文件，再发布到目标。文件冲突、符号链接、目录替换、只读或不支持的权限会被拒绝，编辑器状态和外部文件保持可恢复。
- 完整写入清除 modified 标志；范围导出不会替换当前文件，导出到其他路径不会自动改变当前文件名。`w!` 不提供，已有目标不会被强制覆盖。
- 加载时记录文件版本；检测到文件在加载或上次保存后被修改时，保存会失败。可用 `e!` 丢弃当前编辑重新加载，或用 `w NEW_PATH` 导出到新文件。最终检查与发布之间仍存在常规竞态窗口。
- CLI 会为未保存编辑自动维护 `.vir-recovery-*` 快照。快照默认位于初始文件旁边；未命名缓冲区或缺少父目录时使用当前目录。设置 `VIR_RECOVERY_DIR` 可指定一个已有恢复目录。

恢复示例：

```sh
mkdir -p "$PWD/.vir-recovery"
VIR_RECOVERY_DIR="$PWD/.vir-recovery" target/release/vir --vi notes.txt

# 进程异常结束后查找快照
target/release/vir --recover-list "$PWD/.vir-recovery"

# 恢复到未命名缓冲区，再写入新文件
target/release/vir --ex --recover "$PWD/.vir-recovery/.vir-recovery-..."
# 在 ex 会话中输入：w recovered.txt，然后 q
```

成功保存、重新加载和正常退出会清理当前会话快照；`q!` 和 ex EOF 会有意丢弃未保存恢复数据。快照只包含最近一次成功检查点，不恢复 undo、寄存器或光标，也不会自动合并或覆盖原文件；恢复源快照不会被消费，应在确认内容后手动删除。单个快照上限为 128 MiB，恢复机制不等同于完整断电保护。

## 事务、撤销和资源边界

编辑会话通常以一个可逆事务提交：输入、change、Replace、paste、宏组、join、move/copy、ex 行编辑和 put 都有明确的单次撤销边界。失败的地址、搜索、保存、粘贴、计数或寄存器操作会尽量保持文本和已发布状态原子不变。

为防止失控输入，visual 寄存器 put 及其点重放限制展开总量为 1,000,000 UTF-8 字节，整行寄存器每行另计一个分隔字节（含空行）。计数输入和点重放有工作量上限，宏展开有深度/按键预算，恢复快照有大小上限，bracketed paste 单次事件限制为 64 KiB。这些限制超出时会在编辑前或当前事务边界报告错误，不应依赖它们处理任意大的批量数据。

## 明确不支持或不承诺的功能

以下项目不属于当前精简范围：

- `global`/`v`、完整 ex 宏、source、map、abbr、配置脚本和外部 shell/filter 命令；
- tags、跨文件历史导航、历史上下文跳转栈、完整多文件 ex 工作流和 next-with-new-list；
- 完整历史 vi BRE、替换 `&`/`~` 之外的全部历史替换语法、替换确认、多行替换和其他分隔符；
- ex `join!`、尾随 count、`w!`，以及把外部进程输出作为 `read` 输入；
- 系统剪贴板、录制宏命令、宏/标记会话持久化、跨文件或大写标记；
- 段落/句子/节移动，`z` 系列和额外滚动命令，`tabstop`/`shiftwidth`/LISP 等全部历史缩进选项；
- 历史终端的每个特殊按键语义、复杂 grapheme cluster 的语言级分词，以及跨平台 PTY 行为保证。

这份列表描述的是当前产品边界，不表示相关功能在历史 vi 中不存在。具体命令和有限行为契约以本文及运行时帮助为准。

## 项目文档

- [运行时帮助](src/help.txt)：与 `--help` 完全一致的紧凑命令参考。

## 许可证

仓库当前未提供单独的许可证文件。分发或集成前请根据项目实际授权补充并确认许可证。
