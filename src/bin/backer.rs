use backer::{OutputFormat, Result, ScanConfig};
use backer::scanner::Scanner;
use backer::utils::{load_targets, save_results, get_random_user_agent};
use clap::{Parser, ValueEnum};
use env_logger::Env;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(
    name = "backer",
    about = "一个高性能、多线程的网站备份文件扫描工具",
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
)]
struct Cli {
    /// 目标网站列表文件路径（每行一个URL）
    #[clap(short, long, value_name = "FILE")]
    targets: PathBuf,
    
    /// 自定义备份文件模式列表（每行一个模式）
    #[clap(short, long, value_name = "FILE")]
    patterns: Option<PathBuf>,
    
    /// 并发线程数量
    #[clap(short = 'j', long, default_value = "10")]
    threads: usize,
    
    /// 请求超时时间（秒）
    #[clap(short = 'T', long, default_value = "30")]
    timeout: u64,
    
    /// 请求失败重试次数
    #[clap(short = 'r', long, default_value = "3")]
    retry: u32,
    
    /// 自定义User-Agent
    #[clap(short = 'a', long)]
    user_agent: Option<String>,
    
    /// 输出格式
    #[clap(short, long, value_enum, default_value = "json")]
    format: Format,
    
    /// 结果输出文件路径
    #[clap(short = 'o', long, value_name = "FILE")]
    output: Option<PathBuf>,
    
    /// 验证文件内容（会下载文件头部）
    #[clap(short = 'v', long)]
    verify: bool,
    
    /// 启用调试日志
    #[clap(short, long)]
    debug: bool,
    
    /// 使用随机请求头
    #[clap(long)]
    random_headers: bool,
    
    /// 使用随机IP (X-Forwarded-For)
    #[clap(long)]
    random_ip: bool,
    
    /// 禁用随机请求头 (默认启用)
    #[clap(long)]
    no_random_headers: bool,
    
    /// 禁用随机IP (默认启用)
    #[clap(long)]
    no_random_ip: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Format {
    Json,
    Csv,
    Markdown,
}

impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Json => OutputFormat::Json,
            Format::Csv => OutputFormat::Csv,
            Format::Markdown => OutputFormat::Markdown,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 解析命令行参数
    let cli = Cli::parse();
    
    // 配置日志级别，如果debug开启则设置为debug，否则为error
    let log_level = if cli.debug { "debug" } else { "error" };
    env_logger::Builder::from_env(Env::default().default_filter_or(log_level))
        .format_timestamp_millis()
        .init();
    
    // 加载目标站点（使用异步函数）
    let targets = match load_targets(&cli.targets).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("加载目标站点失败: {}", e);
            return Ok(());
        }
    };
        
    if targets.is_empty() {
        eprintln!("没有找到有效的目标站点");
        return Ok(());
    }
    
    // 获取User-Agent
    let user_agent = if let Some(ua) = cli.user_agent {
        ua
    } else {
        get_random_user_agent()
    };
    
    // 创建扫描配置
    let config = ScanConfig {
        targets_file: cli.targets.clone(),
        patterns_file: cli.patterns.clone(),
        threads: cli.threads,
        timeout: cli.timeout,
        retry_count: cli.retry,
        user_agent,
        output_format: cli.format.into(),
        output_file: cli.output.clone(),
        verify_content: cli.verify,
        debug: cli.debug,
    };
    
    // 创建扫描器
    let mut scanner = Scanner::new(config).await?;
    
    // 配置随机请求头和随机IP
    if cli.random_headers && !cli.no_random_headers {
        scanner.set_random_headers(true);
    } else if cli.no_random_headers {
        scanner.set_random_headers(false);
    }
    
    if cli.random_ip && !cli.no_random_ip {
        scanner.set_random_ip(true);
    } else if cli.no_random_ip {
        scanner.set_random_ip(false);
    }
    
    // 设置debug模式
    scanner.set_debug(cli.debug);
    
    // 打印扫描配置信息
    println!("扫描配置:");
    println!("  目标文件: {}", cli.targets.display());
    if let Some(ref patterns) = cli.patterns {
        println!("  模式文件: {}", patterns.display());
    }
    println!("  线程数: {}", cli.threads);
    println!("  超时: {} 秒", cli.timeout);
    println!("  重试次数: {}", cli.retry);
    println!("  随机请求头: {}", !cli.no_random_headers);
    println!("  随机IP: {}", !cli.no_random_ip);
    println!("  验证内容: {}", cli.verify);
    
    // 设置全局超时保护，防止程序永久卡住
    let total_timeout = std::cmp::max(cli.timeout * 5, 60); // 至少60秒，最多是超时的5倍
    
    // 用超时包装扫描过程
    let scan_result = tokio::time::timeout(
        std::time::Duration::from_secs(total_timeout),
        scanner.scan(targets)
    ).await;
    
    let results = match scan_result {
        Ok(result) => match result {
            Ok(results) => results,
            Err(e) => {
                eprintln!("扫描过程中发生错误: {}", e);
                return Ok(());
            }
        },
        Err(_) => {
            eprintln!("扫描总体超时，程序将退出");
            return Ok(());
        }
    };
    
    // 保存结果
    if !results.is_empty() && cli.output.is_some() {
        save_results(&results, cli.format.into(), cli.output.as_ref())?;
    }
    
    Ok(())
}
