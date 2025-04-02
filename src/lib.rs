pub mod scanner;
pub mod patterns;
pub mod http;
pub mod utils;

use std::path::PathBuf;
use thiserror::Error;
use serde::{Serialize, Deserialize};

#[derive(Error, Debug)]
pub enum BackerError {
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("HTTP错误: {0}")]
    Http(#[from] reqwest::Error),
    
    #[error("URL解析错误: {0}")]
    Url(#[from] url::ParseError),
    
    #[error("JSON错误: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("CSV错误: {0}")]
    Csv(#[from] csv::Error),
    
    #[error("配置错误: {0}")]
    Config(String),
    
    #[error("扫描错误: {0}")]
    Scan(String),
    
    #[error("其它错误: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BackerError>;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// 目标站点文件
    pub targets_file: PathBuf,
    /// 自定义备份文件模式列表
    pub patterns_file: Option<PathBuf>,
    /// 并发线程数
    pub threads: usize,
    /// 超时时间(秒)
    pub timeout: u64,
    /// 失败重试次数
    pub retry_count: u32,
    /// User-Agent
    pub user_agent: String,
    /// 输出格式
    pub output_format: OutputFormat,
    /// 输出文件
    pub output_file: Option<PathBuf>,
    /// 是否验证文件内容
    pub verify_content: bool,
    /// 是否启用调试模式
    pub debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Json,
    Csv,
    Markdown,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            targets_file: PathBuf::new(),
            patterns_file: None,
            threads: 10,
            timeout: 30,
            retry_count: 3,
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36".to_string(),
            output_format: OutputFormat::Json,
            output_file: None,
            verify_content: false,
            debug: false,
        }
    }
}

/// 扫描结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// 发现的URL
    pub url: String,
    /// HTTP状态码
    pub status_code: u16,
    /// 内容类型（Content-Type）
    pub content_type: Option<String>,
    /// 内容长度（Content-Length）
    pub content_length: Option<u64>,
    /// 是否已验证文件内容
    pub verified: bool,
}
