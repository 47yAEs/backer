# Backer - 备份扫描器

Backer 是一个用 Rust 编写的高性能、多线程的网站备份文件扫描工具，能够帮助你检测目标网站可能存在的备份文件。

## 主要特性

- **高效扫描**：真正的多线程并发扫描，充分利用系统资源
- **智能识别**：通过多种方法检测备份文件，包括状态码、内容类型和文件头分析
- **灵活配置**：支持自定义备份文件模式、线程数量、超时控制等
- **丰富输出**：可选JSON、CSV或Markdown格式输出结果
- **伪装功能**：支持随机请求头和随机IP，避免被目标站点识别和封锁

## 安装方法

确保你已安装 [Rust 和 Cargo](https://www.rust-lang.org/tools/install)，然后运行：

```bash
# 从源代码编译安装
git clone https://github.com/yourusername/backer.git
cd backer
cargo build --release

# 编译后的可执行文件位于 target/release/backer
```

## 使用方法

### 基本用法

```bash
# 扫描目标网站列表
backer -t targets.txt -o results.json

# 使用10个线程扫描，并限制超时为5秒
backer -t targets.txt -o results.json -j 10 -T 5

# 使用自定义备份文件模式
backer -t targets.txt -p patterns.txt -o results.json

# 验证文件内容并输出为Markdown格式
backer -t targets.txt -v -f markdown -o results.md

# 使用随机请求头和随机IP进行扫描（默认已启用）
backer -t targets.txt --random-headers --random-ip -o results.json
```

### 命令行参数

```
选项：
  -t, --targets <FILE>           目标网站列表文件路径（每行一个URL）
  -p, --patterns <FILE>          自定义备份文件模式列表（每行一个模式）
  -j, --threads <N>              并发线程数量 [默认值: 10]
  -T, --timeout <SECONDS>        请求超时时间（秒） [默认值: 30]
  -r, --retry <N>                请求失败重试次数 [默认值: 3]
  -a, --user-agent <STRING>      自定义User-Agent
  -f, --format <FORMAT>          输出格式 [默认值: json] [可能值: json, csv, markdown]
  -o, --output <FILE>            结果输出文件路径
  -v, --verify                   验证文件内容（会下载文件头部）
  -d, --debug                    启用调试日志
      --random-headers           使用随机请求头（默认开启）
      --random-ip                使用随机IP (X-Forwarded-For)（默认开启）
      --no-random-headers        禁用随机请求头
      --no-random-ip             禁用随机IP
  -h, --help                     打印帮助信息
  -V, --version                  打印版本信息
```

## 输入文件格式

### 目标站点列表 (targets.txt)

每行一个URL：

```
https://example.com
http://test.com
domain.com    # 如果没有指定协议，将使用http://
```

### 自定义模式文件 (patterns.txt)

每行一个后缀或模式：

```
.zip
.tar.gz
.old
.bak2023
_backup.sql
```

## 隐蔽性特性

Backer默认使用以下隐蔽性技术，帮助你的扫描更加隐蔽：

1. **随机User-Agent**：每次请求随机从多种浏览器User-Agent列表中选择，模拟不同的客户端
2. **随机请求头**：添加随机的HTTP请求头，模拟真实浏览器行为
3. **随机IP地址**：通过X-Forwarded-For头部伪装来源IP地址
4. **智能延时**：请求失败时采用指数退避算法，避免频繁请求

这些特性可以通过命令行选项禁用（如`--no-random-headers`），但在大多数情况下建议保持启用。你也可以使用`-a`或`--user-agent`选项指定自定义的User-Agent。

## 高级使用示例

```bash
# 使用20个线程和自定义User-Agent
backer -t targets.txt -threads 20 -a "Mozilla/5.0 (compatible; MyScanner/1.0)" -o results.json

# 扫描大量目标，并启用调试日志
backer -t large-targets.txt -o results.json -d

# 以CSV格式输出并验证文件内容
backer -t targets.txt -v -f csv -o results.csv

# 禁用随机IP功能（在某些情况下可能需要）
backer -t targets.txt --no-random-ip -o results.json
```

## 注意事项

- 请确保你有权对目标站点进行扫描
- 过高的并发数可能会对目标服务器造成压力
- 一些服务器可能会封锁频繁的扫描请求
- 尽管工具有隐蔽性措施，但不能保证100%不被检测

## 许可证

MIT 
